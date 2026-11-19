use super::*;
use crate::foundry::db::Database;
use crate::foundry::db::{MessageRole, MessageStatus};
use serde_json::json;

#[test]
fn the_same_run_request_returns_the_original_record() {
    let database = Database::open_in_memory().expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let first = database
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("run");
    let repeated = database
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("same run");

    assert_eq!(first, repeated);
    assert_eq!(first.state, RunState::Prepared);
    assert_eq!(first.context_revision, 1);
    assert_eq!(
        database.agent_store().run(first.id).expect("stored run"),
        first
    );
}

#[test]
fn calls_are_structured_ordered_and_sanitized_without_losing_user_messages() {
    let database = Database::open_in_memory().expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    database
        .append_message(
            conversation.id,
            MessageRole::User,
            "inspect the fixture",
            "",
            None,
            MessageStatus::Complete,
        )
        .expect("message");
    let run = database
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("run");
    let call = database
        .agent_store()
        .propose_call(
            run.id,
            "call-a",
            "read_file",
            &json!({"path":"private-fixture", "token":"fixture-secret"}),
        )
        .expect("call");
    database
        .append_message(
            conversation.id,
            MessageRole::Assistant,
            "waiting for approval",
            "",
            None,
            MessageStatus::Complete,
        )
        .expect("message");

    let history = database
        .agent_store()
        .history(conversation.id)
        .expect("history");
    assert_eq!(history.len(), 3);
    assert!(
        history
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence)
    );
    assert!(
        matches!(&history[0].content, HistoryContent::Text { role: MessageRole::User, content, .. } if content == "inspect the fixture")
    );
    assert!(
        matches!(&history[1].content, HistoryContent::ToolCall { call_id, tool, .. } if call_id == "call-a" && tool == "read_file")
    );
    assert_eq!(
        call.arguments,
        json!({"path":"[redacted]", "token":"[redacted]"})
    );
    let stored = serde_json::to_string(&history).expect("history JSON");
    assert!(!stored.contains("fixture-secret"));
    assert!(!stored.contains("private-fixture"));
    assert_eq!(call.arguments_fingerprint.len(), 64);
}

#[test]
fn repeated_call_ids_are_idempotent_only_for_the_exact_operation() {
    let database = Database::open_in_memory().expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let run = database
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("run");
    let first = database
        .agent_store()
        .propose_call(
            run.id,
            "call-a",
            "read_file",
            &json!({"path":"a", "limit": 10}),
        )
        .expect("call");
    let repeated = database
        .agent_store()
        .propose_call(
            run.id,
            "call-a",
            "read_file",
            &json!({"limit":10, "path":"a"}),
        )
        .expect("same call");
    assert_eq!(first, repeated);
    assert_eq!(
        database
            .agent_store()
            .history(conversation.id)
            .expect("history")
            .len(),
        1
    );
    assert_eq!(
        database.agent_store().propose_call(
            run.id,
            "call-a",
            "read_file",
            &json!({"path":"b", "limit":10})
        ),
        Err(AgentStoreError::Conflict)
    );
    assert_eq!(
        database.agent_store().propose_call(
            run.id,
            "call-a",
            "write_file",
            &json!({"path":"a", "limit":10})
        ),
        Err(AgentStoreError::Conflict)
    );
}

#[test]
fn approvals_bind_arguments_and_a_call_can_be_claimed_only_once() {
    let database = Database::open_in_memory().expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let run = database
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("run");
    let arguments = json!({"path":"fixture-a"});
    let call = database
        .agent_store()
        .propose_call(run.id, "call-a", "read_file", &arguments)
        .expect("call");
    assert!(matches!(
        database.agent_store().claim_call(&call, &arguments),
        Err(AgentStoreError::InvalidTransition)
    ));
    assert_eq!(
        database.agent_store().decide_call(
            &call,
            &json!({"path":"fixture-b"}),
            ApprovalDecision::Approved
        ),
        Err(AgentStoreError::Conflict)
    );
    database
        .agent_store()
        .decide_call(&call, &arguments, ApprovalDecision::Approved)
        .expect("approval");
    let claim = database
        .agent_store()
        .claim_call(&call, &arguments)
        .expect("claim");
    assert!(matches!(
        database.agent_store().claim_call(&call, &arguments),
        Err(AgentStoreError::AlreadyClaimed)
    ));
    database
        .agent_store()
        .complete_call(
            &claim,
            ActionOutcome::Succeeded,
            &json!({"text":"fixture-secret-result"}),
        )
        .expect("complete");
    database
        .agent_store()
        .complete_call(
            &claim,
            ActionOutcome::Succeeded,
            &json!({"text":"fixture-secret-result"}),
        )
        .expect("idempotent complete");
    assert!(matches!(
        database.agent_store().claim_call(&call, &arguments),
        Err(AgentStoreError::AlreadyClaimed)
    ));
    let history = database
        .agent_store()
        .history(conversation.id)
        .expect("history");
    assert_eq!(history.len(), 2);
    assert!(
        matches!(&history[1].content, HistoryContent::ToolResult { call_id, outcome: ActionOutcome::Succeeded, .. } if call_id == "call-a")
    );
    assert!(
        !serde_json::to_string(&history)
            .expect("JSON")
            .contains("fixture-secret-result")
    );
    assert_eq!(
        database
            .agent_store()
            .action(call.id)
            .expect("action")
            .state,
        ActionState::Complete
    );
}

#[test]
fn reopening_marks_unfinished_actions_uncertain_without_replaying_or_inventing_results() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let path = folder.path().join("agent.sqlite3");
    let database = Database::open(&path).expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let run = database
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("run");
    let arguments = json!({"path":"fixture-a"});
    let call = database
        .agent_store()
        .propose_call(run.id, "call-a", "read_file", &arguments)
        .expect("call");
    database
        .agent_store()
        .decide_call(&call, &arguments, ApprovalDecision::Approved)
        .expect("approve");
    let claim = database
        .agent_store()
        .claim_call(&call, &arguments)
        .expect("claim");
    drop(database);

    let reopened = Database::open(&path).expect("reopen");
    assert_eq!(
        reopened.agent_store().run(run.id).expect("run").state,
        RunState::Interrupted
    );
    assert_eq!(
        reopened
            .agent_store()
            .action(call.id)
            .expect("action")
            .state,
        ActionState::Interrupted
    );
    assert_eq!(
        reopened.agent_store().advance_context(conversation.id, 1),
        Err(AgentStoreError::InvalidTransition)
    );
    assert!(
        reopened
            .agent_store()
            .claim_call(&call, &arguments)
            .is_err()
    );
    assert!(
        reopened
            .agent_store()
            .complete_call(&claim, ActionOutcome::Succeeded, &json!({"text":"late"}))
            .is_err()
    );
    let repeated = reopened
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("same request");
    assert_eq!(repeated.id, run.id);
    assert_eq!(repeated.state, RunState::Interrupted);
    assert_eq!(
        reopened
            .agent_store()
            .history(conversation.id)
            .expect("history")
            .len(),
        1
    );
    let another = reopened
        .agent_store()
        .start_run(conversation.id, "request-b")
        .expect("new prepared run");
    let another_call = reopened
        .agent_store()
        .propose_call(another.id, "call-b", "read_file", &arguments)
        .expect("new call");
    reopened
        .agent_store()
        .decide_call(&another_call, &arguments, ApprovalDecision::Approved)
        .expect("new approval");
    assert!(matches!(
        reopened.agent_store().claim_call(&another_call, &arguments),
        Err(AgentStoreError::InvalidTransition)
    ));
}

#[test]
fn snapshots_remain_independent_when_the_source_is_edited_or_deleted() {
    let database = Database::open_in_memory().expect("database");
    let source = database
        .create_conversation("fixture-model")
        .expect("source");
    let destination = database
        .create_conversation("fixture-model")
        .expect("destination");
    database
        .append_message(
            source.id,
            MessageRole::User,
            "reference before edit",
            "",
            None,
            MessageStatus::Complete,
        )
        .expect("message");
    let snapshot = database
        .agent_store()
        .capture_snapshot(source.id, destination.id)
        .expect("capture");
    database
        .append_message(
            source.id,
            MessageRole::Assistant,
            "later message",
            "",
            None,
            MessageStatus::Complete,
        )
        .expect("later message");
    database
        .delete_conversation(source.id)
        .expect("delete source");
    let stored = database
        .agent_store()
        .snapshot(destination.id, snapshot.id)
        .expect("copy survives");
    assert_eq!(stored, snapshot);
    assert_eq!(stored.entries.len(), 1);
    assert!(
        matches!(&stored.entries[0].content, HistoryContent::Text {content, ..} if content == "reference before edit")
    );
    assert_eq!(
        database.agent_store().snapshot(source.id, snapshot.id),
        Err(AgentStoreError::NotFound)
    );
    database
        .delete_conversation(destination.id)
        .expect("delete destination");
    assert_eq!(
        database.agent_store().snapshot(destination.id, snapshot.id),
        Err(AgentStoreError::NotFound)
    );
}

#[test]
fn context_changes_invalidate_old_decisions_and_cannot_replace_active_runs() {
    let database = Database::open_in_memory().expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let run = database
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("run");
    let arguments = json!({"path":"fixture-a"});
    let call = database
        .agent_store()
        .propose_call(run.id, "call-a", "read_file", &arguments)
        .expect("call");
    assert_eq!(
        database.agent_store().advance_context(conversation.id, 1),
        Err(AgentStoreError::InvalidTransition)
    );
    assert_eq!(
        database.agent_store().finish_run(run.id),
        Err(AgentStoreError::InvalidTransition)
    );
    database
        .agent_store()
        .decide_call(&call, &arguments, ApprovalDecision::Denied)
        .expect("deny");
    database
        .agent_store()
        .finish_run(run.id)
        .expect("finish run");
    assert_eq!(
        database
            .agent_store()
            .advance_context(conversation.id, 1)
            .expect("new revision"),
        2
    );
    assert_eq!(
        database
            .agent_store()
            .decide_call(&call, &arguments, ApprovalDecision::Approved),
        Err(AgentStoreError::StaleContext)
    );
    assert_eq!(
        database
            .agent_store()
            .start_run(conversation.id, "request-a"),
        Err(AgentStoreError::StaleContext)
    );
    assert_eq!(
        database
            .agent_store()
            .start_run(conversation.id, "request-b")
            .expect("new run")
            .context_revision,
        2
    );
}

#[test]
fn typed_context_facts_are_owned_by_the_conversation_and_network_metadata_is_sanitized() {
    let database = Database::open_in_memory().expect("database");
    let source = database
        .create_conversation("fixture-model")
        .expect("source");
    let other = database
        .create_conversation("fixture-model")
        .expect("other");
    let run = database
        .agent_store()
        .start_run(source.id, "request-a")
        .expect("run");
    let hash = "a".repeat(64);
    let facts = [
        ContextFact::PermissionGrant {
            operation: "run_tests".into(),
            constraints_fingerprint: hash.clone(),
        },
        ContextFact::AuthorizedRoot {
            identity_key: "fixture:1:2".into(),
            display_path: "/selected/fixture".into(),
        },
        ContextFact::InstructionApproval {
            identity_key: "fixture:1:3".into(),
            content_fingerprint: hash.clone(),
        },
        ContextFact::ContextManifest {
            sources: vec![SourceReference {
                identity_key: "fixture:1:3".into(),
                content_fingerprint: hash.clone(),
            }],
        },
        ContextFact::NetworkEvent {
            destination: "https://user:fixture-secret@example.invalid/private?token=fixture-secret"
                .into(),
            state: NetworkEventState::Blocked,
            http_status: None,
        },
        ContextFact::ChangeSet {
            proposal_fingerprint: hash.clone(),
            resource_keys: vec!["fixture:1:3".into()],
        },
    ];
    for (index, fact) in facts.iter().enumerate() {
        database
            .agent_store()
            .record_fact(source.id, 1, Some(run.id), &format!("fact-{index}"), fact)
            .expect("fact");
    }
    let stored = database.agent_store().facts(source.id).expect("facts");
    assert_eq!(stored.len(), 6);
    assert!(
        database
            .agent_store()
            .facts(other.id)
            .expect("other facts")
            .is_empty()
    );
    assert_eq!(
        database.agent_store().fact(other.id, stored[0].id),
        Err(AgentStoreError::NotFound)
    );
    let encoded = serde_json::to_string(&stored).expect("JSON");
    assert!(!encoded.contains("fixture-secret"));
    assert!(!encoded.contains("/private"));
    let recovery = ContextFact::RecoveryRecord {
        change_set_id: stored[5].id,
        git_reference: "refs/foundryui/recovery/fixture".into(),
    };
    let saved = database
        .agent_store()
        .record_fact(source.id, 1, Some(run.id), "recovery-a", &recovery)
        .expect("recovery");
    assert_eq!(
        database
            .agent_store()
            .fact(source.id, saved.id)
            .expect("fact"),
        saved
    );
    database
        .agent_store()
        .finish_run(run.id)
        .expect("finish run");
    database.delete_conversation(source.id).expect("delete");
    assert!(
        database
            .agent_store()
            .facts(source.id)
            .expect("deleted facts")
            .is_empty()
    );
}

#[test]
fn project_records_deduplicate_live_identity_hints_and_contexts_default_to_review() {
    let database = Database::open_in_memory().expect("database");
    let first = database
        .agent_store()
        .record_project("Fixture", "darwin:fixture:1:2", "/selected/fixture")
        .expect("project");
    let same = database
        .agent_store()
        .record_project("Alias", "darwin:fixture:1:2", "/selected/alias")
        .expect("same project");
    assert_eq!(first, same);
    assert_eq!(
        database.agent_store().project(first.id).expect("project"),
        first
    );
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let context = database
        .agent_store()
        .context(conversation.id)
        .expect("context");
    assert_eq!(context.mode, AgentMode::Review);
    assert_eq!(context.revision, 1);
    assert_eq!(context.project_id, None);
}

#[test]
fn the_durable_event_journal_orders_intent_approval_start_and_result_without_copying_permissions() {
    let database = Database::open_in_memory().expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let destination = database
        .create_conversation("fixture-model")
        .expect("destination");
    let arguments = json!({"path":"fixture-a"});
    let run = database
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("run");
    let call = database
        .agent_store()
        .propose_call(run.id, "call-a", "read_file", &arguments)
        .expect("call");
    database
        .agent_store()
        .decide_call(&call, &arguments, ApprovalDecision::Approved)
        .expect("approve");
    let claim = database
        .agent_store()
        .claim_call(&call, &arguments)
        .expect("claim");
    database
        .agent_store()
        .complete_call(
            &claim,
            ActionOutcome::Succeeded,
            &json!({"summary":"fixture"}),
        )
        .expect("complete");
    database
        .agent_store()
        .finish_run(run.id)
        .expect("finish run");
    let events = database
        .agent_store()
        .events(conversation.id)
        .expect("events");
    assert_eq!(events.len(), 6);
    assert!(
        events
            .windows(2)
            .all(|pair| pair[0].sequence < pair[1].sequence)
    );
    assert!(matches!(
        events[0].content,
        HistoryContent::StateEvent {
            event: AgentEventState::RunPrepared,
            ..
        }
    ));
    assert!(
        matches!(events[2].content, HistoryContent::StateEvent {event: AgentEventState::Approved, action_id: Some(id), ..} if id == call.id)
    );
    assert!(matches!(
        events[3].content,
        HistoryContent::StateEvent {
            event: AgentEventState::Started,
            ..
        }
    ));
    assert!(matches!(
        events[5].content,
        HistoryContent::StateEvent {
            event: AgentEventState::RunCompleted,
            ..
        }
    ));
    let snapshot = database
        .agent_store()
        .capture_snapshot(conversation.id, destination.id)
        .expect("capture");
    assert_eq!(snapshot.entries.len(), 2);
    assert!(
        !snapshot
            .entries
            .iter()
            .any(|entry| matches!(entry.content, HistoryContent::StateEvent { .. }))
    );
}

#[test]
fn historical_facts_remain_auditable_but_are_not_current_context_decisions() {
    let database = Database::open_in_memory().expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let run = database
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("run");
    let fact = ContextFact::PermissionGrant {
        operation: "run_tests".into(),
        constraints_fingerprint: "a".repeat(64),
    };
    let saved = database
        .agent_store()
        .record_fact(conversation.id, 1, Some(run.id), "permission-a", &fact)
        .expect("fact");
    assert_eq!(
        database
            .agent_store()
            .record_fact(conversation.id, 1, Some(run.id), "permission-a", &fact)
            .expect("same fact"),
        saved
    );
    database
        .agent_store()
        .finish_run(run.id)
        .expect("finish run");
    database
        .agent_store()
        .advance_context(conversation.id, 1)
        .expect("advance");
    assert!(
        database
            .agent_store()
            .active_facts(conversation.id, 2)
            .expect("current facts")
            .is_empty()
    );
    assert_eq!(
        database.agent_store().active_facts(conversation.id, 1),
        Err(AgentStoreError::StaleContext)
    );
    assert_eq!(
        database
            .agent_store()
            .facts(conversation.id)
            .expect("audit facts")
            .len(),
        1
    );
    assert_eq!(
        database
            .agent_store()
            .record_fact(conversation.id, 1, Some(run.id), "late", &fact),
        Err(AgentStoreError::StaleContext)
    );
}

#[test]
fn rejected_result_metadata_does_not_finish_or_duplicate_the_call() {
    let database = Database::open_in_memory().expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let run = database
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("run");
    let arguments = json!({"path":"fixture-a"});
    let call = database
        .agent_store()
        .propose_call(run.id, "call-a", "read_file", &arguments)
        .expect("call");
    database
        .agent_store()
        .decide_call(&call, &arguments, ApprovalDecision::Approved)
        .expect("approve");
    let claim = database
        .agent_store()
        .claim_call(&call, &arguments)
        .expect("claim");
    assert_eq!(
        database.agent_store().complete_call(
            &claim,
            ActionOutcome::Succeeded,
            &json!({"oversized":"x".repeat(65_536)})
        ),
        Err(AgentStoreError::InvalidInput)
    );
    assert_eq!(
        database
            .agent_store()
            .action(call.id)
            .expect("action")
            .state,
        ActionState::Started
    );
    assert_eq!(
        database
            .agent_store()
            .history(conversation.id)
            .expect("history")
            .len(),
        1
    );
    database
        .agent_store()
        .complete_call(
            &claim,
            ActionOutcome::Failed,
            &json!({"code":"bounded_failure"}),
        )
        .expect("complete");
    assert_eq!(
        database.agent_store().complete_call(
            &claim,
            ActionOutcome::Succeeded,
            &json!({"code":"different"})
        ),
        Err(AgentStoreError::Conflict)
    );
    assert_eq!(
        database
            .agent_store()
            .history(conversation.id)
            .expect("history")
            .len(),
        2
    );
}

#[test]
fn only_one_concurrent_caller_obtains_the_action_claim() {
    let database = Database::open_in_memory().expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let run = database
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("run");
    let arguments = json!({"path":"fixture-a"});
    let call = database
        .agent_store()
        .propose_call(run.id, "call-a", "read_file", &arguments)
        .expect("call");
    database
        .agent_store()
        .decide_call(&call, &arguments, ApprovalDecision::Approved)
        .expect("approve");
    let barrier = std::sync::Barrier::new(8);
    let acquired = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    database.agent_store().claim_call(&call, &arguments)
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().expect("worker").is_ok() as usize)
            .sum::<usize>()
    });
    assert_eq!(acquired, 1);
}

#[test]
fn completed_history_and_independent_copies_survive_reopening() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let path = folder.path().join("agent.sqlite3");
    let database = Database::open(&path).expect("database");
    let source = database
        .create_conversation("fixture-model")
        .expect("source");
    let destination = database
        .create_conversation("fixture-model")
        .expect("destination");
    let arguments = json!({"token":"ephemeral-fixture-secret"});
    let run = database
        .agent_store()
        .start_run(source.id, "request-a")
        .expect("run");
    let call = database
        .agent_store()
        .propose_call(run.id, "call-a", "read_file", &arguments)
        .expect("call");
    database
        .agent_store()
        .decide_call(&call, &arguments, ApprovalDecision::Approved)
        .expect("approve");
    let claim = database
        .agent_store()
        .claim_call(&call, &arguments)
        .expect("claim");
    database
        .agent_store()
        .complete_call(
            &claim,
            ActionOutcome::Succeeded,
            &json!({"result":"fixture"}),
        )
        .expect("complete");
    database
        .agent_store()
        .finish_run(run.id)
        .expect("finish run");
    let snapshot = database
        .agent_store()
        .capture_snapshot(source.id, destination.id)
        .expect("snapshot");
    let events = database.agent_store().events(source.id).expect("events");
    drop(database);
    let reopened = Database::open(&path).expect("reopen");
    assert_eq!(
        reopened.agent_store().run(run.id).expect("run").state,
        RunState::Complete
    );
    assert_eq!(
        reopened.agent_store().events(source.id).expect("events"),
        events
    );
    assert_eq!(
        reopened
            .agent_store()
            .snapshot(destination.id, snapshot.id)
            .expect("snapshot"),
        snapshot
    );
    assert!(matches!(
        reopened.agent_store().claim_call(&call, &arguments),
        Err(AgentStoreError::AlreadyClaimed)
    ));
    assert!(
        !serde_json::to_string(&events)
            .expect("JSON")
            .contains("ephemeral-fixture-secret")
    );
    reopened
        .delete_conversation(source.id)
        .expect("delete completed conversation");
    assert_eq!(
        reopened.agent_store().action(call.id),
        Err(AgentStoreError::NotFound)
    );
    assert_eq!(
        reopened
            .agent_store()
            .snapshot(destination.id, snapshot.id)
            .expect("independent snapshot"),
        snapshot
    );
}

#[test]
fn facts_cannot_reference_another_conversations_execution_or_change_set() {
    let database = Database::open_in_memory().expect("database");
    let source = database
        .create_conversation("fixture-model")
        .expect("source");
    let destination = database
        .create_conversation("fixture-model")
        .expect("destination");
    let run = database
        .agent_store()
        .start_run(source.id, "request-a")
        .expect("run");
    database
        .agent_store()
        .context(destination.id)
        .expect("destination context");
    let fact = ContextFact::ChangeSet {
        proposal_fingerprint: "a".repeat(64),
        resource_keys: vec![],
    };
    let saved = database
        .agent_store()
        .record_fact(source.id, 1, Some(run.id), "change-a", &fact)
        .expect("fact");
    assert_eq!(
        database
            .agent_store()
            .record_fact(destination.id, 1, Some(run.id), "wrong-run", &fact),
        Err(AgentStoreError::Conflict)
    );
    let recovery = ContextFact::RecoveryRecord {
        change_set_id: saved.id,
        git_reference: "refs/foundryui/recovery/fixture".into(),
    };
    assert_eq!(
        database
            .agent_store()
            .record_fact(destination.id, 1, None, "wrong-change", &recovery),
        Err(AgentStoreError::NotFound)
    );
    assert!(
        database
            .agent_store()
            .facts(destination.id)
            .expect("facts")
            .is_empty()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn live_root_identity_deduplicates_project_records_across_selected_aliases() {
    use crate::foundry::resources::RootHandle;
    let folder = tempfile::tempdir().expect("temporary directory");
    let selected = folder.path().join("selected");
    let alias = folder.path().join("alias");
    std::fs::create_dir(&selected).expect("directory");
    std::os::unix::fs::symlink(&selected, &alias).expect("alias");
    let root = RootHandle::enroll(&selected).expect("root");
    let alias = RootHandle::enroll(&alias).expect("alias root");
    let database = Database::open_in_memory().expect("database");
    let first = database
        .agent_store()
        .record_project(
            "Selected",
            &root.identity().expect("identity").storage_key(),
            &root.display_path().to_string_lossy(),
        )
        .expect("project");
    let same = database
        .agent_store()
        .record_project(
            "Alias",
            &alias.identity().expect("identity").storage_key(),
            &alias.display_path().to_string_lossy(),
        )
        .expect("same project");
    assert_eq!(first, same);
    assert!(root.operation_io_available().expect("operation capability"));
}

#[test]
fn malformed_call_arguments_do_not_create_actions_or_history_entries() {
    let database = Database::open_in_memory().expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let run = database
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("run");
    for (index, arguments) in [
        json!(null),
        json!(true),
        json!(12),
        json!("opaque"),
        json!([]),
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            database.agent_store().propose_call(
                run.id,
                &format!("invalid-{index}"),
                "read_file",
                arguments
            ),
            Err(AgentStoreError::InvalidInput)
        );
    }
    assert!(
        database
            .agent_store()
            .history(conversation.id)
            .expect("history")
            .is_empty()
    );
}

#[test]
fn dynamic_object_keys_are_sanitized_in_calls_results_and_snapshots() {
    let database = Database::open_in_memory().expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let destination = database
        .create_conversation("fixture-model")
        .expect("destination");
    let run = database
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("run");
    let arguments = json!({"files":{"/private/customer-secret.txt":"contents", "customer_access_token_abc123":"value"}});
    let call = database
        .agent_store()
        .propose_call(run.id, "call-a", "read_file", &arguments)
        .expect("call");
    database
        .agent_store()
        .decide_call(&call, &arguments, ApprovalDecision::Approved)
        .expect("approve");
    let claim = database
        .agent_store()
        .claim_call(&call, &arguments)
        .expect("claim");
    database
        .agent_store()
        .complete_call(&claim, ActionOutcome::Succeeded, &arguments)
        .expect("complete");
    let snapshot = database
        .agent_store()
        .capture_snapshot(conversation.id, destination.id)
        .expect("snapshot");
    let encoded = serde_json::to_string(&(
        call,
        database
            .agent_store()
            .history(conversation.id)
            .expect("history"),
        snapshot,
    ))
    .expect("JSON");
    assert!(!encoded.contains("customer-secret"));
    assert!(!encoded.contains("customer_access_token_abc123"));
    assert!(!encoded.contains("contents"));
}

#[test]
fn conversation_deletion_preserves_started_and_uncertain_writer_barriers() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let path = folder.path().join("agent.sqlite3");
    let database = Database::open(&path).expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let run = database
        .agent_store()
        .start_run(conversation.id, "request-a")
        .expect("run");
    let arguments = json!({"path":"fixture-a"});
    let call = database
        .agent_store()
        .propose_call(run.id, "call-a", "read_file", &arguments)
        .expect("call");
    database
        .agent_store()
        .decide_call(&call, &arguments, ApprovalDecision::Approved)
        .expect("approve");
    let _claim = database
        .agent_store()
        .claim_call(&call, &arguments)
        .expect("claim");
    assert!(database.delete_conversation(conversation.id).is_err());
    assert_eq!(
        database
            .agent_store()
            .action(call.id)
            .expect("action remains")
            .state,
        ActionState::Started
    );
    drop(database);
    let reopened = Database::open(&path).expect("reopen");
    assert_eq!(
        reopened
            .agent_store()
            .action(call.id)
            .expect("uncertain action")
            .state,
        ActionState::Interrupted
    );
    assert!(reopened.delete_conversation(conversation.id).is_err());
    assert!(reopened.conversation(conversation.id).is_ok());
    assert!(
        !reopened
            .agent_store()
            .events(conversation.id)
            .expect("events remain")
            .is_empty()
    );
}

#[test]
fn database_contention_retains_safe_error_codes_without_payloads() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let path = folder.path().join("agent.sqlite3");
    let database = Database::open(&path).expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let mut external_writer =
        rusqlite::Connection::open(&path).expect("independent writer fixture");
    let lock = external_writer
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .expect("hold write lock");
    let error = database
        .agent_store()
        .start_run(conversation.id, "private-request-key-value")
        .expect_err("locked database");
    drop(lock);
    assert!(matches!(
        error,
        AgentStoreError::Database {
            kind: StorageFailureKind::Busy | StorageFailureKind::Locked,
            sqlite_code: Some(_)
        }
    ));
    assert!(!format!("{error:?}").contains("private-request-key-value"));
    assert!(
        !error
            .to_string()
            .contains(&path.to_string_lossy().to_string())
    );
}

#[test]
fn persisted_tool_results_drop_untrusted_payloads() {
    let redacted = super::history::redact_result(&json!({
        "status": "succeeded",
        "code": "ok",
        "outcome": "done",
        "truncated": true,
        "bytes": 12,
        "preview": "secret",
        "output": {"content": "secret", "path": "/tmp/x"},
        "extra": "dropped",
        "$field_0": "collision"
    }));
    assert_eq!(
        redacted,
        json!({
            "status": "succeeded",
            "code": "ok",
            "outcome": "done",
            "truncated": true,
            "bytes": 12,
            "output": {}
        })
    );
}

#[test]
fn rejected_calls_persist_both_the_call_and_its_result() {
    let database = Database::open_in_memory().expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let store = database.agent_store();
    let run = store.start_run(conversation.id, "request").expect("run");
    store
        .record_unbound_tool_result(
            run.id,
            "call_a",
            "shell",
            &json!({"argv": ["ls"]}),
            ActionOutcome::Failed,
            &json!({"status": "rejected", "code": "unknown_tool"}),
        )
        .expect("result");

    let history = store.history(conversation.id).expect("history");
    let calls = history
        .iter()
        .filter(|entry| matches!(entry.content, HistoryContent::ToolCall { .. }))
        .count();
    let results = history
        .iter()
        .filter(|entry| matches!(entry.content, HistoryContent::ToolResult { .. }))
        .count();
    assert_eq!((calls, results), (1, 1));
}

#[test]
fn a_failed_run_keeps_a_single_failure_event() {
    let database = Database::open_in_memory().expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let store = database.agent_store();
    let run = store.start_run(conversation.id, "request").expect("run");
    store.fail_run(run.id, Some("boom")).expect("failure");
    store
        .fail_run(run.id, Some("later"))
        .expect("repeated failure");

    let failures = store
        .events(conversation.id)
        .expect("events")
        .into_iter()
        .filter(|entry| {
            matches!(
                entry.content,
                HistoryContent::StateEvent {
                    event: AgentEventState::RunFailed,
                    ..
                }
            )
        })
        .count();
    assert_eq!(failures, 1);
    assert_eq!(store.run(run.id).expect("run").state, RunState::Failed);
}

#[test]
fn a_context_budget_manifest_is_persisted_and_validated() {
    let database = Database::open_in_memory().expect("database");
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let store = database.agent_store();
    let revision = store
        .authorization_revision(conversation.id)
        .expect("revision");
    let manifest = json!({
        "capacity_tokens": 8192,
        "capacity_origin": "conservative",
        "sources": [],
        "omissions": [],
    })
    .to_string();

    let record = store
        .record_context_budget(
            conversation.id,
            revision.key().context_revision,
            None,
            "context_budget.message.1",
            &manifest,
        )
        .expect("budget");
    assert!(matches!(record.fact, ContextFact::ContextBudget { .. }));
    assert!(
        store
            .facts(conversation.id)
            .expect("facts")
            .iter()
            .any(|fact| matches!(fact.fact, ContextFact::ContextBudget { .. }))
    );

    assert!(matches!(
        store.record_context_budget(
            conversation.id,
            revision.key().context_revision,
            None,
            "context_budget.message.2",
            "not-json",
        ),
        Err(AgentStoreError::InvalidInput)
    ));
}
