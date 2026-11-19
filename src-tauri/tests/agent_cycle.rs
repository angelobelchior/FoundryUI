use std::collections::VecDeque;
use std::future::Future;
use std::time::Duration;

use async_trait::async_trait;
use foundryui_lib::foundry::agent_runtime::{
    ApprovalChoice, ApprovalProvider, ChatOnlyReason, CycleDeps, CycleEvent, CycleEventKind,
    CycleLimits, CycleOutcome, CycleRequest, CycleSink, ModelRequest, ModelTurn,
    PersistedApprovals, RegisteredTool, StopReason, ToolAuthorizer, ToolAvailability, ToolCall,
    ToolExecution, ToolExecutor, ToolRegistry, ToolSpec, ToolUnavailable, TurnProvider,
    run_agent_cycle, run_cycle,
};
use foundryui_lib::foundry::db::Database;
use foundryui_lib::foundry::db::agent::authorization::{
    AuthorizationRequest, AuthorizationRevision, AuthorizationStatus, RequestDecision,
};
use foundryui_lib::foundry::db::agent::{
    ActionOutcome, ActionRecord, ActionState, AgentMode, AgentStoreError, HistoryContent, RunState,
};
use foundryui_lib::foundry::policy as p;
use foundryui_lib::foundry::resources::RootHandle;
use serde_json::{Value, json};
use tempfile::TempDir;

struct Fixture {
    database: Database,
    _directory: TempDir,
    root: RootHandle,
    conversation: i64,
}

impl Fixture {
    fn project() -> Self {
        let database = Database::open_in_memory().expect("database");
        let directory = tempfile::tempdir().expect("directory");
        let root = RootHandle::enroll(directory.path()).expect("root");
        let store = database.agent_store();
        let project = store.create_project(&root, Some("Cycle")).expect("project");
        let conversation = store
            .create_project_conversation(Some(project.id), "fixture-model")
            .expect("conversation")
            .conversation
            .id;
        Self {
            database,
            _directory: directory,
            root,
            conversation,
        }
    }

    fn standalone() -> Self {
        let database = Database::open_in_memory().expect("database");
        let directory = tempfile::tempdir().expect("directory");
        let root = RootHandle::enroll(directory.path()).expect("root");
        let conversation = database
            .create_conversation("fixture-model")
            .expect("conversation")
            .id;
        let store = database.agent_store();
        store.context(conversation).expect("context");
        store
            .register_authorization_root(conversation, 1, &root)
            .expect("root");
        Self {
            database,
            _directory: directory,
            root,
            conversation,
        }
    }

    fn store(&self) -> foundryui_lib::foundry::db::agent::AgentStore<'_> {
        self.database.agent_store()
    }
}

struct ReadAuthorizer<'a> {
    root: &'a RootHandle,
}

impl ToolAuthorizer for ReadAuthorizer<'_> {
    fn facts(
        &self,
        store: &foundryui_lib::foundry::db::agent::AgentStore<'_>,
        revision: &AuthorizationRevision,
        action: &ActionRecord,
        call: &ToolCall,
    ) -> Result<p::TrustedFacts, String> {
        if revision.project_root_id().is_some() {
            store
                .register_action_root(action, self.root)
                .map_err(|error| error.to_string())?;
        }
        let resource = p::Resource {
            root_id: self
                .root
                .identity()
                .map_err(|error| error.to_string())?
                .storage_key(),
            object_id: format!(
                "object-{}",
                call.arguments["path"].as_str().unwrap_or("unknown")
            ),
            version: "v1".into(),
        };
        let policy_action = p::Action {
            key: p::ActionKey {
                context: revision.key().clone(),
                run_id: action.run_id,
                action_id: action.id,
            },
            operation: p::Operation::File(p::FileOperation::Read),
            arguments_fingerprint: action.arguments_fingerprint.clone(),
            resources: vec![resource.clone()],
            origin: p::InstructionOrigin::Model,
        };
        Ok(p::TrustedFacts {
            action: policy_action.clone(),
            current_key: policy_action.key.clone(),
            mode: revision.mode(),
            mode_at_proposal: revision.mode(),
            project_root_id: revision.project_root_id().map(str::to_string),
            global_read_allowed: revision.global_read_allowed(),
            read_evidence: p::ReadEvidence::ProvenReads {
                resources: vec![resource.clone()],
            },
            capability: p::Capability::Proven {
                effects: p::Effects::ReadOnly,
                opened_resources_validated: true,
                process_isolation: false,
                network: p::NetworkControl::Disabled,
            },
            resources: vec![p::ResolvedResource {
                resource,
                scope: if revision.project_root_id().is_some() {
                    p::ResourceScope::Project
                } else {
                    p::ResourceScope::Standalone
                },
            }],
            concrete_change_id: None,
            recovery: p::Recovery::NotGuaranteed,
            approvals: Vec::new(),
            denied_actions: Vec::new(),
            grants: Vec::new(),
        })
    }
}

struct UnavailableAuthorizer;

impl ToolAuthorizer for UnavailableAuthorizer {
    fn facts(
        &self,
        _store: &foundryui_lib::foundry::db::agent::AgentStore<'_>,
        _revision: &AuthorizationRevision,
        _action: &ActionRecord,
        _call: &ToolCall,
    ) -> Result<p::TrustedFacts, String> {
        Err("no proof".into())
    }
}

struct ScriptedProvider {
    turns: VecDeque<ModelTurn>,
    requests: Vec<ModelRequest>,
}

impl ScriptedProvider {
    fn new(turns: Vec<ModelTurn>) -> Self {
        Self {
            turns: turns.into(),
            requests: Vec::new(),
        }
    }
}

#[async_trait]
impl TurnProvider for ScriptedProvider {
    async fn next_turn(&mut self, request: ModelRequest) -> Result<ModelTurn, String> {
        self.requests.push(request);
        self.turns
            .pop_front()
            .ok_or_else(|| "the scripted model ran out of turns".to_string())
    }
}

struct RecordingExecutor {
    calls: Vec<ToolCall>,
    summary: Value,
    failing: bool,
}

impl RecordingExecutor {
    fn new(summary: Value) -> Self {
        Self {
            calls: Vec::new(),
            summary,
            failing: false,
        }
    }

    fn failing(summary: Value) -> Self {
        Self {
            calls: Vec::new(),
            summary,
            failing: true,
        }
    }
}

#[async_trait]
impl ToolExecutor for RecordingExecutor {
    async fn execute(
        &mut self,
        call: &ToolCall,
        _spec: &ToolSpec,
        _host: &foundryui_lib::foundry::agent_runtime::ToolHost<'_, '_>,
    ) -> ToolExecution {
        self.calls.push(call.clone());
        if self.failing {
            ToolExecution::Failed {
                summary: self.summary.clone(),
            }
        } else {
            ToolExecution::Succeeded {
                summary: self.summary.clone(),
            }
        }
    }
}

struct ScriptedApprovals {
    choice: ApprovalChoice,
}

impl ApprovalProvider for ScriptedApprovals {
    fn choose(&mut self, _request: &AuthorizationRequest) -> ApprovalChoice {
        self.choice
    }
}

struct RecordingSink {
    events: std::sync::Mutex<Vec<CycleEvent>>,
}

impl RecordingSink {
    fn new() -> Self {
        Self {
            events: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn kinds(&self) -> Vec<CycleEventKind> {
        self.events
            .lock()
            .expect("events")
            .iter()
            .map(|event| event.kind.clone())
            .collect()
    }
}

impl CycleSink for RecordingSink {
    fn emit(&self, event: &CycleEvent) -> Result<(), String> {
        self.events.lock().expect("events").push(event.clone());
        Ok(())
    }
}

fn read_spec() -> ToolSpec {
    ToolSpec {
        name: "read_file".into(),
        description: "Read an authorized file.".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1, "maxLength": 64}
            },
            "required": ["path"],
            "additionalProperties": false
        }),
    }
}

fn registry() -> ToolRegistry {
    let mut registry = ToolRegistry::default();
    registry
        .register(RegisteredTool {
            spec: read_spec(),
            availability: ToolAvailability::Available,
        })
        .unwrap();
    registry
        .register(RegisteredTool {
            spec: ToolSpec {
                name: "run_command".into(),
                description: "Run a controlled command.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {"argv": {"type": "array", "items": {"type": "string"}}},
                    "required": ["argv"],
                    "additionalProperties": false
                }),
            },
            availability: ToolAvailability::Unavailable {
                reason: ToolUnavailable::CapabilityBlocked,
            },
        })
        .unwrap();
    registry
}

fn call(id: &str, path: &str) -> ToolCall {
    ToolCall {
        id: id.into(),
        name: "read_file".into(),
        arguments: json!({"path": path}),
    }
}

fn turn(calls: Vec<ToolCall>) -> ModelTurn {
    ModelTurn {
        content: String::new(),
        reasoning: String::new(),
        calls,
    }
}

fn final_turn() -> ModelTurn {
    ModelTurn {
        content: "done".into(),
        reasoning: String::new(),
        calls: Vec::new(),
    }
}

fn limits() -> CycleLimits {
    CycleLimits::default()
}

fn request(conversation: i64) -> CycleRequest {
    CycleRequest {
        conversation_id: conversation,
        request_key: "cycle-request".into(),
        messages: vec![json!({"role": "user", "content": "do the work"})],
        model_supports_tools: true,
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Runtime::new()
        .expect("runtime")
        .block_on(future)
}

#[allow(clippy::too_many_arguments)]
fn run(
    fixture: &Fixture,
    registry: &ToolRegistry,
    provider: &mut ScriptedProvider,
    executor: &mut dyn ToolExecutor,
    authorizer: &dyn ToolAuthorizer,
    approvals: &mut dyn ApprovalProvider,
    limits: &CycleLimits,
    sink: &dyn CycleSink,
) -> CycleOutcome {
    block_on(run_cycle(
        &fixture.store(),
        CycleDeps {
            registry,
            provider,
            executor,
            authorizer,
            approvals,
            limits,
            sink,
        },
        request(fixture.conversation),
    ))
}

#[test]
fn the_cycle_continues_across_several_tools_without_losing_correlation() {
    let fixture = Fixture::project();
    let registry = registry();
    let mut provider = ScriptedProvider::new(vec![
        turn(vec![call("call_a", "a.txt"), call("call_b", "b.txt")]),
        final_turn(),
    ]);
    let mut executor = RecordingExecutor::new(json!({"content": "fixture"}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Suspend,
    };
    let outcome = run(
        &fixture,
        &registry,
        &mut provider,
        &mut executor,
        &ReadAuthorizer {
            root: &fixture.root,
        },
        &mut approvals,
        &limits(),
        &(),
    );

    assert_eq!(
        outcome,
        CycleOutcome::Completed {
            steps: 2,
            tool_calls: 2
        }
    );
    assert_eq!(executor.calls.len(), 2);
    let second_request = &provider.requests[1];
    assert_eq!(second_request.tools.len(), 1);
    let tool_messages = second_request
        .messages
        .iter()
        .filter(|message| message["role"] == "tool")
        .collect::<Vec<_>>();
    assert_eq!(tool_messages.len(), 2);
    assert_eq!(tool_messages[0]["tool_call_id"], "call_a");
    assert_eq!(tool_messages[1]["tool_call_id"], "call_b");

    let store = fixture.store();
    let history = store.history(fixture.conversation).unwrap();
    let calls = history
        .iter()
        .filter(|entry| matches!(entry.content, HistoryContent::ToolCall { .. }))
        .count();
    let results = history
        .iter()
        .filter(|entry| matches!(entry.content, HistoryContent::ToolResult { .. }))
        .count();
    assert_eq!((calls, results), (2, 2));
    assert_eq!(store.run(1).unwrap().state, RunState::Complete);
    assert_eq!(store.action(1).unwrap().state, ActionState::Complete);
}

#[test]
fn unknown_unavailable_and_invalid_calls_return_controlled_results() {
    let fixture = Fixture::project();
    let registry = registry();
    let mut provider = ScriptedProvider::new(vec![
        turn(vec![
            ToolCall {
                id: "call_unknown".into(),
                name: "shell".into(),
                arguments: json!({"argv": ["rm", "-rf", "/"]}),
            },
            ToolCall {
                id: "call_invalid".into(),
                name: "read_file".into(),
                arguments: json!({"limit": 4}),
            },
            ToolCall {
                id: "call_blocked".into(),
                name: "run_command".into(),
                arguments: json!({"argv": ["status"]}),
            },
        ]),
        final_turn(),
    ]);
    let mut executor = RecordingExecutor::new(json!({"ok": true}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Suspend,
    };
    let outcome = run(
        &fixture,
        &registry,
        &mut provider,
        &mut executor,
        &ReadAuthorizer {
            root: &fixture.root,
        },
        &mut approvals,
        &limits(),
        &(),
    );

    assert_eq!(
        outcome,
        CycleOutcome::Completed {
            steps: 2,
            tool_calls: 3
        }
    );
    assert!(executor.calls.is_empty());
    let store = fixture.store();
    let history = store.history(fixture.conversation).unwrap();
    let results = history
        .iter()
        .filter_map(|entry| match &entry.content {
            HistoryContent::ToolResult { summary, .. } => Some(summary.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 3);
    assert_eq!(results[0]["code"], "unknown_tool");
    assert_eq!(results[1]["code"], "invalid_arguments");
    assert_eq!(results[2]["code"], "capability_blocked");
    assert!(
        store
            .events(fixture.conversation)
            .unwrap()
            .iter()
            .any(|entry| {
                matches!(
                    entry.content,
                    HistoryContent::StateEvent {
                        event: foundryui_lib::foundry::db::agent::AgentEventState::ToolRejected,
                        ..
                    }
                )
            })
    );
}

#[test]
fn a_denial_reaches_the_model_without_granting_an_equivalent_alternative() {
    let fixture = Fixture::standalone();
    let registry = registry();
    let mut provider = ScriptedProvider::new(vec![
        turn(vec![call("call_a", "a.txt")]),
        turn(vec![call("call_b", "a.txt")]),
        final_turn(),
    ]);
    let mut executor = RecordingExecutor::new(json!({"content": "fixture"}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Deny,
    };
    let outcome = run(
        &fixture,
        &registry,
        &mut provider,
        &mut executor,
        &ReadAuthorizer {
            root: &fixture.root,
        },
        &mut approvals,
        &limits(),
        &(),
    );

    assert_eq!(
        outcome,
        CycleOutcome::Completed {
            steps: 3,
            tool_calls: 2
        }
    );
    assert!(executor.calls.is_empty());
    let store = fixture.store();
    assert_eq!(store.list_requests(fixture.conversation).unwrap().len(), 1);
    let history = store.history(fixture.conversation).unwrap();
    let results = history
        .iter()
        .filter_map(|entry| match &entry.content {
            HistoryContent::ToolResult { summary, .. } => Some(summary.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0]["status"], "denied");
    assert_eq!(results[1]["status"], "denied");
    assert_eq!(store.action(1).unwrap().state, ActionState::Denied);
    assert_eq!(store.action(2).unwrap().state, ActionState::Denied);
}

#[test]
fn duplicate_call_ids_never_execute_twice() {
    let fixture = Fixture::project();
    let registry = registry();
    let same = call("call_same", "a.txt");
    let mut provider = ScriptedProvider::new(vec![
        turn(vec![same.clone()]),
        turn(vec![same]),
        final_turn(),
    ]);
    let mut executor = RecordingExecutor::new(json!({"content": "fixture"}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Suspend,
    };
    let outcome = run(
        &fixture,
        &registry,
        &mut provider,
        &mut executor,
        &ReadAuthorizer {
            root: &fixture.root,
        },
        &mut approvals,
        &limits(),
        &(),
    );

    assert_eq!(
        outcome,
        CycleOutcome::Completed {
            steps: 3,
            tool_calls: 2
        }
    );
    assert_eq!(executor.calls.len(), 1);
    let store = fixture.store();
    let history = store.history(fixture.conversation).unwrap();
    let calls = history
        .iter()
        .filter(|entry| matches!(entry.content, HistoryContent::ToolCall { .. }))
        .count();
    assert_eq!(calls, 1);
    let second_request = &provider.requests[2];
    let tool_messages = second_request
        .messages
        .iter()
        .filter(|message| message["role"] == "tool")
        .collect::<Vec<_>>();
    assert_eq!(tool_messages.len(), 2);
    let first: Value = serde_json::from_str(tool_messages[0]["content"].as_str().unwrap()).unwrap();
    let second: Value =
        serde_json::from_str(tool_messages[1]["content"].as_str().unwrap()).unwrap();
    assert_eq!(first["status"], "succeeded");
    assert_eq!(second["status"], "succeeded");
}

#[test]
fn limits_end_the_run_observably_and_report_incomplete_work() {
    let fixture = Fixture::project();
    let registry = registry();
    let mut provider = ScriptedProvider::new(vec![
        turn(vec![call("call_a", "a.txt")]),
        turn(vec![call("call_b", "b.txt")]),
    ]);
    let mut executor = RecordingExecutor::new(json!({"content": "fixture"}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Suspend,
    };
    let limits = CycleLimits {
        max_steps: 1,
        ..CycleLimits::default()
    };
    let outcome = run(
        &fixture,
        &registry,
        &mut provider,
        &mut executor,
        &ReadAuthorizer {
            root: &fixture.root,
        },
        &mut approvals,
        &limits,
        &(),
    );

    assert!(matches!(
        outcome,
        CycleOutcome::Stopped {
            reason: StopReason::StepLimit,
            incomplete: true,
            ..
        }
    ));
    let store = fixture.store();
    assert_eq!(store.run(1).unwrap().state, RunState::Failed);
    let events = store.events(fixture.conversation).unwrap();
    assert!(events.iter().any(|entry| matches!(
        entry.content,
        HistoryContent::StateEvent {
            event: foundryui_lib::foundry::db::agent::AgentEventState::LimitReached,
            ..
        }
    )));
    assert!(events.iter().any(|entry| matches!(
        entry.content,
        HistoryContent::StateEvent {
            event: foundryui_lib::foundry::db::agent::AgentEventState::RunFailed,
            ..
        }
    )));
}

#[test]
fn a_repeated_identical_call_stops_before_it_runs_again() {
    let fixture = Fixture::project();
    let registry = registry();
    let mut provider = ScriptedProvider::new(vec![
        turn(vec![call("call_a", "a.txt")]),
        turn(vec![call("call_b", "a.txt")]),
    ]);
    let mut executor = RecordingExecutor::new(json!({"content": "fixture"}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Suspend,
    };
    let limits = CycleLimits {
        max_repetitions: 1,
        ..CycleLimits::default()
    };
    let outcome = run(
        &fixture,
        &registry,
        &mut provider,
        &mut executor,
        &ReadAuthorizer {
            root: &fixture.root,
        },
        &mut approvals,
        &limits,
        &(),
    );

    assert!(matches!(
        outcome,
        CycleOutcome::Stopped {
            reason: StopReason::RepetitionLimit,
            ..
        }
    ));
    assert_eq!(executor.calls.len(), 1);
}

#[test]
fn oversized_tool_output_is_bounded_and_marked_as_truncated() {
    let fixture = Fixture::project();
    let registry = registry();
    let mut provider =
        ScriptedProvider::new(vec![turn(vec![call("call_a", "a.txt")]), final_turn()]);
    let mut executor = RecordingExecutor::new(json!({"content": "x".repeat(8192)}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Suspend,
    };
    let limits = CycleLimits {
        max_output_bytes: 1024,
        ..CycleLimits::default()
    };
    run(
        &fixture,
        &registry,
        &mut provider,
        &mut executor,
        &ReadAuthorizer {
            root: &fixture.root,
        },
        &mut approvals,
        &limits,
        &(),
    );
    let tool_message = provider.requests[1]
        .messages
        .iter()
        .find(|message| message["role"] == "tool")
        .expect("tool message");
    let content: Value = serde_json::from_str(tool_message["content"].as_str().unwrap()).unwrap();
    assert_eq!(content["truncated"], true);
    assert!(
        serde_json::to_vec(&content).unwrap().len() <= 1024,
        "the bounded result must fit its limit: {content}"
    );
    assert!(content["preview"].as_str().unwrap().contains('x'));
}

#[test]
fn a_small_output_limit_is_still_respected() {
    let fixture = Fixture::project();
    let registry = registry();
    let mut provider =
        ScriptedProvider::new(vec![turn(vec![call("call_a", "a.txt")]), final_turn()]);
    let mut executor = RecordingExecutor::new(json!({"content": "x".repeat(4096)}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Suspend,
    };
    let limits = CycleLimits {
        max_output_bytes: 300,
        ..CycleLimits::default()
    };
    run(
        &fixture,
        &registry,
        &mut provider,
        &mut executor,
        &ReadAuthorizer {
            root: &fixture.root,
        },
        &mut approvals,
        &limits,
        &(),
    );
    let tool_message = provider.requests[1]
        .messages
        .iter()
        .find(|message| message["role"] == "tool")
        .expect("tool message");
    let content: Value = serde_json::from_str(tool_message["content"].as_str().unwrap()).unwrap();
    assert_eq!(content["truncated"], true);
    assert!(
        serde_json::to_vec(&content).unwrap().len() <= 300,
        "the bounded result must fit its limit: {content}"
    );
}

#[test]
fn incompatible_models_and_empty_registries_stay_in_plain_chat() {
    let fixture = Fixture::project();
    let registry = registry();
    let mut provider = ScriptedProvider::new(Vec::new());
    let mut executor = RecordingExecutor::new(json!({}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Suspend,
    };
    let mut incompatible = request(fixture.conversation);
    incompatible.model_supports_tools = false;
    let outcome = block_on(run_cycle(
        &fixture.store(),
        CycleDeps {
            registry: &registry,
            provider: &mut provider,
            executor: &mut executor,
            authorizer: &UnavailableAuthorizer,
            approvals: &mut approvals,
            limits: &limits(),
            sink: &(),
        },
        incompatible,
    ));
    assert_eq!(
        outcome,
        CycleOutcome::ChatOnly {
            reason: ChatOnlyReason::ModelIncompatible
        }
    );

    let empty = ToolRegistry::default();
    let outcome = block_on(run_cycle(
        &fixture.store(),
        CycleDeps {
            registry: &empty,
            provider: &mut provider,
            executor: &mut executor,
            authorizer: &UnavailableAuthorizer,
            approvals: &mut approvals,
            limits: &limits(),
            sink: &(),
        },
        request(fixture.conversation),
    ));
    assert_eq!(
        outcome,
        CycleOutcome::ChatOnly {
            reason: ChatOnlyReason::NoAvailableTools
        }
    );
    assert!(
        fixture
            .store()
            .history(fixture.conversation)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn pending_approvals_suspend_the_run_for_a_later_decision() {
    let fixture = Fixture::standalone();
    let registry = registry();
    let mut provider = ScriptedProvider::new(vec![turn(vec![call("call_a", "a.txt")])]);
    let mut executor = RecordingExecutor::new(json!({"content": "fixture"}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Suspend,
    };
    let outcome = run(
        &fixture,
        &registry,
        &mut provider,
        &mut executor,
        &ReadAuthorizer {
            root: &fixture.root,
        },
        &mut approvals,
        &limits(),
        &(),
    );
    assert!(matches!(
        outcome,
        CycleOutcome::AwaitingAuthorization { .. }
    ));
    assert!(executor.calls.is_empty());
    let store = fixture.store();
    assert_eq!(store.run(1).unwrap().state, RunState::Prepared);
    assert_eq!(store.action(1).unwrap().state, ActionState::Proposed);
    assert_eq!(store.list_requests(fixture.conversation).unwrap().len(), 1);
}

#[test]
fn tool_results_record_the_expected_outcome() {
    let fixture = Fixture::project();
    let registry = registry();
    let mut provider =
        ScriptedProvider::new(vec![turn(vec![call("call_a", "a.txt")]), final_turn()]);
    let mut executor = RecordingExecutor::new(json!({"content": "fixture"}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Suspend,
    };
    assert_eq!(
        run(
            &fixture,
            &registry,
            &mut provider,
            &mut executor,
            &ReadAuthorizer {
                root: &fixture.root,
            },
            &mut approvals,
            &limits(),
            &(),
        ),
        CycleOutcome::Completed {
            steps: 2,
            tool_calls: 1
        }
    );
    let result = fixture.store().tool_result_for(1).unwrap().unwrap();
    assert_eq!(result.0, ActionOutcome::Succeeded);
    assert_eq!(result.1["status"], "succeeded");
}

#[test]
fn runtime_events_are_identified_and_ordered() {
    let fixture = Fixture::project();
    let registry = registry();
    let mut provider =
        ScriptedProvider::new(vec![turn(vec![call("call_a", "a.txt")]), final_turn()]);
    let mut executor = RecordingExecutor::new(json!({"content": "fixture"}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Suspend,
    };
    let sink = RecordingSink::new();
    let outcome = run(
        &fixture,
        &registry,
        &mut provider,
        &mut executor,
        &ReadAuthorizer {
            root: &fixture.root,
        },
        &mut approvals,
        &limits(),
        &sink,
    );
    assert_eq!(
        outcome,
        CycleOutcome::Completed {
            steps: 2,
            tool_calls: 1
        }
    );

    let events = sink.events.lock().expect("events").clone();
    assert_eq!(
        sink.kinds(),
        vec![
            CycleEventKind::RunStarted,
            CycleEventKind::TurnStarted { step: 1 },
            CycleEventKind::ToolProposed {
                action_id: 1,
                call_id: "call_a".into(),
                tool: "read_file".into(),
            },
            CycleEventKind::ToolCompleted {
                action_id: 1,
                call_id: "call_a".into(),
                outcome: "succeeded".into(),
            },
            CycleEventKind::TurnStarted { step: 2 },
            CycleEventKind::RunCompleted,
        ]
    );
    for event in &events {
        assert_eq!(event.conversation_id, fixture.conversation);
        assert_eq!(event.run_id, 1);
        assert!(event.sequence.is_some());
    }
    let sequences = events
        .iter()
        .map(|event| event.sequence.unwrap())
        .collect::<Vec<_>>();
    assert!(sequences.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn a_failing_executor_returns_a_controlled_result() {
    let fixture = Fixture::project();
    let registry = registry();
    let mut provider =
        ScriptedProvider::new(vec![turn(vec![call("call_a", "a.txt")]), final_turn()]);
    let mut executor = RecordingExecutor::failing(json!({"error": "boom"}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Suspend,
    };
    let outcome = run(
        &fixture,
        &registry,
        &mut provider,
        &mut executor,
        &ReadAuthorizer {
            root: &fixture.root,
        },
        &mut approvals,
        &limits(),
        &(),
    );
    assert_eq!(
        outcome,
        CycleOutcome::Completed {
            steps: 2,
            tool_calls: 1
        }
    );
    assert_eq!(executor.calls.len(), 1);
    let tool_message = provider.requests[1]
        .messages
        .iter()
        .find(|message| message["role"] == "tool")
        .expect("tool message");
    let content: Value = serde_json::from_str(tool_message["content"].as_str().unwrap()).unwrap();
    assert_eq!(content["status"], "failed");
    let store = fixture.store();
    assert_eq!(store.action(1).unwrap().state, ActionState::Failed);
    assert_eq!(store.run(1).unwrap().state, RunState::Complete);
}

#[test]
fn an_authorizer_error_returns_a_controlled_result() {
    let fixture = Fixture::standalone();
    let registry = registry();
    let mut provider =
        ScriptedProvider::new(vec![turn(vec![call("call_a", "a.txt")]), final_turn()]);
    let mut executor = RecordingExecutor::new(json!({"content": "fixture"}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Suspend,
    };
    let outcome = run(
        &fixture,
        &registry,
        &mut provider,
        &mut executor,
        &UnavailableAuthorizer,
        &mut approvals,
        &limits(),
        &(),
    );
    assert_eq!(
        outcome,
        CycleOutcome::Completed {
            steps: 2,
            tool_calls: 1
        }
    );
    assert!(executor.calls.is_empty());
    let store = fixture.store();
    let results = store
        .history(fixture.conversation)
        .unwrap()
        .into_iter()
        .filter_map(|entry| match entry.content {
            HistoryContent::ToolResult { summary, .. } => Some(summary),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0]["code"], "authorization_unavailable");
}

#[test]
fn every_stop_reason_ends_the_run_observably() {
    let cases = [
        (
            CycleLimits {
                max_calls_per_turn: 1,
                ..CycleLimits::default()
            },
            vec![turn(vec![call("call_a", "a.txt"), call("call_b", "b.txt")])],
            StopReason::CallLimit,
        ),
        (
            CycleLimits {
                max_input_bytes: 1,
                ..CycleLimits::default()
            },
            vec![final_turn()],
            StopReason::InputLimit,
        ),
        (
            CycleLimits {
                max_duration: Duration::ZERO,
                ..CycleLimits::default()
            },
            vec![final_turn()],
            StopReason::DurationLimit,
        ),
    ];
    for (limits, turns, expected) in cases {
        let fixture = Fixture::project();
        let registry = registry();
        let mut provider = ScriptedProvider::new(turns);
        let mut executor = RecordingExecutor::new(json!({"content": "fixture"}));
        let mut approvals = ScriptedApprovals {
            choice: ApprovalChoice::Suspend,
        };
        let outcome = run(
            &fixture,
            &registry,
            &mut provider,
            &mut executor,
            &ReadAuthorizer {
                root: &fixture.root,
            },
            &mut approvals,
            &limits,
            &(),
        );
        assert!(
            matches!(
                outcome,
                CycleOutcome::Stopped {
                    reason,
                    incomplete: true,
                    ..
                } if reason == expected
            ),
            "expected {expected:?}, got {outcome:?}"
        );
        assert_eq!(fixture.store().run(1).unwrap().state, RunState::Failed);
    }
}

#[test]
fn a_duplicate_request_key_reports_the_finished_run() {
    let fixture = Fixture::project();
    let registry = registry();
    let mut provider = ScriptedProvider::new(vec![final_turn()]);
    let mut executor = RecordingExecutor::new(json!({"content": "fixture"}));
    let mut approvals = ScriptedApprovals {
        choice: ApprovalChoice::Suspend,
    };
    let first = run(
        &fixture,
        &registry,
        &mut provider,
        &mut executor,
        &ReadAuthorizer {
            root: &fixture.root,
        },
        &mut approvals,
        &limits(),
        &(),
    );
    assert_eq!(
        first,
        CycleOutcome::Completed {
            steps: 1,
            tool_calls: 0
        }
    );
    let second = run(
        &fixture,
        &registry,
        &mut provider,
        &mut executor,
        &ReadAuthorizer {
            root: &fixture.root,
        },
        &mut approvals,
        &limits(),
        &(),
    );
    assert_eq!(
        second,
        CycleOutcome::AlreadyFinished {
            run_state: RunState::Complete
        }
    );
    assert_eq!(provider.requests.len(), 1);
}

#[test]
fn the_assembled_agent_runs_read_tools_end_to_end() {
    let fixture = Fixture::project();
    std::fs::write(
        fixture._directory.path().join("a.txt"),
        b"hello from the project",
    )
    .expect("write");
    let mut provider =
        ScriptedProvider::new(vec![turn(vec![call("call_a", "a.txt")]), final_turn()]);
    let outcome = block_on(run_agent_cycle(
        &fixture.store(),
        request(fixture.conversation),
        &mut provider,
        &(),
    ));
    assert_eq!(
        outcome,
        CycleOutcome::Completed {
            steps: 2,
            tool_calls: 1
        }
    );
    let tool_message = provider.requests[1]
        .messages
        .iter()
        .find(|message| message["role"] == "tool")
        .expect("tool message");
    let content: Value = serde_json::from_str(tool_message["content"].as_str().unwrap()).unwrap();
    assert_eq!(content["status"], "succeeded");
    assert_eq!(content["content"], "hello from the project");
}

async fn next_pending(fixture: &Fixture) -> AuthorizationRequest {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Some(request) = fixture
                .store()
                .list_requests(fixture.conversation)
                .unwrap()
                .into_iter()
                .find(|request| request.status == AuthorizationStatus::Pending)
            {
                return request;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pending request")
}

#[test]
fn persisted_decisions_resume_the_same_turn_without_losing_remaining_calls() {
    let fixture = Fixture::standalone();
    let registry = registry();
    let mut provider = ScriptedProvider::new(vec![
        turn(vec![call("first", "a.txt"), call("second", "b.txt")]),
        final_turn(),
    ]);
    let mut executor = RecordingExecutor::new(json!({"content": "fixture"}));
    let mut approvals = PersistedApprovals;
    let sink = RecordingSink::new();
    let store = fixture.store();
    let authorizer = ReadAuthorizer {
        root: &fixture.root,
    };
    let limits = limits();
    let (outcome, ()) = block_on(async {
        tokio::join!(
            run_cycle(
                &store,
                CycleDeps {
                    registry: &registry,
                    provider: &mut provider,
                    executor: &mut executor,
                    authorizer: &authorizer,
                    approvals: &mut approvals,
                    limits: &limits,
                    sink: &sink
                },
                request(fixture.conversation)
            ),
            async {
                for _ in 0..2 {
                    let request = next_pending(&fixture).await;
                    store
                        .decide_request(
                            fixture.conversation,
                            request.id,
                            RequestDecision::Approve { remember: false },
                        )
                        .unwrap();
                }
            }
        )
    });
    assert_eq!(
        outcome,
        CycleOutcome::Completed {
            steps: 2,
            tool_calls: 2
        }
    );
    assert_eq!(
        executor
            .calls
            .iter()
            .map(|call| call.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
    assert_eq!(provider.requests.len(), 2);
    assert_eq!(
        provider.requests[1]
            .messages
            .iter()
            .filter(|message| message["role"] == "tool")
            .count(),
        2
    );
    assert_eq!(
        sink.kinds()
            .iter()
            .filter(|kind| matches!(kind, CycleEventKind::AuthorizationPending { .. }))
            .count(),
        2
    );
}

#[test]
fn a_persisted_denial_resumes_with_a_denied_result_without_execution() {
    let fixture = Fixture::standalone();
    let registry = registry();
    let mut provider =
        ScriptedProvider::new(vec![turn(vec![call("first", "a.txt")]), final_turn()]);
    let mut executor = RecordingExecutor::new(json!({}));
    let mut approvals = PersistedApprovals;
    let store = fixture.store();
    let authorizer = ReadAuthorizer {
        root: &fixture.root,
    };
    let limits = limits();
    let (outcome, ()) = block_on(async {
        tokio::join!(
            run_cycle(
                &store,
                CycleDeps {
                    registry: &registry,
                    provider: &mut provider,
                    executor: &mut executor,
                    authorizer: &authorizer,
                    approvals: &mut approvals,
                    limits: &limits,
                    sink: &()
                },
                request(fixture.conversation)
            ),
            async {
                let request = next_pending(&fixture).await;
                store
                    .decide_request(fixture.conversation, request.id, RequestDecision::Deny)
                    .unwrap();
            }
        )
    });
    assert_eq!(
        outcome,
        CycleOutcome::Completed {
            steps: 2,
            tool_calls: 1
        }
    );
    assert!(executor.calls.is_empty());
    let result = provider.requests[1]
        .messages
        .iter()
        .find(|message| message["role"] == "tool")
        .unwrap();
    let content: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
    assert_eq!(content["status"], "denied");
}

#[test]
fn cancellation_during_persisted_approval_wait_blocks_the_late_decision() {
    let fixture = Fixture::standalone();
    let registry = registry();
    let mut provider = ScriptedProvider::new(vec![turn(vec![call("first", "a.txt")])]);
    let mut executor = RecordingExecutor::new(json!({}));
    let mut approvals = PersistedApprovals;
    let store = fixture.store();
    let authorizer = ReadAuthorizer {
        root: &fixture.root,
    };
    let limits = limits();
    let (outcome, ()) = block_on(async {
        tokio::join!(
            run_cycle(
                &store,
                CycleDeps {
                    registry: &registry,
                    provider: &mut provider,
                    executor: &mut executor,
                    authorizer: &authorizer,
                    approvals: &mut approvals,
                    limits: &limits,
                    sink: &()
                },
                request(fixture.conversation)
            ),
            async {
                let request = next_pending(&fixture).await;
                store.cancel_run(request.run_id).unwrap();
                assert_eq!(
                    store.decide_request(
                        fixture.conversation,
                        request.id,
                        RequestDecision::Approve { remember: false }
                    ),
                    Err(AgentStoreError::InvalidTransition)
                );
            }
        )
    });
    assert_eq!(
        outcome,
        CycleOutcome::AlreadyFinished {
            run_state: RunState::Cancelled
        }
    );
    assert!(executor.calls.is_empty());
    assert_eq!(provider.requests.len(), 1);
}

#[test]
fn changing_mode_invalidates_the_wait_instead_of_granting_pending_access() {
    let fixture = Fixture::standalone();
    let registry = registry();
    let mut provider =
        ScriptedProvider::new(vec![turn(vec![call("first", "a.txt")]), final_turn()]);
    let mut executor = RecordingExecutor::new(json!({}));
    let mut approvals = PersistedApprovals;
    let store = fixture.store();
    let authorizer = ReadAuthorizer {
        root: &fixture.root,
    };
    let limits = limits();
    let (outcome, ()) = block_on(async {
        tokio::join!(
            run_cycle(
                &store,
                CycleDeps {
                    registry: &registry,
                    provider: &mut provider,
                    executor: &mut executor,
                    authorizer: &authorizer,
                    approvals: &mut approvals,
                    limits: &limits,
                    sink: &()
                },
                request(fixture.conversation)
            ),
            async {
                let request = next_pending(&fixture).await;
                let revision = store.authorization_revision(fixture.conversation).unwrap();
                store
                    .set_authorization_mode(revision.key(), AgentMode::Automatic)
                    .unwrap();
                assert!(
                    store
                        .decide_request(
                            fixture.conversation,
                            request.id,
                            RequestDecision::Approve { remember: false }
                        )
                        .is_err()
                );
            }
        )
    });
    assert_eq!(
        outcome,
        CycleOutcome::Completed {
            steps: 2,
            tool_calls: 1
        }
    );
    assert!(executor.calls.is_empty());
    let result = provider.requests[1]
        .messages
        .iter()
        .find(|message| message["role"] == "tool")
        .unwrap();
    let content: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
    assert_eq!(content["code"], "authorization_changed");
}

#[test]
fn persisted_approval_wait_obeys_the_cycle_deadline() {
    let fixture = Fixture::standalone();
    let registry = registry();
    let mut provider = ScriptedProvider::new(vec![turn(vec![call("first", "a.txt")])]);
    let mut executor = RecordingExecutor::new(json!({}));
    let mut approvals = PersistedApprovals;
    let limits = CycleLimits {
        max_duration: Duration::from_millis(50),
        ..limits()
    };
    let outcome = block_on(run_cycle(
        &fixture.store(),
        CycleDeps {
            registry: &registry,
            provider: &mut provider,
            executor: &mut executor,
            authorizer: &ReadAuthorizer {
                root: &fixture.root,
            },
            approvals: &mut approvals,
            limits: &limits,
            sink: &(),
        },
        request(fixture.conversation),
    ));
    assert!(matches!(
        outcome,
        CycleOutcome::Stopped {
            reason: StopReason::DurationLimit,
            ..
        }
    ));
    assert!(executor.calls.is_empty());
}

#[test]
fn resumed_dispatch_revalidates_facts_before_executing() {
    struct ChangingAuthorizer<'a> {
        root: &'a RootHandle,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl ToolAuthorizer for ChangingAuthorizer<'_> {
        fn facts(
            &self,
            store: &foundryui_lib::foundry::db::agent::AgentStore<'_>,
            revision: &AuthorizationRevision,
            action: &ActionRecord,
            call: &ToolCall,
        ) -> Result<p::TrustedFacts, String> {
            let mut facts =
                ReadAuthorizer { root: self.root }.facts(store, revision, action, call)?;
            if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) != 0 {
                facts.action.resources[0].version = "changed".into();
                facts.resources[0].resource.version = "changed".into();
                facts.read_evidence = p::ReadEvidence::ProvenReads {
                    resources: facts.action.resources.clone(),
                };
            }
            Ok(facts)
        }
    }
    let fixture = Fixture::standalone();
    let registry = registry();
    let mut provider =
        ScriptedProvider::new(vec![turn(vec![call("first", "a.txt")]), final_turn()]);
    let mut executor = RecordingExecutor::new(json!({}));
    let mut approvals = PersistedApprovals;
    let store = fixture.store();
    let authorizer = ChangingAuthorizer {
        root: &fixture.root,
        calls: std::sync::atomic::AtomicUsize::new(0),
    };
    let limits = limits();
    let (outcome, ()) = block_on(async {
        tokio::join!(
            run_cycle(
                &store,
                CycleDeps {
                    registry: &registry,
                    provider: &mut provider,
                    executor: &mut executor,
                    authorizer: &authorizer,
                    approvals: &mut approvals,
                    limits: &limits,
                    sink: &()
                },
                request(fixture.conversation)
            ),
            async {
                let request = next_pending(&fixture).await;
                store
                    .decide_request(
                        fixture.conversation,
                        request.id,
                        RequestDecision::Approve { remember: false },
                    )
                    .unwrap();
                assert_eq!(
                    store.decide_request(
                        fixture.conversation,
                        request.id,
                        RequestDecision::Approve { remember: false }
                    ),
                    Err(AgentStoreError::InvalidTransition)
                );
            }
        )
    });
    assert_eq!(
        outcome,
        CycleOutcome::Completed {
            steps: 2,
            tool_calls: 1
        }
    );
    assert!(executor.calls.is_empty());
    let result = provider.requests[1]
        .messages
        .iter()
        .find(|message| message["role"] == "tool")
        .unwrap();
    let content: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
    assert_eq!(content["code"], "authorization_changed");
}
