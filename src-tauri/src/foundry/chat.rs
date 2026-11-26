use std::collections::HashMap;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::ipc::Channel;
use tokio::sync::watch;

use super::db::{
    Attachment, ChatMessage, Conversation, Database, MessageRole, MessageStatus, StreamBuffer,
    TokenUsage,
};
use super::error::{FoundryError, FoundryResult};
use super::harmony::HarmonyStream;
use super::http::{StreamTimeouts, await_between_tokens, await_first_token};
use super::jobs::now_millis;

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatParams {
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub top_p: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
    #[serde(default)]
    pub presence_penalty: Option<f64>,
    #[serde(default)]
    pub frequency_penalty: Option<f64>,
    #[serde(default)]
    pub seed: Option<i64>,
    #[serde(default)]
    pub stop: Option<Vec<String>>,
    #[serde(default)]
    pub reasoning_effort: Option<ReasoningEffort>,
    #[serde(default)]
    pub response_verbosity: Option<ResponseVerbosity>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResponseVerbosity {
    Short,
    Balanced,
    Detailed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum GenerationParam {
    ReasoningEffort,
    ResponseVerbosity,
    Temperature,
    TopP,
    MaxTokens,
    Seed,
    Stop,
}

impl GenerationParam {
    pub const ALL: [GenerationParam; 7] = [
        GenerationParam::ReasoningEffort,
        GenerationParam::ResponseVerbosity,
        GenerationParam::Temperature,
        GenerationParam::TopP,
        GenerationParam::MaxTokens,
        GenerationParam::Seed,
        GenerationParam::Stop,
    ];
}

impl ChatParams {
    pub fn validate(self) -> FoundryResult<Self> {
        if let Some(temperature) = self.temperature
            && !(0.0..=2.0).contains(&temperature)
        {
            return Err(invalid("temperature must be between 0 and 2"));
        }

        if let Some(top_p) = self.top_p
            && !(0.0..=1.0).contains(&top_p)
        {
            return Err(invalid("top_p must be between 0 and 1"));
        }

        if let Some(max_tokens) = self.max_tokens
            && !(1..=1_000_000).contains(&max_tokens)
        {
            return Err(invalid("max_tokens must be between 1 and 1000000"));
        }

        for penalty in [self.presence_penalty, self.frequency_penalty]
            .into_iter()
            .flatten()
        {
            if !(-2.0..=2.0).contains(&penalty) {
                return Err(invalid("penalties must be between -2 and 2"));
            }
        }

        if self
            .stop
            .as_ref()
            .is_some_and(|stop| stop.iter().any(|value| value.is_empty()))
        {
            return Err(invalid("stop sequences cannot be empty"));
        }

        Ok(self)
    }

    pub fn is_empty(&self) -> bool {
        self.temperature.is_none()
            && self.top_p.is_none()
            && self.max_tokens.is_none()
            && self.presence_penalty.is_none()
            && self.frequency_penalty.is_none()
            && self.seed.is_none()
            && self.stop.as_ref().is_none_or(|stop| stop.is_empty())
            && self.reasoning_effort.is_none()
            && self.response_verbosity.is_none()
    }

    pub fn apply(&self, payload: &mut Value) {
        let Some(fields) = payload.as_object_mut() else {
            return;
        };

        if let Some(value) = self.temperature {
            fields.insert("temperature".to_string(), json!(value));
        }

        if let Some(value) = self.top_p {
            fields.insert("top_p".to_string(), json!(value));
        }

        if let Some(value) = self.max_tokens {
            fields.insert("max_tokens".to_string(), json!(value));
        }

        if let Some(value) = self.presence_penalty {
            fields.insert("presence_penalty".to_string(), json!(value));
        }

        if let Some(value) = self.frequency_penalty {
            fields.insert("frequency_penalty".to_string(), json!(value));
        }

        if let Some(value) = self.seed {
            fields.insert("seed".to_string(), json!(value));
        }

        if let Some(value) = &self.stop {
            fields.insert("stop".to_string(), json!(value));
        }
    }

    pub fn filter_supported(&self, supported: &[GenerationParam]) -> Self {
        let supports = |field| supported.contains(&field);

        Self {
            temperature: supports(GenerationParam::Temperature)
                .then_some(self.temperature)
                .flatten(),
            top_p: supports(GenerationParam::TopP)
                .then_some(self.top_p)
                .flatten(),
            max_tokens: supports(GenerationParam::MaxTokens)
                .then_some(self.max_tokens)
                .flatten(),
            presence_penalty: self.presence_penalty,
            frequency_penalty: self.frequency_penalty,
            seed: supports(GenerationParam::Seed)
                .then_some(self.seed)
                .flatten(),
            stop: supports(GenerationParam::Stop)
                .then_some(self.stop.clone())
                .flatten(),
            reasoning_effort: supports(GenerationParam::ReasoningEffort)
                .then_some(self.reasoning_effort)
                .flatten(),
            response_verbosity: supports(GenerationParam::ResponseVerbosity)
                .then_some(self.response_verbosity)
                .flatten(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendReceipt {
    pub request_id: String,
    pub conversation: Conversation,
    pub user_message: ChatMessage,
    pub assistant_message: ChatMessage,
    pub removed: Vec<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum StreamEvent {
    Started {
        request_id: String,
        conversation: Conversation,
        user_message: Box<ChatMessage>,
        assistant_message: Box<ChatMessage>,
    },
    Delta {
        request_id: String,
        message_id: i64,
        content: String,
        reasoning: String,
    },
    Usage {
        request_id: String,
        message_id: i64,
        usage: TokenUsage,
    },
    Done {
        request_id: String,
        message: Box<ChatMessage>,
        conversation: Conversation,
        cancelled: bool,
    },
    Error {
        request_id: String,
        message: Box<ChatMessage>,
        conversation: Conversation,
        error: FoundryError,
    },
}

#[derive(Debug)]
pub struct GenerationRegistry {
    running: Mutex<HashMap<String, RunningGeneration>>,
}

#[derive(Debug)]
struct RunningGeneration {
    conversation_id: i64,
    cancellation: watch::Sender<bool>,
}

pub struct GenerationGuard {
    registry: Arc<GenerationRegistry>,
    request_id: String,
}

pub struct GenerationStart {
    pub request_id: String,
    pub cancellation: watch::Receiver<bool>,
    pub guard: GenerationGuard,
}

impl Drop for GenerationGuard {
    fn drop(&mut self) {
        self.registry.finish(&self.request_id);
    }
}

impl Default for GenerationRegistry {
    fn default() -> Self {
        Self {
            running: Mutex::new(HashMap::new()),
        }
    }
}

impl GenerationRegistry {
    pub fn begin(self: &Arc<Self>, conversation_id: i64) -> Option<GenerationStart> {
        let mut running = lock(&self.running);

        if running
            .values()
            .any(|generation| generation.conversation_id == conversation_id)
        {
            return None;
        }

        let request_id = format!(
            "req-{}-{}",
            now_millis(),
            REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let (cancellation, receiver) = watch::channel(false);

        running.insert(
            request_id.clone(),
            RunningGeneration {
                conversation_id,
                cancellation,
            },
        );

        Some(GenerationStart {
            request_id: request_id.clone(),
            cancellation: receiver,
            guard: GenerationGuard {
                registry: self.clone(),
                request_id,
            },
        })
    }

    pub fn cancel(&self, request_id: &str) -> bool {
        match lock(&self.running).get(request_id) {
            Some(generation) => generation.cancellation.send(true).is_ok(),
            None => false,
        }
    }

    pub fn with_idle_conversations<T>(
        &self,
        ids: &[i64],
        operation: impl FnOnce() -> T,
    ) -> Option<T> {
        self.with_generation_barrier(|running| {
            if running.iter().any(|id| ids.contains(id)) {
                None
            } else {
                Some(operation())
            }
        })
    }

    pub fn with_generation_barrier<T>(&self, operation: impl FnOnce(&[i64]) -> T) -> T {
        let running = lock(&self.running);
        let ids = running
            .values()
            .map(|generation| generation.conversation_id)
            .collect::<Vec<_>>();
        let result = operation(&ids);
        drop(running);
        result
    }

    fn finish(&self, request_id: &str) {
        lock(&self.running).remove(request_id);
    }
}

pub trait StreamSink: Send + Sync {
    fn emit(&self, event: StreamEvent);
}

impl StreamSink for Channel<StreamEvent> {
    fn emit(&self, event: StreamEvent) {
        let _ = self.send(event);
    }
}

pub struct GenerationContext {
    pub request_id: String,
    pub database: Arc<Database>,
    pub endpoint: super::local_inference::VerifiedLocalEndpoint,
    pub model_id: String,
    pub conversation: Conversation,
    pub user_message: ChatMessage,
    pub assistant_message: ChatMessage,
    pub params: Option<ChatParams>,
    pub supports_reasoning: bool,
    pub timeouts: StreamTimeouts,
    pub cancellation: watch::Receiver<bool>,
    pub _guard: GenerationGuard,
}

pub async fn run_generation<S: StreamSink>(context: GenerationContext, sink: S) {
    sink.emit(StreamEvent::Started {
        request_id: context.request_id.clone(),
        conversation: context.conversation.clone(),
        user_message: Box::new(context.user_message.clone()),
        assistant_message: Box::new(context.assistant_message.clone()),
    });

    let outcome = stream_completion(&context, &sink).await;

    match outcome {
        Ok(StreamOutcome::Completed {
            content,
            reasoning,
            usage,
        }) => {
            finish(
                &context,
                &sink,
                MessageStatus::Complete,
                &content,
                &reasoning,
                usage,
                None,
                false,
            )
            .await;
        }
        Ok(StreamOutcome::Cancelled {
            content,
            reasoning,
            usage,
        }) => {
            finish(
                &context,
                &sink,
                MessageStatus::Cancelled,
                &content,
                &reasoning,
                usage,
                None,
                true,
            )
            .await;
        }
        Err(failure) => {
            let failure = *failure;

            finish(
                &context,
                &sink,
                MessageStatus::Error,
                &failure.content,
                &failure.reasoning,
                failure.usage,
                Some(&failure.error),
                false,
            )
            .await;
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn finish<S: StreamSink>(
    context: &GenerationContext,
    sink: &S,
    status: MessageStatus,
    content: &str,
    reasoning: &str,
    usage: Option<TokenUsage>,
    error: Option<&FoundryError>,
    cancelled: bool,
) {
    let message = match context.database.finish_message(
        context.assistant_message.id,
        status,
        content,
        reasoning,
        usage,
        error,
    ) {
        Ok(message) => message,
        Err(failure) => {
            sink.emit(StreamEvent::Error {
                request_id: context.request_id.clone(),
                message: Box::new(context.assistant_message.clone()),
                conversation: context.conversation.clone(),
                error: failure,
            });

            return;
        }
    };
    let conversation = context
        .database
        .touch_conversation(context.conversation.id)
        .unwrap_or_else(|_| context.conversation.clone());

    match error {
        Some(error) => {
            sink.emit(StreamEvent::Error {
                request_id: context.request_id.clone(),
                message: Box::new(message),
                conversation,
                error: error.clone(),
            });
        }
        None => {
            sink.emit(StreamEvent::Done {
                request_id: context.request_id.clone(),
                message: Box::new(message),
                conversation,
                cancelled,
            });
        }
    }
}

enum StreamOutcome {
    Completed {
        content: String,
        reasoning: String,
        usage: Option<TokenUsage>,
    },
    Cancelled {
        content: String,
        reasoning: String,
        usage: Option<TokenUsage>,
    },
}

struct StreamFailureData {
    error: FoundryError,
    content: String,
    reasoning: String,
    usage: Option<TokenUsage>,
}

type StreamFailure = Box<StreamFailureData>;

fn failed(error: FoundryError, buffer: &StreamBuffer, usage: Option<TokenUsage>) -> StreamFailure {
    Box::new(StreamFailureData {
        error,
        content: buffer.content().to_string(),
        reasoning: buffer.reasoning().to_string(),
        usage,
    })
}

async fn stream_completion<S: StreamSink>(
    context: &GenerationContext,
    sink: &S,
) -> Result<StreamOutcome, StreamFailure> {
    let mut buffer = StreamBuffer::new(
        context.assistant_message.content.clone(),
        context.assistant_message.reasoning.clone(),
    );
    let mut harmony = HarmonyStream::new();
    let mut usage: Option<TokenUsage> = None;
    let mut cancellation = context.cancellation.clone();

    if *cancellation.borrow() {
        return Ok(StreamOutcome::Cancelled {
            content: buffer.content().to_string(),
            reasoning: buffer.reasoning().to_string(),
            usage,
        });
    }

    let history = match context
        .database
        .messages_for_prompt(context.conversation.id, context.assistant_message.id)
    {
        Ok(history) => history,
        Err(error) => return cancelled_or_failed(&mut cancellation, error, &buffer, usage),
    };
    let messages = history
        .iter()
        .filter(|message| message.content.trim() != "" || !message.attachments.is_empty())
        .map(|message| {
            message_content(&context.database, message).map(|content| {
                json!({
                    "role": match message.role {
                        MessageRole::User => "user",
                        MessageRole::Assistant => "assistant",
                        MessageRole::Divider => "system",
                    },
                    "content": content,
                })
            })
        })
        .collect::<FoundryResult<Vec<Value>>>();
    let mut messages = match messages {
        Ok(messages) => messages,
        Err(error) => return cancelled_or_failed(&mut cancellation, error, &buffer, usage),
    };

    if let Some(params) = &context.params
        && let Some(instruction) = behavior_instruction(params, context.supports_reasoning)
    {
        messages.insert(0, json!({"role": "system", "content": instruction}));
    }

    let messages = match super::agent_runtime::with_approved_instructions(
        &context.database.agent_store(),
        context.conversation.id,
        messages,
        262_144,
    ) {
        Ok(messages) => messages,
        Err(detail) => {
            return cancelled_or_failed(
                &mut cancellation,
                FoundryError::CommandFailed {
                    command: "instruction_context".into(),
                    message: detail,
                    hint: None,
                },
                &buffer,
                usage,
            );
        }
    };
    let messages = super::agent_runtime::with_retrieved_context(
        &context.database.agent_store(),
        context.conversation.id,
        messages,
        16_384,
    );
    let plan = super::agent_runtime::plan_context(super::agent_runtime::ContextPlanRequest {
        messages,
        capacity_tokens: None,
        max_input_bytes: Some(262_144),
        requested_output_tokens: context
            .params
            .as_ref()
            .and_then(|params| params.max_tokens)
            .map(|tokens| tokens as usize),
    });
    let store = context.database.agent_store();
    if let (Ok(revision), Ok(manifest)) = (
        store.authorization_revision(context.conversation.id),
        serde_json::to_string(&plan.manifest),
    ) {
        let _ = store.record_context_budget(
            context.conversation.id,
            revision.key().context_revision,
            None,
            &format!("context_budget.message.{}", context.assistant_message.id),
            &manifest,
        );
    }
    let messages = plan.messages;
    let uses_images = messages.iter().any(has_image_content);

    let (route, payload) = if uses_images {
        let input = messages.into_iter().map(to_responses_message).collect::<Vec<_>>();
        let mut payload = json!({
            "model": context.model_id,
            "input": input,
            "stream": true,
        });

        if let Some(params) = &context.params {
            apply_responses_params(&mut payload, params);
        }

        ("/v1/responses", payload)
    } else {
        let mut payload = json!({
            "model": context.model_id,
            "messages": messages,
            "stream": true,
            "stream_options": { "include_usage": true },
        });

        if let Some(params) = &context.params {
            params.apply(&mut payload);
            apply_native_behavior_controls(&mut payload, params, context.supports_reasoning);
        }

        ("/v1/chat/completions", payload)
    };

    let request = context.endpoint.post_json_stream(
        route,
        &payload,
        context.timeouts.first_token,
    );
    let responses_mode = uses_images;

    let response = match with_cancel(&mut cancellation, request).await {
        None => {
            return Ok(StreamOutcome::Cancelled {
                content: buffer.content().to_string(),
                reasoning: buffer.reasoning().to_string(),
                usage,
            });
        }
        Some(Err(error)) => {
            return Err(failed(
                super::http::local_inference_error(error),
                &buffer,
                usage,
            ));
        }
        Some(Ok(response)) => response,
    };

    let status = response.status();

    if !status.is_success() {
        let detail = response.text().await.unwrap_or_else(|_| status.to_string());

        let error = if is_missing_model(&detail) || status == reqwest::StatusCode::NOT_FOUND {
            FoundryError::ModelNotLoaded {
                alias: context.conversation.model_alias.clone(),
            }
        } else {
            FoundryError::BadResponse {
                detail: format!("HTTP {status}: {detail}"),
            }
        };

        return Err(failed(error, &buffer, usage));
    }

    let mut stream = response.bytes_stream();
    let mut pending = String::new();
    let mut first_frame = true;
    let mut current_event = String::new();

    loop {
        let next = with_cancel(&mut cancellation, async {
            if first_frame {
                await_first_token(stream.next(), context.timeouts).await
            } else {
                await_between_tokens(stream.next(), context.timeouts).await
            }
        })
        .await;

        let Some(next) = next else {
            drain_harmony(context, sink, &mut buffer, &mut harmony);
            flush(&context.database, context.assistant_message.id, &mut buffer);

            return Ok(StreamOutcome::Cancelled {
                content: buffer.content().to_string(),
                reasoning: buffer.reasoning().to_string(),
                usage,
            });
        };

        let chunk = match next {
            Ok(chunk) => chunk,
            Err(error) => {
                return Err(failed(error, &buffer, usage));
            }
        };

        let Some(read) = chunk else {
            break;
        };

        let bytes = match read {
            Ok(bytes) => bytes,
            Err(error) => {
                return Err(failed(
                    stream_transport_error(error, context.timeouts.between_tokens.as_secs()),
                    &buffer,
                    usage,
                ));
            }
        };

        pending.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(index) = pending.find('\n') {
            let line = pending[..index].trim_end_matches('\r').to_string();

            pending.drain(..=index);

            if line.trim().is_empty() {
                continue;
            }

            first_frame = false;

            match parse_stream_line(&line, responses_mode, &mut current_event) {
                Ok(Some(Frame::Delta {
                    content,
                    reasoning,
                    usage: reported,
                })) => {
                    absorb_delta(
                        context,
                        sink,
                        &mut buffer,
                        &mut harmony,
                        &content,
                        &reasoning,
                        reported,
                        &mut usage,
                    );
                }
                Ok(Some(Frame::Usage(reported))) => {
                    report_usage(context, sink, &mut usage, reported);
                }
                Ok(Some(Frame::Done)) | Ok(None) => {}
                Err(error) => {
                    return Err(failed(error, &buffer, usage));
                }
            }
        }
    }

    if !pending.trim().is_empty() {
        match parse_stream_line(&pending, responses_mode, &mut current_event) {
            Ok(Some(Frame::Delta {
                content,
                reasoning,
                usage: reported,
            })) => {
                absorb_delta(
                    context,
                    sink,
                    &mut buffer,
                    &mut harmony,
                    &content,
                    &reasoning,
                    reported,
                    &mut usage,
                );
            }
            Ok(Some(Frame::Usage(reported))) => {
                report_usage(context, sink, &mut usage, reported);
            }
            _ => {}
        }
    }

    drain_harmony(context, sink, &mut buffer, &mut harmony);
    flush(&context.database, context.assistant_message.id, &mut buffer);

    Ok(StreamOutcome::Completed {
        content: buffer.content().to_string(),
        reasoning: buffer.reasoning().to_string(),
        usage,
    })
}

#[allow(clippy::too_many_arguments)]
fn absorb_delta<S: StreamSink>(
    context: &GenerationContext,
    sink: &S,
    buffer: &mut StreamBuffer,
    harmony: &mut HarmonyStream,
    content: &str,
    reasoning: &str,
    reported: Option<TokenUsage>,
    usage: &mut Option<TokenUsage>,
) {
    let parsed = harmony.push(content);
    let reasoning = if parsed.reasoning.is_empty() {
        reasoning.to_string()
    } else if reasoning.is_empty() {
        parsed.reasoning
    } else {
        format!("{}{}", parsed.reasoning, reasoning)
    };

    buffer.append(&parsed.content, &reasoning);

    if !parsed.content.is_empty() || !reasoning.is_empty() {
        sink.emit(StreamEvent::Delta {
            request_id: context.request_id.clone(),
            message_id: context.assistant_message.id,
            content: parsed.content,
            reasoning,
        });
    }

    if let Some(reported) = reported {
        report_usage(context, sink, usage, reported);
    }

    if buffer.due() {
        flush(&context.database, context.assistant_message.id, buffer);
    }
}

fn drain_harmony<S: StreamSink>(
    context: &GenerationContext,
    sink: &S,
    buffer: &mut StreamBuffer,
    harmony: &mut HarmonyStream,
) {
    let tail = harmony.finish();

    buffer.append(&tail.content, &tail.reasoning);

    if !tail.content.is_empty() || !tail.reasoning.is_empty() {
        sink.emit(StreamEvent::Delta {
            request_id: context.request_id.clone(),
            message_id: context.assistant_message.id,
            content: tail.content,
            reasoning: tail.reasoning,
        });
    }
}

fn message_content(database: &Database, message: &ChatMessage) -> FoundryResult<Value> {
    let images: Vec<&Attachment> = message
        .attachments
        .iter()
        .filter(|attachment| attachment.kind == "image")
        .collect();

    if images.is_empty() {
        return Ok(Value::String(message.content.clone()));
    }

    let mut parts = Vec::new();

    if !message.content.trim().is_empty() {
        parts.push(json!({"type": "text", "text": message.content}));
    }

    for image in images {
        let url = database.attachment_data(image.id)?;
        parts.push(json!({"type": "image_url", "image_url": {"url": url}}));
    }

    Ok(Value::Array(parts))
}

fn report_usage<S: StreamSink>(
    context: &GenerationContext,
    sink: &S,
    usage: &mut Option<TokenUsage>,
    reported: TokenUsage,
) {
    *usage = Some(reported);
    sink.emit(StreamEvent::Usage {
        request_id: context.request_id.clone(),
        message_id: context.assistant_message.id,
        usage: reported,
    });
}

fn apply_native_behavior_controls(
    payload: &mut Value,
    params: &ChatParams,
    supports_reasoning: bool,
) {
    if !supports_reasoning {
        return;
    }

    let Some(reasoning_effort) = params.reasoning_effort else {
        return;
    };

    let Some(fields) = payload.as_object_mut() else {
        return;
    };

    fields.insert("reasoning_effort".to_string(), json!(reasoning_effort));
}

fn behavior_instruction(params: &ChatParams, supports_reasoning: bool) -> Option<String> {
    let mut instructions = Vec::new();

    if !supports_reasoning {
        if let Some(reasoning_effort) = params.reasoning_effort {
            let text = match reasoning_effort {
                ReasoningEffort::Low => {
                    "Use minimal internal reasoning before answering and favor direct outputs."
                }
                ReasoningEffort::Medium => {
                    "Use a moderate amount of internal reasoning before answering."
                }
                ReasoningEffort::High => {
                    "Use deeper internal reasoning before answering and validate key assumptions."
                }
            };

            instructions.push(text.to_string());
        }
    }

    if let Some(response_verbosity) = params.response_verbosity {
        let text = match response_verbosity {
            ResponseVerbosity::Short => {
                "Keep the final response concise and direct, with short paragraphs and minimal bullets."
            }
            ResponseVerbosity::Balanced => {
                "Keep the final response balanced: clear and complete without excessive detail."
            }
            ResponseVerbosity::Detailed => {
                "Provide a detailed final response with clear structure and explicit tradeoffs when relevant."
            }
        };

        instructions.push(text.to_string());
    }

    if instructions.is_empty() {
        None
    } else {
        Some(instructions.join(" "))
    }
}

fn flush(database: &Database, message_id: i64, buffer: &mut StreamBuffer) {
    if database
        .update_streaming(message_id, buffer.content(), buffer.reasoning())
        .is_ok()
    {
        buffer.mark_flushed();
    }
}

fn cancelled_or_failed(
    cancellation: &mut watch::Receiver<bool>,
    error: FoundryError,
    buffer: &StreamBuffer,
    usage: Option<TokenUsage>,
) -> Result<StreamOutcome, StreamFailure> {
    if *cancellation.borrow() {
        return Ok(StreamOutcome::Cancelled {
            content: buffer.content().to_string(),
            reasoning: buffer.reasoning().to_string(),
            usage,
        });
    }

    Err(failed(error, buffer, usage))
}

async fn with_cancel<T>(
    cancellation: &mut watch::Receiver<bool>,
    future: impl Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        changed = cancellation.changed() => {
            let _ = changed;
            None
        }
        value = future => Some(value),
    }
}

#[derive(Debug, PartialEq)]
enum Frame {
    Delta {
        content: String,
        reasoning: String,
        usage: Option<TokenUsage>,
    },
    Usage(TokenUsage),
    Done,
}

#[derive(Deserialize)]
struct ChatChunk {
    #[serde(default)]
    choices: Vec<ChatChoice>,
    #[serde(default)]
    usage: Option<ApiUsage>,
}

#[derive(Deserialize)]
struct ChatChoice {
    #[serde(default)]
    delta: ChatDelta,
}

#[derive(Default, Deserialize)]
struct ChatDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
}

#[derive(Deserialize)]
struct ApiUsage {
    #[serde(default)]
    prompt_tokens: i64,
    #[serde(default)]
    completion_tokens: i64,
    #[serde(default)]
    total_tokens: i64,
}

fn parse_frame(line: &str) -> FoundryResult<Option<Frame>> {
    let trimmed = line.trim();

    if trimmed.is_empty() || trimmed.starts_with(':') {
        return Ok(None);
    }

    let Some(data) = trimmed.strip_prefix("data:") else {
        return Ok(None);
    };
    let data = data.trim();

    if data == "[DONE]" {
        return Ok(Some(Frame::Done));
    }

    let chunk: ChatChunk =
        serde_json::from_str(data).map_err(|error| FoundryError::BadResponse {
            detail: format!("invalid stream frame: {error}"),
        })?;

    let usage = chunk.usage.map(|usage| TokenUsage {
        prompt_tokens: usage.prompt_tokens,
        completion_tokens: usage.completion_tokens,
        total_tokens: usage.total_tokens,
    });

    let content = chunk
        .choices
        .iter()
        .filter_map(|choice| choice.delta.content.as_deref())
        .collect::<String>();
    let reasoning = chunk
        .choices
        .iter()
        .filter_map(|choice| choice.delta.reasoning_content.as_deref())
        .collect::<String>();

    if content.is_empty() && reasoning.is_empty() {
        return Ok(usage.map(Frame::Usage));
    }

    Ok(Some(Frame::Delta {
        content,
        reasoning,
        usage,
    }))
}

fn parse_stream_line(
    line: &str,
    responses: bool,
    current_event: &mut String,
) -> FoundryResult<Option<Frame>> {
    if responses {
        parse_responses_line(line, current_event)
    } else {
        parse_frame(line)
    }
}

fn parse_responses_line(line: &str, current_event: &mut String) -> FoundryResult<Option<Frame>> {
    let trimmed = line.trim();

    if trimmed.is_empty() || trimmed.starts_with(':') {
        return Ok(None);
    }

    if let Some(event) = trimmed.strip_prefix("event:") {
        *current_event = event.trim().to_string();
        return Ok(None);
    }

    let Some(data) = trimmed.strip_prefix("data:") else {
        return Ok(None);
    };
    let data = data.trim();

    if data == "[DONE]" {
        return Ok(Some(Frame::Done));
    }

    let chunk: Value = serde_json::from_str(data).map_err(|error| FoundryError::BadResponse {
        detail: format!("invalid responses stream frame: {error}"),
    })?;

    match current_event.as_str() {
        "response.output_text.delta" => {
            let content = chunk
                .get("delta")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Ok(Some(Frame::Delta {
                content,
                reasoning: String::new(),
                usage: None,
            }))
        }
        "response.completed" => {
            let usage = chunk
                .get("response")
                .and_then(|response| response.get("usage"))
                .map(|usage| TokenUsage {
                    prompt_tokens: usage.get("input_tokens").and_then(Value::as_i64).unwrap_or(0),
                    completion_tokens: usage
                        .get("output_tokens")
                        .and_then(Value::as_i64)
                        .unwrap_or(0),
                    total_tokens: usage.get("total_tokens").and_then(Value::as_i64).unwrap_or(0),
                });
            Ok(usage.map(Frame::Usage))
        }
        _ => Ok(None),
    }
}

fn has_image_content(message: &Value) -> bool {
    message
        .get("content")
        .and_then(Value::as_array)
        .is_some_and(|parts| {
            parts
                .iter()
                .any(|part| part.get("type").and_then(Value::as_str) == Some("image_url"))
        })
}

fn to_responses_message(message: Value) -> Value {
    let Some(parts) = message.get("content").and_then(Value::as_array) else {
        return message;
    };
    let converted: Vec<Value> = parts
        .iter()
        .map(|part| match part.get("type").and_then(Value::as_str) {
            Some("text") => json!({
                "type": "input_text",
                "text": part.get("text").cloned().unwrap_or(Value::Null),
            }),
            Some("image_url") => {
                let url = part
                    .get("image_url")
                    .and_then(|value| value.get("url"))
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                json!({ "type": "input_image", "image_url": url })
            }
            _ => part.clone(),
        })
        .collect();
    let mut object = message.as_object().cloned().unwrap_or_default();
    object.insert("content".to_string(), Value::Array(converted));
    Value::Object(object)
}

fn apply_responses_params(payload: &mut Value, params: &ChatParams) {
    let Some(fields) = payload.as_object_mut() else {
        return;
    };

    if let Some(value) = params.temperature {
        fields.insert("temperature".to_string(), json!(value));
    }
    if let Some(value) = params.top_p {
        fields.insert("top_p".to_string(), json!(value));
    }
    if let Some(value) = params.max_tokens {
        fields.insert("max_output_tokens".to_string(), json!(value));
    }
    if let Some(value) = params.presence_penalty {
        fields.insert("presence_penalty".to_string(), json!(value));
    }
    if let Some(value) = params.frequency_penalty {
        fields.insert("frequency_penalty".to_string(), json!(value));
    }
    if let Some(value) = params.seed {
        fields.insert("seed".to_string(), json!(value));
    }
    if let Some(value) = &params.stop {
        fields.insert("stop".to_string(), json!(value));
    }
}

fn is_missing_model(detail: &str) -> bool {
    let lower = detail.to_lowercase();

    lower.contains("model not found") || lower.contains("no model matching")
}

fn stream_transport_error(error: reqwest::Error, idle_seconds: u64) -> FoundryError {
    if error.is_timeout() {
        FoundryError::Timeout {
            stage: "token stream".to_string(),
            seconds: idle_seconds,
        }
    } else {
        FoundryError::ServerUnreachable {
            detail: error.to_string(),
        }
    }
}

fn invalid(detail: &str) -> FoundryError {
    FoundryError::BadResponse {
        detail: detail.to_string(),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_responses_stream_frames() {
        let mut event = String::new();

        assert_eq!(
            parse_stream_line("event: response.output_text.delta", true, &mut event)
                .expect("event"),
            None
        );
        assert_eq!(event, "response.output_text.delta");

        let frame = parse_stream_line(
            r#"data: {"delta":"Red","type":"response.output_text.delta"}"#,
            true,
            &mut event,
        )
        .expect("delta");
        match frame {
            Some(Frame::Delta {
                content,
                reasoning,
                usage,
            }) => {
                assert_eq!(content, "Red");
                assert!(reasoning.is_empty());
                assert!(usage.is_none());
            }
            other => panic!("expected a delta frame, got {other:?}"),
        }

        event.clear();
        assert_eq!(
            parse_stream_line("event: response.completed", true, &mut event).expect("event"),
            None
        );
        let frame = parse_stream_line(
            r#"data: {"response":{"usage":{"input_tokens":279,"output_tokens":1,"total_tokens":280}},"type":"response.completed"}"#,
            true,
            &mut event,
        )
        .expect("usage");
        match frame {
            Some(Frame::Usage(usage)) => {
                assert_eq!(usage.prompt_tokens, 279);
                assert_eq!(usage.completion_tokens, 1);
                assert_eq!(usage.total_tokens, 280);
            }
            other => panic!("expected a usage frame, got {other:?}"),
        }

        assert_eq!(
            parse_stream_line("data: [DONE]", true, &mut event).expect("done"),
            Some(Frame::Done)
        );
    }

    #[test]
    fn parses_content_reasoning_usage_and_done_frames() {
        let content =
            parse_frame(r#"data: {"choices":[{"delta":{"content":"hi"}}]}"#).expect("frame");        assert_eq!(
            content,
            Some(Frame::Delta {
                content: "hi".to_string(),
                reasoning: String::new(),
                usage: None
            })
        );

        let reasoning =
            parse_frame(r#"data: {"choices":[{"delta":{"reasoning_content":"thinking"}}]}"#)
                .expect("frame");
        assert_eq!(
            reasoning,
            Some(Frame::Delta {
                content: String::new(),
                reasoning: "thinking".to_string(),
                usage: None
            })
        );

        let combined = parse_frame(
            r#"data: {"choices":[{"delta":{"content":"hi"}}],"usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3}}"#,
        )
        .expect("frame");
        assert_eq!(
            combined,
            Some(Frame::Delta {
                content: "hi".to_string(),
                reasoning: String::new(),
                usage: Some(TokenUsage {
                    prompt_tokens: 1,
                    completion_tokens: 2,
                    total_tokens: 3
                })
            })
        );

        let usage = parse_frame(
            r#"data: {"choices":[],"usage":{"prompt_tokens":3,"completion_tokens":4,"total_tokens":7}}"#,
        )
        .expect("frame");
        assert_eq!(
            usage,
            Some(Frame::Usage(TokenUsage {
                prompt_tokens: 3,
                completion_tokens: 4,
                total_tokens: 7
            }))
        );

        assert_eq!(
            parse_frame("data: [DONE]").expect("done"),
            Some(Frame::Done)
        );
        assert_eq!(parse_frame(": keep-alive").expect("comment"), None);
        assert_eq!(parse_frame("").expect("blank"), None);
    }

    #[test]
    fn rejects_invalid_stream_frames() {
        let error = parse_frame("data: {not json").expect_err("invalid frame");

        assert_eq!(error.code(), "BadResponse");
    }

    #[test]
    fn validates_and_whitelists_generation_params() {
        let params = ChatParams {
            temperature: Some(0.7),
            top_p: Some(0.9),
            max_tokens: Some(256),
            seed: Some(42),
            stop: Some(vec!["END".to_string()]),
            ..ChatParams::default()
        }
        .validate()
        .expect("valid");
        let mut payload = json!({"model": "qwen3-0.6b", "stream": true});

        params.apply(&mut payload);

        assert_eq!(payload["temperature"], json!(0.7));
        assert_eq!(payload["top_p"], json!(0.9));
        assert_eq!(payload["max_tokens"], json!(256));
        assert_eq!(payload["seed"], json!(42));
        assert_eq!(payload["stop"], json!(["END"]));
        assert!(payload.get("presence_penalty").is_none());
    }

    #[test]
    fn rejects_out_of_range_params() {
        let error = ChatParams {
            temperature: Some(3.0),
            ..ChatParams::default()
        }
        .validate()
        .expect_err("temperature");

        assert_eq!(error.code(), "BadResponse");
        assert!(
            ChatParams {
                top_p: Some(1.5),
                ..ChatParams::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            ChatParams {
                max_tokens: Some(0),
                ..ChatParams::default()
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn behavior_instruction_is_built_when_needed() {
        let with_fallback = behavior_instruction(
            &ChatParams {
                reasoning_effort: Some(ReasoningEffort::High),
                response_verbosity: Some(ResponseVerbosity::Short),
                ..ChatParams::default()
            },
            false,
        )
        .expect("instruction");

        assert!(with_fallback.contains("deeper internal reasoning"));
        assert!(with_fallback.contains("concise and direct"));

        let without_reasoning_fallback = behavior_instruction(
            &ChatParams {
                reasoning_effort: Some(ReasoningEffort::Low),
                ..ChatParams::default()
            },
            true,
        );

        assert!(without_reasoning_fallback.is_none());
    }

    #[test]
    fn native_reasoning_effort_is_added_only_when_supported() {
        let mut payload = json!({"model": "qwen3-0.6b"});
        apply_native_behavior_controls(
            &mut payload,
            &ChatParams {
                reasoning_effort: Some(ReasoningEffort::Medium),
                ..ChatParams::default()
            },
            true,
        );

        assert_eq!(payload["reasoning_effort"], json!("medium"));

        let mut unsupported = json!({"model": "qwen3-0.6b"});
        apply_native_behavior_controls(
            &mut unsupported,
            &ChatParams {
                reasoning_effort: Some(ReasoningEffort::High),
                ..ChatParams::default()
            },
            false,
        );

        assert!(unsupported.get("reasoning_effort").is_none());
    }

    #[test]
    fn a_conversation_streams_one_generation_at_a_time() {
        let registry = Arc::new(GenerationRegistry::default());
        let start = registry.begin(7).expect("first starts");
        let GenerationStart {
            request_id,
            cancellation,
            guard,
        } = start;

        assert!(registry.begin(7).is_none());
        assert!(registry.cancel(&request_id));
        assert!(*cancellation.borrow());
        assert!(!registry.cancel("unknown"));

        drop(guard);
        assert!(registry.begin(7).is_some());
    }

    #[test]
    fn missing_model_errors_are_recognized() {
        assert!(is_missing_model(
            r#"{"error":{"message":"Model not found: No model matching 'qwen3-0.6b'"}}"#
        ));
        assert!(!is_missing_model("internal server error"));
    }

    type CancelAfterDelta = Option<(Arc<GenerationRegistry>, String)>;

    #[derive(Clone, Default)]
    struct Collector {
        events: Arc<Mutex<Vec<StreamEvent>>>,
        cancel: Arc<Mutex<CancelAfterDelta>>,
    }

    impl StreamSink for Collector {
        fn emit(&self, event: StreamEvent) {
            if matches!(event, StreamEvent::Delta { .. })
                && let Some((registry, request_id)) = self.cancel.lock().unwrap().take()
            {
                registry.cancel(&request_id);
            }

            self.events.lock().unwrap().push(event);
        }
    }

    fn test_context(
        database: Arc<Database>,
        endpoint: super::super::local_inference::VerifiedLocalEndpoint,
        cancel_after_delta: bool,
    ) -> (GenerationContext, Collector) {
        let conversation = database
            .create_conversation("qwen3-0.6b")
            .expect("conversation");
        let user_message = database
            .append_message(
                conversation.id,
                MessageRole::User,
                "Say hi in five words.",
                "",
                Some("qwen3-0.6b"),
                MessageStatus::Complete,
            )
            .expect("user message");
        let assistant_message = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "",
                "",
                Some("qwen3-0.6b"),
                MessageStatus::Streaming,
            )
            .expect("assistant message");
        let registry = Arc::new(GenerationRegistry::default());
        let GenerationStart {
            request_id,
            cancellation,
            guard,
        } = registry.begin(conversation.id).expect("generation starts");
        let collector = Collector::default();

        if cancel_after_delta {
            *collector.cancel.lock().unwrap() = Some((registry, request_id.clone()));
        }

        (
            GenerationContext {
                request_id,
                database,
                endpoint,
                model_id: "qwen3-0.6b-generic-gpu:2".to_string(),
                conversation,
                user_message,
                assistant_message,
                params: Some(ChatParams {
                    max_tokens: Some(256),
                    temperature: Some(0.2),
                    ..ChatParams::default()
                }),
                supports_reasoning: false,
                timeouts: StreamTimeouts::from_seconds(20, 20),
                cancellation,
                _guard: guard,
            },
            collector,
        )
    }

    #[tokio::test]
    async fn a_daemon_that_dies_mid_stream_reports_an_unreachable_server() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");

        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("connection");
            let mut buffer = [0u8; 2048];
            let _ = socket.read(&mut buffer);
            let body = "data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\n\n";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 4096\r\nConnection: close\r\n\r\n{body}"
            );
            let _ = socket.write_all(response.as_bytes());
        });

        let database = Arc::new(Database::open_in_memory().expect("database"));
        let endpoint = verified_fixture_endpoint(&format!("http://{address}"));
        let (context, collector) = test_context(database.clone(), endpoint, false);
        let message_id = context.assistant_message.id;

        run_generation(context, collector.clone()).await;

        let events = collector.events.lock().unwrap();
        let error = events.iter().find_map(|event| match event {
            StreamEvent::Error { error, .. } => Some(error),
            _ => None,
        });

        assert_eq!(error.expect("error event").code(), "ServerUnreachable");
        assert_eq!(
            database.message(message_id).expect("message").status,
            MessageStatus::Error
        );
    }

    #[tokio::test]
    async fn an_unreachable_server_reports_a_clear_error() {
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let endpoint = verified_fixture_endpoint(&format!(
            "http://{}",
            listener.local_addr().expect("address")
        ));
        drop(listener);
        let (context, collector) = test_context(database.clone(), endpoint, false);
        let message_id = context.assistant_message.id;

        run_generation(context, collector.clone()).await;

        let events = collector.events.lock().unwrap();
        let error = events.iter().find_map(|event| match event {
            StreamEvent::Error { error, .. } => Some(error),
            _ => None,
        });

        assert_eq!(error.expect("error event").code(), "CommandFailed");
        assert_eq!(
            database.message(message_id).expect("message").status,
            MessageStatus::Error
        );
    }

    #[tokio::test]
    async fn an_image_attachment_is_serialized_into_the_request_payload() {
        use std::io::{Read, Write};

        let database = Arc::new(Database::open_in_memory().expect("database"));
        let conversation = database
            .create_conversation("gemma-4-e2b-it")
            .expect("conversation");
        let user_message = database
            .append_message(
                conversation.id,
                MessageRole::User,
                "que animal é?",
                "",
                Some("gemma-4-e2b-it"),
                MessageStatus::Complete,
            )
            .expect("user message");
        let store = super::super::attachments::AttachmentStore::new();
        let staged = store
            .store("data:image/png;base64,iVBORw0KGgo=")
            .expect("image");
        let staged = store.take(&[staged.token]).expect("taken").remove(0);
        database
            .add_attachment(user_message.id, "image", &staged)
            .expect("attachment");
        let assistant_message = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "",
                "",
                Some("gemma-4-e2b-it"),
                MessageStatus::Streaming,
            )
            .expect("assistant message");

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        let address = listener.local_addr().expect("address");
        let captured = Arc::new(std::sync::Mutex::new(String::new()));
        let captured_for_thread = Arc::clone(&captured);
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().expect("connection");
            let mut request = Vec::new();
            let mut buffer = [0u8; 8192];
            let header_end = loop {
                let n = socket.read(&mut buffer).expect("read");
                if n == 0 {
                    break request.len();
                }
                request.extend_from_slice(&buffer[..n]);
                if let Some(pos) = request.windows(4).position(|window| window == b"\r\n\r\n") {
                    break pos + 4;
                }
            };
            let headers = String::from_utf8_lossy(&request[..header_end]).to_string();
            let content_length = headers
                .lines()
                .find_map(|line| {
                    line.strip_prefix("Content-Length: ")
                        .or_else(|| line.strip_prefix("content-length: "))
                })
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            while request.len() < header_end + content_length {
                let n = socket.read(&mut buffer).expect("read");
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..n]);
            }
            *captured_for_thread.lock().unwrap() = String::from_utf8_lossy(&request).to_string();
            let response = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: [DONE]\n\n";
            let _ = socket.write_all(response.as_bytes());
        });

        let endpoint = verified_fixture_endpoint(&format!("http://{address}"));
        let registry = Arc::new(GenerationRegistry::default());
        let GenerationStart {
            request_id,
            cancellation,
            guard,
        } = registry.begin(conversation.id).expect("generation");
        let context = GenerationContext {
            request_id,
            database,
            endpoint,
            model_id: "gemma-4-e2b-it-generic-gpu:1".to_string(),
            conversation,
            user_message,
            assistant_message,
            params: None,
            supports_reasoning: false,
            timeouts: StreamTimeouts::from_seconds(20, 20),
            cancellation,
            _guard: guard,
        };
        let collector = Collector::default();

        run_generation(context, collector.clone()).await;
        server.join().expect("server thread");

        let request = captured.lock().unwrap();
        assert!(
            request.contains("input_image"),
            "the request payload must carry the input_image content part"
        );
        assert!(
            request.contains("data:image/png;base64,iVBORw0KGgo="),
            "the data URL survives into the payload"
        );
    }

    fn verified_fixture_endpoint(
        endpoint: &str,
    ) -> super::super::local_inference::VerifiedLocalEndpoint {
        super::super::local_inference::VerifiedLocalEndpoint::new(
            endpoint,
            std::time::Duration::from_secs(5),
        )
        .expect("fixture endpoint")
    }

    fn sample_message(content: &str, attachments: Vec<Attachment>) -> ChatMessage {
        ChatMessage {
            id: 1,
            conversation_id: 1,
            seq: 1,
            role: MessageRole::User,
            content: content.to_string(),
            reasoning: String::new(),
            model_alias: Some("qwen3.5-0.8b".to_string()),
            params: None,
            usage: None,
            error: None,
            status: MessageStatus::Complete,
            attachments,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn message_content_is_text_without_images_and_parts_with_them() {
        let text = sample_message("hello", Vec::new());
        let database = Database::open_in_memory().expect("database");

        assert_eq!(
            message_content(&database, &text).expect("text"),
            json!("hello")
        );
        let conversation = database
            .create_conversation("fixture-model")
            .expect("conversation");
        let message = database
            .append_message(
                conversation.id,
                MessageRole::User,
                "describe",
                "",
                None,
                MessageStatus::Complete,
            )
            .expect("message");
        let store = super::super::attachments::AttachmentStore::new();
        let staged = store
            .store("data:image/png;base64,iVBORw0KGgo=")
            .expect("image");
        let staged = store.take(&[staged.token]).expect("staged copy").remove(0);
        database
            .add_attachment(message.id, "image", &staged)
            .expect("persist copy");
        let message = database.message(message.id).expect("persisted message");
        let parts = message_content(&database, &message).expect("content");
        let array = parts.as_array().expect("parts");

        assert_eq!(array[0], json!({"type": "text", "text": "describe"}));
        assert_eq!(
            array[1]["image_url"]["url"],
            "data:image/png;base64,iVBORw0KGgo="
        );
    }

}
