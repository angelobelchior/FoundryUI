use std::collections::HashMap;
use std::future::Future;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Map, Value, json};

use super::context_budget::{PlanRequest, plan};
use super::parser::ToolCall;
use super::schema::{ToolAvailability, ToolRegistry, ToolSpec, ToolUnavailable};
use crate::foundry::db::MessageRole;
use crate::foundry::db::agent::authorization::{
    AuthorizationProposal, AuthorizationRequest, AuthorizationRevision, AuthorizationStatus,
    RequestDecision,
};
use crate::foundry::db::agent::{
    ActionOutcome, ActionRecord, ActionState, AgentEventState, AgentStore, AgentStoreError,
    RejectionState, RunRecord, RunState,
};
use crate::foundry::policy as p;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleLimits {
    pub max_steps: u32,
    pub max_calls_per_turn: u32,
    pub max_tool_calls: u32,
    pub max_duration: Duration,
    pub max_input_bytes: usize,
    pub max_output_bytes: usize,
    pub max_repetitions: u32,
}

impl Default for CycleLimits {
    fn default() -> Self {
        Self {
            max_steps: 8,
            max_calls_per_turn: 8,
            max_tool_calls: 32,
            max_duration: Duration::from_secs(300),
            max_input_bytes: 262_144,
            max_output_bytes: 65_536,
            max_repetitions: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleRequest {
    pub conversation_id: i64,
    pub request_key: String,
    pub messages: Vec<Value>,
    pub model_supports_tools: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatOnlyReason {
    ModelIncompatible,
    NoAvailableTools,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    StepLimit,
    CallLimit,
    DurationLimit,
    InputLimit,
    RepetitionLimit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CycleOutcome {
    ChatOnly {
        reason: ChatOnlyReason,
    },
    Completed {
        steps: u32,
        tool_calls: u32,
    },
    AwaitingAuthorization {
        request_id: i64,
        action_id: i64,
        steps: u32,
        tool_calls: u32,
    },
    AlreadyFinished {
        run_state: RunState,
    },
    Stopped {
        reason: StopReason,
        detail: String,
        incomplete: bool,
        steps: u32,
        tool_calls: u32,
    },
    Failed {
        code: String,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleFailure {
    pub code: &'static str,
    pub detail: String,
}

impl CycleFailure {
    fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self {
            code,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for CycleFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.detail)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum CycleEventKind {
    RunStarted,
    TurnStarted {
        step: u32,
    },
    ToolProposed {
        action_id: i64,
        call_id: String,
        tool: String,
    },
    AuthorizationPending {
        action_id: i64,
        request_id: i64,
    },
    ToolRejected {
        call_id: String,
        tool: String,
        code: String,
    },
    ToolCompleted {
        action_id: i64,
        call_id: String,
        outcome: String,
    },
    LimitReached {
        reason: StopReason,
        detail: String,
    },
    RunCompleted,
    RunFailed {
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CycleEvent {
    pub conversation_id: i64,
    pub run_id: i64,
    pub sequence: Option<i64>,
    pub kind: CycleEventKind,
}

pub trait CycleSink: Send + Sync {
    fn emit(&self, event: &CycleEvent) -> Result<(), String>;
}

impl CycleSink for () {
    fn emit(&self, _event: &CycleEvent) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelRequest {
    pub messages: Vec<Value>,
    pub tools: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelTurn {
    pub content: String,
    pub reasoning: String,
    pub calls: Vec<ToolCall>,
}

#[async_trait]
pub trait TurnProvider: Send {
    async fn next_turn(&mut self, request: ModelRequest) -> Result<ModelTurn, String>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolExecution {
    Succeeded { summary: Value },
    Failed { summary: Value },
}

pub struct ToolHost<'a, 'db> {
    pub store: &'a AgentStore<'db>,
    pub facts: &'a p::TrustedFacts,
}

impl ToolHost<'_, '_> {
    pub fn reads_current(&self) -> bool {
        self.store
            .authorization_revision(self.facts.action.key.context.conversation_id)
            .is_ok_and(|revision| {
                revision.global_read_allowed() && revision.key() == &self.facts.action.key.context
            })
    }
}

#[async_trait]
pub trait ToolExecutor: Send {
    async fn execute(
        &mut self,
        call: &ToolCall,
        spec: &ToolSpec,
        host: &ToolHost<'_, '_>,
    ) -> ToolExecution;
}

pub trait ToolAuthorizer: Send + Sync {
    fn facts(
        &self,
        store: &AgentStore<'_>,
        revision: &AuthorizationRevision,
        action: &ActionRecord,
        call: &ToolCall,
    ) -> Result<p::TrustedFacts, String>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalChoice {
    Approve { remember: bool },
    Deny,
    Suspend,
    Refresh,
}

#[async_trait]
pub trait ApprovalProvider: Send {
    fn choose(&mut self, request: &AuthorizationRequest) -> ApprovalChoice;

    async fn wait(
        &mut self,
        _store: &AgentStore<'_>,
        request: &AuthorizationRequest,
    ) -> Result<ApprovalChoice, String> {
        Ok(self.choose(request))
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SuspendApprovals;

impl ApprovalProvider for SuspendApprovals {
    fn choose(&mut self, _request: &AuthorizationRequest) -> ApprovalChoice {
        ApprovalChoice::Suspend
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UnimplementedTools;

impl ToolAuthorizer for UnimplementedTools {
    fn facts(
        &self,
        _store: &AgentStore<'_>,
        revision: &AuthorizationRevision,
        action: &ActionRecord,
        _call: &ToolCall,
    ) -> Result<p::TrustedFacts, String> {
        let policy_action = p::Action {
            key: p::ActionKey {
                context: revision.key().clone(),
                run_id: action.run_id,
                action_id: action.id,
            },
            operation: p::Operation::Unknown,
            arguments_fingerprint: action.arguments_fingerprint.clone(),
            resources: Vec::new(),
            origin: p::InstructionOrigin::Model,
        };
        Ok(p::TrustedFacts {
            current_key: policy_action.key.clone(),
            action: policy_action,
            mode: revision.mode(),
            mode_at_proposal: revision.mode(),
            project_root_id: revision.project_root_id().map(str::to_string),
            global_read_allowed: revision.global_read_allowed(),
            read_evidence: p::ReadEvidence::ProvenNoReads,
            capability: p::Capability::MissingImplementation,
            resources: Vec::new(),
            concrete_change_id: None,
            recovery: p::Recovery::NotGuaranteed,
            approvals: Vec::new(),
            denied_actions: Vec::new(),
            grants: Vec::new(),
        })
    }
}

pub struct CycleDeps<'a> {
    pub registry: &'a ToolRegistry,
    pub provider: &'a mut dyn TurnProvider,
    pub executor: &'a mut dyn ToolExecutor,
    pub authorizer: &'a dyn ToolAuthorizer,
    pub approvals: &'a mut dyn ApprovalProvider,
    pub limits: &'a CycleLimits,
    pub sink: &'a dyn CycleSink,
}

pub async fn run_cycle(
    store: &AgentStore<'_>,
    deps: CycleDeps<'_>,
    request: CycleRequest,
) -> CycleOutcome {
    if !request.model_supports_tools {
        return CycleOutcome::ChatOnly {
            reason: ChatOnlyReason::ModelIncompatible,
        };
    }
    let definitions = deps.registry.model_definitions();
    if definitions.is_empty() {
        return CycleOutcome::ChatOnly {
            reason: ChatOnlyReason::NoAvailableTools,
        };
    }
    let CycleDeps {
        registry,
        provider,
        executor,
        authorizer,
        approvals,
        limits,
        sink,
    } = deps;
    let run = match store.start_run(request.conversation_id, &request.request_key) {
        Ok(run) => run,
        Err(AgentStoreError::NotFound) => {
            return CycleOutcome::Failed {
                code: "context_unavailable".into(),
                detail: "the conversation context is unavailable".into(),
            };
        }
        Err(error) => {
            return CycleOutcome::Failed {
                code: "run_start_failed".into(),
                detail: format!("the run could not start: {error}"),
            };
        }
    };
    if run.state != RunState::Prepared {
        let pending = match store.list_requests(request.conversation_id) {
            Ok(requests) => requests.into_iter().find(|request| {
                request.run_id == run.id && request.status == AuthorizationStatus::Pending
            }),
            Err(error) => {
                return CycleOutcome::Failed {
                    code: "storage_failed".into(),
                    detail: format!("pending authorizations are unavailable: {error}"),
                };
            }
        };
        return match pending {
            Some(request) => CycleOutcome::AwaitingAuthorization {
                request_id: request.id,
                action_id: request.action_id,
                steps: 0,
                tool_calls: 0,
            },
            None => CycleOutcome::AlreadyFinished {
                run_state: run.state,
            },
        };
    }
    let driver = Driver {
        store,
        registry,
        executor,
        authorizer,
        approvals,
        limits,
        definitions,
        conversation_id: request.conversation_id,
        run: run.clone(),
        results: HashMap::new(),
        started: Instant::now(),
        sink,
    };
    let outcome = driver.drive(provider, request.messages).await;
    match outcome {
        Ok(outcome) => outcome,
        Err(_)
            if store
                .run(run.id)
                .is_ok_and(|run| run.state == RunState::Cancelled) =>
        {
            CycleOutcome::AlreadyFinished {
                run_state: RunState::Cancelled,
            }
        }
        Err(failure) => fail(store, sink, &run, failure),
    }
}

fn fail(
    store: &AgentStore<'_>,
    sink: &dyn CycleSink,
    run: &RunRecord,
    failure: CycleFailure,
) -> CycleOutcome {
    let mut detail = bound_text(&failure.detail, 1024);
    if let Err(error) = store.fail_run(run.id, Some(&detail)) {
        detail = bound_text(
            &format!("{detail}; the run could not be marked failed: {error}"),
            1024,
        );
    }
    let _ = emit_runtime(
        store,
        sink,
        run,
        CycleEventKind::RunFailed {
            detail: detail.clone(),
        },
    );
    CycleOutcome::Failed {
        code: failure.code.into(),
        detail,
    }
}

struct Driver<'a, 'db> {
    store: &'a AgentStore<'db>,
    registry: &'a ToolRegistry,
    executor: &'a mut dyn ToolExecutor,
    authorizer: &'a dyn ToolAuthorizer,
    approvals: &'a mut dyn ApprovalProvider,
    limits: &'a CycleLimits,
    definitions: Vec<Value>,
    conversation_id: i64,
    run: RunRecord,
    results: HashMap<String, Value>,
    started: Instant,
    sink: &'a dyn CycleSink,
}

impl Driver<'_, '_> {
    async fn drive(
        mut self,
        provider: &mut dyn TurnProvider,
        messages: Vec<Value>,
    ) -> Result<CycleOutcome, CycleFailure> {
        self.event(CycleEventKind::RunStarted)?;
        let started = self.started;
        let mut messages = messages;
        let mut request_bytes = estimate_bytes(&messages, &self.definitions);
        let mut steps = 0u32;
        let mut tool_calls = 0u32;
        let mut repetitions: HashMap<String, u32> = HashMap::new();

        loop {
            if started.elapsed() >= self.limits.max_duration {
                return self.stop(
                    StopReason::DurationLimit,
                    "the cycle duration was exceeded",
                    steps,
                    tool_calls,
                );
            }
            if steps >= self.limits.max_steps {
                return self.stop(
                    StopReason::StepLimit,
                    "the agent step limit was reached",
                    steps,
                    tool_calls,
                );
            }
            if request_bytes > self.limits.max_input_bytes {
                return self.stop(
                    StopReason::InputLimit,
                    "the model input exceeded its byte limit",
                    steps,
                    tool_calls,
                );
            }
            steps += 1;
            self.event(CycleEventKind::TurnStarted { step: steps })?;
            let plan = plan(PlanRequest {
                messages: messages.clone(),
                capacity_tokens: None,
                max_input_bytes: Some(self.limits.max_input_bytes),
                requested_output_tokens: None,
            });
            if let Ok(manifest) = serde_json::to_string(&plan.manifest) {
                let _ = self.store.record_context_budget(
                    self.conversation_id,
                    self.run.context_revision,
                    Some(self.run.id),
                    &format!("context_budget.{}.{}", self.run.id, steps),
                    &manifest,
                );
            }
            if plan.manifest.compacted {
                let _ = self.store.record_event(
                    self.conversation_id,
                    self.run.id,
                    None,
                    AgentEventState::ContextCompacted,
                    Some(&format!(
                        "included={} summarized={} omitted={}",
                        plan.manifest.included_messages,
                        plan.manifest.summarized_messages,
                        plan.manifest.omitted_messages
                    )),
                );
            }
            let turn = match within(
                started,
                self.limits.max_duration,
                provider.next_turn(ModelRequest {
                    messages: plan.messages,
                    tools: self.definitions.clone(),
                }),
            )
            .await
            {
                Within::Expired => {
                    return self.stop(
                        StopReason::DurationLimit,
                        "the cycle duration was exceeded while waiting for the model",
                        steps,
                        tool_calls,
                    );
                }
                Within::Value(Ok(turn)) => turn,
                Within::Value(Err(detail)) => {
                    return Err(CycleFailure::new("provider_failed", detail));
                }
            };
            let content = bound_text(&turn.content, self.limits.max_output_bytes);
            let reasoning = bound_text(&turn.reasoning, self.limits.max_output_bytes);
            if !content.is_empty() || !reasoning.is_empty() {
                self.store
                    .record_text(self.run.id, MessageRole::Assistant, &content, &reasoning)
                    .map_err(storage)?;
            }
            if turn.calls.is_empty() {
                self.store.finish_run(self.run.id).map_err(storage)?;
                self.event(CycleEventKind::RunCompleted)?;
                return Ok(CycleOutcome::Completed { steps, tool_calls });
            }
            if turn.calls.len() as u32 > self.limits.max_calls_per_turn {
                return self.stop(
                    StopReason::CallLimit,
                    "the model proposed more calls than the turn allows",
                    steps,
                    tool_calls,
                );
            }
            let assistant = assistant_message(&content, &turn.calls);
            request_bytes = request_bytes.saturating_add(json_len(&assistant));
            messages.push(assistant);
            for call in &turn.calls {
                if started.elapsed() >= self.limits.max_duration {
                    return self.stop(
                        StopReason::DurationLimit,
                        "the cycle duration was exceeded",
                        steps,
                        tool_calls,
                    );
                }
                if tool_calls >= self.limits.max_tool_calls {
                    return self.stop(
                        StopReason::CallLimit,
                        "the tool call limit was reached",
                        steps,
                        tool_calls,
                    );
                }
                if self.counts_toward_repetition(call) {
                    let key = self.repetition_key(call)?;
                    let count = repetitions.entry(key).or_insert(0);
                    *count += 1;
                    if *count > self.limits.max_repetitions {
                        return self.stop(
                            StopReason::RepetitionLimit,
                            "the same call repeated beyond its limit",
                            steps,
                            tool_calls,
                        );
                    }
                }
                tool_calls += 1;
                let disposition =
                    match within(started, self.limits.max_duration, self.process(call)).await {
                        Within::Value(result) => result?,
                        Within::Expired => Disposition::Expired,
                    };
                match disposition {
                    Disposition::Continue(summary) => {
                        let message = tool_message(&call.id, &summary);
                        request_bytes = request_bytes.saturating_add(json_len(&message));
                        messages.push(message);
                    }
                    Disposition::Expired => {
                        return self.stop(
                            StopReason::DurationLimit,
                            "the cycle duration was exceeded while processing a tool call",
                            steps,
                            tool_calls,
                        );
                    }
                    Disposition::Suspend {
                        request_id,
                        action_id,
                    } => {
                        return Ok(CycleOutcome::AwaitingAuthorization {
                            request_id,
                            action_id,
                            steps,
                            tool_calls,
                        });
                    }
                }
            }
        }
    }

    fn stop(
        &self,
        reason: StopReason,
        detail: &str,
        steps: u32,
        tool_calls: u32,
    ) -> Result<CycleOutcome, CycleFailure> {
        self.event(CycleEventKind::LimitReached {
            reason,
            detail: detail.to_string(),
        })?;
        self.store
            .fail_run(self.run.id, Some(detail))
            .map_err(storage)?;
        Ok(CycleOutcome::Stopped {
            reason,
            detail: detail.to_string(),
            incomplete: true,
            steps,
            tool_calls,
        })
    }

    async fn process(&mut self, call: &ToolCall) -> Result<Disposition, CycleFailure> {
        let Some(registered) = self.registry.get(&call.name) else {
            let summary =
                self.reject(call, "unknown_tool", "the model proposed an unknown tool")?;
            return Ok(Disposition::Continue(summary));
        };
        if let ToolAvailability::Unavailable { reason } = registered.availability {
            let code = match reason {
                ToolUnavailable::NotImplemented => "tool_unavailable",
                ToolUnavailable::CapabilityBlocked => "capability_blocked",
                ToolUnavailable::Disabled => "tool_disabled",
            };
            let summary = self.reject(call, code, "the tool is not available")?;
            return Ok(Disposition::Continue(summary));
        }
        if let Err(error) = registered.spec.validate_arguments(&call.arguments) {
            let summary = self.reject(
                call,
                "invalid_arguments",
                &format!("{}: {}", error.path, error.reason),
            )?;
            return Ok(Disposition::Continue(summary));
        }
        let record = self
            .store
            .propose_call(self.run.id, &call.id, &call.name, &call.arguments)
            .map_err(storage)?;
        if record.state != ActionState::Proposed {
            return self.resolve_repeat(&record, call);
        }
        self.event(CycleEventKind::ToolProposed {
            action_id: record.id,
            call_id: call.id.clone(),
            tool: call.name.clone(),
        })?;
        let revision = self
            .store
            .authorization_revision(self.conversation_id)
            .map_err(storage)?;
        let trusted = match self.authorizer.facts(self.store, &revision, &record, call) {
            Ok(trusted) => trusted,
            Err(detail) => {
                return self.unexecuted(
                    &record,
                    call,
                    "authorization_unavailable",
                    &detail,
                    RejectionState::Failed,
                );
            }
        };
        let proposal = match self.store.prepare_request(&record, &trusted) {
            Ok(proposal) => proposal,
            Err(error) => {
                return self.unexecuted(
                    &record,
                    call,
                    "authorization_failed",
                    &error.to_string(),
                    RejectionState::Failed,
                );
            }
        };
        match proposal.status() {
            AuthorizationStatus::Denied => self.deny(&record, call, proposal.denial()),
            AuthorizationStatus::Ready => {
                self.dispatch(&proposal, &record, call, Some(&trusted))
                    .await
            }
            AuthorizationStatus::Pending => self.await_approval(&proposal, &record, call).await,
            AuthorizationStatus::Claimed | AuthorizationStatus::Invalidated => {
                if let Some(summary) = self.stored_result(record.id)? {
                    return Ok(Disposition::Continue(summary));
                }
                self.unexecuted(
                    &record,
                    call,
                    "authorization_changed",
                    "the authorization is no longer current",
                    RejectionState::Failed,
                )
            }
        }
    }

    fn resolve_repeat(
        &mut self,
        record: &ActionRecord,
        call: &ToolCall,
    ) -> Result<Disposition, CycleFailure> {
        if let Some(summary) = self.results.get(&call.id) {
            return Ok(Disposition::Continue(summary.clone()));
        }
        if let Some(summary) = self.stored_result(record.id)? {
            return Ok(Disposition::Continue(summary));
        }
        let code = match record.state {
            ActionState::Started | ActionState::Interrupted => "call_interrupted",
            _ => "duplicate_call",
        };
        let detail = match code {
            "call_interrupted" => "this call was interrupted before a result was confirmed",
            _ => "this call was already resolved",
        };
        self.unexecuted(record, call, code, detail, RejectionState::Failed)
    }

    fn stored_result(&mut self, action_id: i64) -> Result<Option<Value>, CycleFailure> {
        let Some((_, summary)) = self.store.tool_result_for(action_id).map_err(storage)? else {
            return Ok(None);
        };
        Ok(Some(summary))
    }

    fn unexecuted(
        &mut self,
        record: &ActionRecord,
        call: &ToolCall,
        code: &str,
        detail: &str,
        state: RejectionState,
    ) -> Result<Disposition, CycleFailure> {
        let summary = rejection(code, detail);
        self.store
            .record_unexecuted_call(record, &summary, state)
            .map_err(storage)?;
        self.results.insert(call.id.clone(), summary.clone());
        self.event(CycleEventKind::ToolRejected {
            call_id: call.id.clone(),
            tool: call.name.clone(),
            code: code.to_string(),
        })?;
        Ok(Disposition::Continue(summary))
    }

    async fn await_approval(
        &mut self,
        proposal: &AuthorizationProposal,
        record: &ActionRecord,
        call: &ToolCall,
    ) -> Result<Disposition, CycleFailure> {
        let mut announced = Vec::new();
        loop {
            let pending = self
                .store
                .list_requests(self.conversation_id)
                .map_err(storage)?
                .into_iter()
                .filter(|request| {
                    request.action_id == record.id && request.status == AuthorizationStatus::Pending
                })
                .collect::<Vec<_>>();
            if pending.is_empty() {
                break;
            }
            let mut suspended = None;
            for request in &pending {
                if !announced.contains(&request.id) {
                    self.event(CycleEventKind::AuthorizationPending {
                        action_id: record.id,
                        request_id: request.id,
                    })?;
                    announced.push(request.id);
                }
                match self
                    .approvals
                    .wait(self.store, request)
                    .await
                    .map_err(|detail| CycleFailure::new("authorization_changed", detail))?
                {
                    ApprovalChoice::Refresh => {}
                    ApprovalChoice::Suspend => {
                        suspended = Some(request.id);
                        break;
                    }
                    ApprovalChoice::Deny => {
                        self.store
                            .decide_request(self.conversation_id, request.id, RequestDecision::Deny)
                            .map_err(storage)?;
                    }
                    ApprovalChoice::Approve { remember } => {
                        self.store
                            .decide_request(
                                self.conversation_id,
                                request.id,
                                RequestDecision::Approve { remember },
                            )
                            .map_err(storage)?;
                    }
                }
            }
            if let Some(request_id) = suspended {
                return Ok(Disposition::Suspend {
                    request_id,
                    action_id: record.id,
                });
            }
        }
        let action = self.store.action(record.id).map_err(storage)?;
        if action.state == ActionState::Denied {
            return self.deny(&action, call, Some("UserDenied"));
        }
        if action.state != ActionState::Proposed {
            if let Some(summary) = self.stored_result(record.id)? {
                return Ok(Disposition::Continue(summary));
            }
            return self.unexecuted(
                &action,
                call,
                "authorization_changed",
                "the authorization is no longer current",
                RejectionState::Failed,
            );
        }
        self.dispatch(proposal, record, call, None).await
    }

    async fn dispatch(
        &mut self,
        proposal: &AuthorizationProposal,
        record: &ActionRecord,
        call: &ToolCall,
        trusted: Option<&p::TrustedFacts>,
    ) -> Result<Disposition, CycleFailure> {
        let fresh;
        let facts = match trusted {
            Some(facts) => facts,
            None => {
                let revision = self
                    .store
                    .authorization_revision(self.conversation_id)
                    .map_err(storage)?;
                fresh = match self.authorizer.facts(self.store, &revision, record, call) {
                    Ok(facts) => facts,
                    Err(detail) => {
                        return self.unexecuted(
                            record,
                            call,
                            "authorization_unavailable",
                            &detail,
                            RejectionState::Failed,
                        );
                    }
                };
                &fresh
            }
        };
        let claim = match self
            .store
            .claim_authorized_call(proposal, record, &call.arguments, facts)
        {
            Ok(claim) => claim,
            Err(error) => {
                if let Some(summary) = self.stored_result(record.id)? {
                    return Ok(Disposition::Continue(summary));
                }
                return self.unexecuted(
                    record,
                    call,
                    "authorization_changed",
                    &format!("the action could not be claimed: {error}"),
                    RejectionState::Failed,
                );
            }
        };
        let spec = self
            .registry
            .get(&call.name)
            .map(|tool| tool.spec.clone())
            .ok_or_else(|| {
                CycleFailure::new("tool_unavailable", "the tool disappeared before execution")
            })?;
        let execution = match within(
            self.started,
            self.limits.max_duration,
            self.executor.execute(
                call,
                &spec,
                &ToolHost {
                    store: self.store,
                    facts,
                },
            ),
        )
        .await
        {
            Within::Expired => return Ok(Disposition::Expired),
            Within::Value(execution) => execution,
        };
        let (outcome, summary) = match execution {
            ToolExecution::Succeeded { summary } => (ActionOutcome::Succeeded, success(summary)),
            ToolExecution::Failed { summary } => (ActionOutcome::Failed, failure(summary)),
        };
        let summary = bounded(summary, self.limits.max_output_bytes);
        self.store
            .complete_call(claim.action_claim(), outcome, &summary)
            .map_err(storage)?;
        self.results.insert(call.id.clone(), summary.clone());
        self.event(CycleEventKind::ToolCompleted {
            action_id: record.id,
            call_id: call.id.clone(),
            outcome: match outcome {
                ActionOutcome::Succeeded => "succeeded".into(),
                ActionOutcome::Failed => "failed".into(),
                ActionOutcome::Cancelled => "cancelled".into(),
            },
        })?;
        Ok(Disposition::Continue(summary))
    }

    fn deny(
        &mut self,
        record: &ActionRecord,
        call: &ToolCall,
        denial: Option<&str>,
    ) -> Result<Disposition, CycleFailure> {
        if call.name == "http_request"
            && let Some(destination) = call.arguments.get("url").and_then(Value::as_str)
        {
            let revision = self
                .store
                .authorization_revision(self.conversation_id)
                .map_err(storage)?;
            self.store
                .record_network_access(
                    &p::ActionKey {
                        context: revision.key().clone(),
                        run_id: self.run.id,
                        action_id: record.id,
                    },
                    destination,
                    crate::foundry::db::agent::NetworkEventState::Blocked,
                    None,
                )
                .map_err(storage)?;
        }
        let summary = denial_summary(denial);
        self.store
            .record_unexecuted_call(record, &summary, RejectionState::Denied)
            .map_err(storage)?;
        self.results.insert(record.call_id.clone(), summary.clone());
        self.event(CycleEventKind::ToolRejected {
            call_id: record.call_id.clone(),
            tool: record.tool.clone(),
            code: "denied".into(),
        })?;
        Ok(Disposition::Continue(summary))
    }

    fn reject(&mut self, call: &ToolCall, code: &str, detail: &str) -> Result<Value, CycleFailure> {
        let summary = rejection(code, detail);
        self.store
            .record_unbound_tool_result(
                self.run.id,
                &call.id,
                &call.name,
                &call.arguments,
                ActionOutcome::Failed,
                &summary,
            )
            .map_err(storage)?;
        self.results.insert(call.id.clone(), summary.clone());
        self.event(CycleEventKind::ToolRejected {
            call_id: call.id.clone(),
            tool: call.name.clone(),
            code: code.to_string(),
        })?;
        Ok(summary)
    }

    fn counts_toward_repetition(&self, call: &ToolCall) -> bool {
        self.registry.get(&call.name).is_some_and(|tool| {
            tool.availability == ToolAvailability::Available
                && tool.spec.validate_arguments(&call.arguments).is_ok()
        })
    }

    fn repetition_key(&self, call: &ToolCall) -> Result<String, CycleFailure> {
        let fingerprint = self
            .store
            .arguments_fingerprint(&call.arguments)
            .map_err(storage)?;
        Ok(format!("{}\u{0}{fingerprint}", call.name))
    }

    fn event(&self, kind: CycleEventKind) -> Result<(), CycleFailure> {
        emit_runtime(self.store, self.sink, &self.run, kind)
    }
}

enum Disposition {
    Continue(Value),
    Expired,
    Suspend { request_id: i64, action_id: i64 },
}

enum Within<T> {
    Value(T),
    Expired,
}

async fn within<T>(
    started: Instant,
    limit: Duration,
    future: impl Future<Output = T>,
) -> Within<T> {
    let remaining = limit.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Within::Expired;
    }
    match tokio::time::timeout(remaining, future).await {
        Ok(value) => Within::Value(value),
        Err(_) => Within::Expired,
    }
}

fn emit_runtime(
    store: &AgentStore<'_>,
    sink: &dyn CycleSink,
    run: &RunRecord,
    kind: CycleEventKind,
) -> Result<(), CycleFailure> {
    let persisted = match &kind {
        CycleEventKind::TurnStarted { .. } => Some((AgentEventState::TurnStarted, None)),
        CycleEventKind::AuthorizationPending { .. } => {
            Some((AgentEventState::AuthorizationPending, None))
        }
        CycleEventKind::ToolRejected { code, .. } => Some((
            AgentEventState::ToolRejected,
            Some(format!("rejected:{code}")),
        )),
        CycleEventKind::LimitReached { reason, detail } => Some((
            AgentEventState::LimitReached,
            Some(format!("{reason:?}: {detail}")),
        )),
        CycleEventKind::RunStarted
        | CycleEventKind::ToolProposed { .. }
        | CycleEventKind::ToolCompleted { .. }
        | CycleEventKind::RunCompleted
        | CycleEventKind::RunFailed { .. } => None,
    };
    let action_id = match &kind {
        CycleEventKind::ToolProposed { action_id, .. }
        | CycleEventKind::AuthorizationPending { action_id, .. }
        | CycleEventKind::ToolCompleted { action_id, .. } => Some(*action_id),
        _ => None,
    };
    let sequence = match persisted {
        Some((state, detail)) => {
            store
                .record_event(
                    run.conversation_id,
                    run.id,
                    action_id,
                    state,
                    detail.as_deref(),
                )
                .map_err(storage)?
                .sequence
        }
        None => store.last_sequence(run.id).map_err(storage)?,
    };
    sink.emit(&CycleEvent {
        conversation_id: run.conversation_id,
        run_id: run.id,
        sequence: Some(sequence),
        kind,
    })
    .map_err(|error| CycleFailure::new("sink_failed", error))
}

fn assistant_message(content: &str, calls: &[ToolCall]) -> Value {
    let mut message = json!({
        "role": "assistant",
        "content": content,
    });
    if !calls.is_empty() {
        let calls = calls
            .iter()
            .map(|call| {
                json!({
                    "id": call.id,
                    "type": "function",
                    "function": {
                        "name": call.name,
                        "arguments": serde_json::to_string(&call.arguments).unwrap_or_else(|_| "{}".into()),
                    },
                })
            })
            .collect::<Vec<_>>();
        message["tool_calls"] = Value::Array(calls);
    }
    message
}

fn tool_message(call_id: &str, summary: &Value) -> Value {
    json!({
        "role": "tool",
        "tool_call_id": call_id,
        "content": serde_json::to_string(summary).unwrap_or_else(|_| "{\"status\":\"failed\"}".into()),
    })
}

fn estimate_bytes(messages: &[Value], tools: &[Value]) -> usize {
    let messages = json_len(&Value::Array(messages.to_vec()));
    let tools = json_len(&Value::Array(tools.to_vec()));
    messages.saturating_add(tools)
}

fn json_len(value: &Value) -> usize {
    serde_json::to_vec(value)
        .map(|encoded| encoded.len())
        .unwrap_or(usize::MAX)
}

fn success(summary: Value) -> Value {
    match summary {
        Value::Object(mut object) => {
            object.insert("status".into(), Value::String("succeeded".into()));
            Value::Object(object)
        }
        other => json!({"status": "succeeded", "output": other}),
    }
}

fn failure(summary: Value) -> Value {
    match summary {
        Value::Object(mut object) => {
            object.insert("status".into(), Value::String("failed".into()));
            Value::Object(object)
        }
        other => json!({"status": "failed", "output": other}),
    }
}

fn rejection(code: &str, detail: &str) -> Value {
    json!({
        "status": "rejected",
        "code": code,
        "detail": detail.chars().take(512).collect::<String>(),
    })
}

fn denial_summary(denial: Option<&str>) -> Value {
    json!({
        "status": "denied",
        "reason": denial.unwrap_or("policy_denied").chars().take(256).collect::<String>(),
    })
}

fn bound_text(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    let mut bounded = text[..end].to_string();
    bounded.push_str("\n[truncated]");
    bounded
}

fn bounded(summary: Value, max_bytes: usize) -> Value {
    let limit = max_bytes.max(256);
    let encoded = json_len(&summary);
    if encoded <= limit {
        return summary;
    }
    let full = serde_json::to_string(&summary).unwrap_or_default();
    let mut object = match summary {
        Value::Object(object) => object,
        _ => Map::new(),
    };
    let mut envelope = Map::new();
    for key in ["status", "code", "outcome"] {
        if let Some(value) = object.remove(key)
            && value.is_string()
        {
            envelope.insert(key.into(), value);
        }
    }
    envelope.insert("truncated".into(), Value::Bool(true));
    envelope.insert("bytes".into(), Value::from(encoded as u64));
    let base = json_len(&Value::Object(envelope.clone()));
    let mut budget = limit.saturating_sub(base + 32);
    loop {
        let mut candidate = envelope.clone();
        let preview = full.chars().take(budget).collect::<String>();
        candidate.insert("preview".into(), Value::String(preview));
        let result = Value::Object(candidate);
        if json_len(&result) <= limit {
            return result;
        }
        if budget == 0 {
            return json!({"truncated": true, "bytes": encoded});
        }
        budget = budget.saturating_mul(3) / 4;
    }
}

fn storage(error: AgentStoreError) -> CycleFailure {
    CycleFailure::new("storage_failed", format!("agent storage failed: {error}"))
}
