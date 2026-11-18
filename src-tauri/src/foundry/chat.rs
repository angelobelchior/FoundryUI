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
        let running = lock(&self.running);
        if running
            .values()
            .any(|generation| ids.contains(&generation.conversation_id))
        {
            return None;
        }
        let result = operation();
        drop(running);
        Some(result)
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
    let messages = match messages {
        Ok(messages) => messages,
        Err(error) => return cancelled_or_failed(&mut cancellation, error, &buffer, usage),
    };

    let mut payload = json!({
        "model": context.model_id,
        "messages": messages,
        "stream": true,
        "stream_options": { "include_usage": true },
    });

    if let Some(params) = &context.params {
        params.apply(&mut payload);
    }

    let request = context.endpoint.post_json_stream(
        "/v1/chat/completions",
        &payload,
        context.timeouts.first_token,
    );

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

            match parse_frame(&line) {
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
        match parse_frame(&pending) {
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
    fn parses_content_reasoning_usage_and_done_frames() {
        let content =
            parse_frame(r#"data: {"choices":[{"delta":{"content":"hi"}}]}"#).expect("frame");
        assert_eq!(
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
            presence_penalty: None,
            frequency_penalty: None,
            seed: Some(42),
            stop: Some(vec!["END".to_string()]),
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
    async fn a_session_change_blocks_chat_history_before_the_request() {
        use std::sync::atomic::{AtomicU64, Ordering};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("listener");
        listener.set_nonblocking(true).expect("nonblocking");
        let revision = Arc::new(AtomicU64::new(1));
        let endpoint = verified_fixture_endpoint(&format!(
            "http://{}",
            listener.local_addr().expect("address")
        ))
        .bind_session(revision.clone(), 1);
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let (context, collector) = test_context(database.clone(), endpoint, false);
        let message_id = context.assistant_message.id;
        revision.store(2, Ordering::SeqCst);

        run_generation(context, collector.clone()).await;

        assert_eq!(
            database.message(message_id).expect("message").status,
            MessageStatus::Error
        );
        assert!(collector.events.lock().unwrap().iter().any(|event| matches!(event, StreamEvent::Error { error, .. } if error.code() == "CommandFailed")));
        assert_eq!(
            listener.accept().expect_err("no request").kind(),
            std::io::ErrorKind::WouldBlock
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
    #[ignore = "requires an installed foundry CLI, a running daemon and the loaded qwen3-0.6b model"]
    async fn real_completion_streams_and_persists() {
        use super::super::backend::FoundryBackend;
        use super::super::cli::CliBackend;

        let cli = CliBackend::detect().expect("foundry CLI");
        let status = cli.server_status().await.expect("server status");
        let endpoint = verified_live_endpoint(&cli, &status);
        let loaded = cli.list_loaded_models().await.expect("loaded models");
        let model = loaded
            .iter()
            .find(|model| model.alias == "qwen3-0.6b")
            .expect("qwen3-0.6b is loaded");
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let (mut context, collector) = test_context(database.clone(), endpoint, false);
        let message_id = context.assistant_message.id;

        context.model_id = model.id.clone();
        run_generation(context, collector.clone()).await;

        let events = collector.events.lock().unwrap();
        let deltas = events
            .iter()
            .filter(|event| matches!(event, StreamEvent::Delta { .. }))
            .count();
        let done = events.iter().find_map(|event| match event {
            StreamEvent::Done {
                message, cancelled, ..
            } => Some((message, *cancelled)),
            _ => None,
        });

        assert!(deltas > 1, "expected token deltas");
        let (done, cancelled) = done.expect("done event");
        assert!(!cancelled);
        assert_eq!(done.status, MessageStatus::Complete);
        assert!(!done.content.is_empty() || !done.reasoning.is_empty());

        let stored = database.message(message_id).expect("stored message");
        assert_eq!(stored.status, MessageStatus::Complete);
        assert_eq!(stored.content, done.content);
        assert_eq!(stored.reasoning, done.reasoning);
        assert!(stored.usage.is_some(), "usage is reported");
    }

    #[tokio::test]
    #[ignore = "requires an installed foundry CLI, a running daemon and the loaded gpt-oss-20b model"]
    async fn gpt_oss_harmony_output_is_split_into_reasoning_and_content() {
        use super::super::backend::FoundryBackend;
        use super::super::cli::CliBackend;

        let cli = CliBackend::detect().expect("foundry CLI");
        let status = cli.server_status().await.expect("server status");
        let endpoint = verified_live_endpoint(&cli, &status);
        let loaded = cli.list_loaded_models().await.expect("loaded models");
        let model = loaded
            .iter()
            .find(|model| model.alias == "gpt-oss-20b")
            .expect("gpt-oss-20b is loaded");
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let (mut context, collector) = test_context(database.clone(), endpoint, false);
        let message_id = context.assistant_message.id;

        context.model_id = model.id.clone();
        run_generation(context, collector.clone()).await;

        let events = collector.events.lock().unwrap();
        let done = events.iter().find_map(|event| match event {
            StreamEvent::Done { message, .. } => Some(message),
            _ => None,
        });
        let done = done.expect("done event");

        assert!(
            !done.content.contains("<|"),
            "content keeps harmony tokens: {}",
            done.content
        );
        assert!(
            !done.reasoning.contains("<|"),
            "reasoning keeps harmony tokens: {}",
            done.reasoning
        );
        assert!(!done.content.trim().is_empty(), "final answer is present");

        let stored = database.message(message_id).expect("stored message");
        assert_eq!(stored.content, done.content);
        assert_eq!(stored.reasoning, done.reasoning);
    }

    #[tokio::test]
    #[ignore = "requires an installed foundry CLI, a running daemon and the loaded qwen3-0.6b model"]
    async fn cancelling_keeps_the_partial_reply() {
        use super::super::backend::FoundryBackend;
        use super::super::cli::CliBackend;

        let cli = CliBackend::detect().expect("foundry CLI");
        let status = cli.server_status().await.expect("server status");
        let endpoint = verified_live_endpoint(&cli, &status);
        let loaded = cli.list_loaded_models().await.expect("loaded models");
        let model = loaded
            .iter()
            .find(|model| model.alias == "qwen3-0.6b")
            .expect("qwen3-0.6b is loaded");
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let (mut context, collector) = test_context(database.clone(), endpoint, true);
        let message_id = context.assistant_message.id;

        context.model_id = model.id.clone();
        run_generation(context, collector.clone()).await;

        let events = collector.events.lock().unwrap();
        let done = events.iter().find_map(|event| match event {
            StreamEvent::Done {
                message, cancelled, ..
            } => Some((message, *cancelled)),
            _ => None,
        });

        let (done, cancelled) = done.expect("done event");
        assert!(cancelled);
        assert_eq!(done.status, MessageStatus::Cancelled);
        assert!(!done.content.is_empty() || !done.reasoning.is_empty());

        let stored = database.message(message_id).expect("stored message");
        assert_eq!(stored.status, MessageStatus::Cancelled);
        assert_eq!(stored.content, done.content);
    }

    fn verified_fixture_endpoint(
        endpoint: &str,
    ) -> super::super::local_inference::VerifiedLocalEndpoint {
        use super::super::local_inference::{
            RuntimeBinding, RuntimeObservation, VerifiedLocalEndpoint,
        };
        let binding = RuntimeBinding::default();
        binding.replace(Some(
            RuntimeObservation::new(
                endpoint,
                std::process::id(),
                std::env::current_exe().expect("test executable"),
                1,
            )
            .expect("fixture observation"),
        ));
        VerifiedLocalEndpoint::verify(&binding).expect("native fixture proof")
    }

    fn verified_live_endpoint(
        cli: &super::super::cli::CliBackend,
        status: &super::super::json::ServerStatus,
    ) -> super::super::local_inference::VerifiedLocalEndpoint {
        super::super::local_inference::verify_foundry_endpoint(
            std::path::Path::new(&cli.info().path),
            status.pid.expect("runtime PID"),
            status.endpoint().expect("running server"),
            std::time::Duration::from_secs(5),
        )
        .expect("verified Foundry runtime")
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

    #[tokio::test]
    #[ignore = "requires foundry CLI, a running daemon and the loaded qwen3.5-0.8b model"]
    async fn a_png_attachment_reaches_qwen35_as_a_data_url() {
        use base64::Engine;
        use base64::engine::general_purpose::STANDARD;

        use super::super::attachments::StagedAttachment;
        use super::super::attachments::StagedFile;
        use super::super::backend::FoundryBackend;
        use super::super::cli::CliBackend;

        let cli = CliBackend::detect().expect("foundry CLI");
        let status = cli.server_status().await.expect("server status");
        let endpoint = verified_live_endpoint(&cli, &status);
        let loaded = cli.list_loaded_models().await.expect("loaded models");
        let model = loaded
            .iter()
            .find(|model| model.alias == "qwen3.5-0.8b")
            .expect("qwen3.5-0.8b is loaded");
        let database = Arc::new(Database::open_in_memory().expect("database"));
        let conversation = database
            .create_conversation("qwen3.5-0.8b")
            .expect("conversation");
        let user_message = database
            .append_message(
                conversation.id,
                MessageRole::User,
                "Describe the color of this image in one sentence.",
                "",
                Some("qwen3.5-0.8b"),
                MessageStatus::Complete,
            )
            .expect("user message");
        let pixel = STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8DwHwAFAAH/q842iQAAAABJRU5ErkJggg==")
            .expect("png");
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("pixel.png");

        std::fs::write(&path, &pixel).expect("write");

        database
            .add_attachment(
                user_message.id,
                "image",
                &StagedFile {
                    meta: StagedAttachment {
                        token: "test".to_string(),
                        mime: "image/png".to_string(),
                        bytes: pixel.len() as u64,
                        width: Some(1),
                        height: Some(1),
                    },
                    data: pixel,
                },
            )
            .expect("attachment");

        let assistant_message = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "",
                "",
                Some("qwen3.5-0.8b"),
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
        let user_message = database
            .message(user_message.id)
            .expect("stored user message");

        assert_eq!(user_message.attachments.len(), 1);
        assert!(
            message_content(&database, &user_message)
                .expect("content")
                .is_array()
        );

        let context = GenerationContext {
            request_id,
            database,
            endpoint,
            model_id: model.id.clone(),
            conversation,
            user_message,
            assistant_message,
            params: Some(ChatParams {
                max_tokens: Some(64),
                ..ChatParams::default()
            }),
            timeouts: StreamTimeouts::from_seconds(60, 60),
            cancellation,
            _guard: guard,
        };

        run_generation(context, collector.clone()).await;

        let events = collector.events.lock().unwrap();
        let done = events.iter().find_map(|event| match event {
            StreamEvent::Done { message, .. } => Some(message),
            _ => None,
        });
        let done = done.expect("done event");

        assert!(
            !done.content.trim().is_empty(),
            "the model replied to the image"
        );
    }
}
