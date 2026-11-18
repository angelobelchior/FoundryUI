use super::*;
use crate::foundry::db::{MessageRole, MessageStatus};

#[test]
fn all_three_moves_preserve_history_and_advance_context_once_per_request() {
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let first = store.record_project("First", "first", "/first").unwrap();
    let second = store.record_project("Second", "second", "/second").unwrap();
    let created = store.create_project_conversation(None, "fixture").unwrap();
    let id = created.conversation.id;
    database
        .append_message(
            id,
            MessageRole::User,
            "Source context survives",
            "",
            None,
            MessageStatus::Complete,
        )
        .unwrap();
    let attachment = attach(&database, id, b"\x89PNG\r\n\x1a\n");
    for (key, destination, revision) in [
        ("enter", Some(first.id), 2),
        ("switch", Some(second.id), 3),
        ("leave", None, 4),
    ] {
        let plan = store
            .prepare_conversation_move(key, id, destination)
            .unwrap();
        assert_eq!(plan.conversations()[0].revision, revision - 1);
        let result = store.commit_context_cycle(&plan).unwrap();
        assert_eq!(result.contexts()[0].revision, revision);
        assert_eq!(store.context(id).unwrap().project_id, destination);
        assert_eq!(store.commit_context_cycle(&plan).unwrap(), result);
        let repeated = store
            .prepare_conversation_move(key, id, destination)
            .unwrap();
        assert_eq!(store.commit_context_cycle(&repeated).unwrap(), result);
        assert_eq!(store.history(id).unwrap().len(), 2);
        assert_eq!(
            database.attachment(attachment.id).unwrap().path,
            format!("attachment:{}", attachment.id)
        );
        assert_eq!(
            database.attachment_data(attachment.id).unwrap(),
            "data:image/png;base64,iVBORw0KGgo="
        );
    }
    assert_eq!(store.context(id).unwrap().revision, 4);
}

#[test]
fn streaming_chat_blocks_prepare_commit_and_legacy_deletion_without_changing_context() {
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store.record_project("First", "first", "/first").unwrap();
    let created = store.create_project_conversation(None, "fixture").unwrap();
    let id = created.conversation.id;
    let plan = store
        .prepare_conversation_move("before-stream", id, Some(project.id))
        .unwrap();
    let message = database
        .append_message(
            id,
            MessageRole::Assistant,
            "Partial",
            "",
            None,
            MessageStatus::Streaming,
        )
        .unwrap();
    assert!(matches!(
        store.prepare_conversation_move("during-stream", id, Some(project.id)),
        Err(AgentStoreError::ConversationBusy)
    ));
    assert_eq!(
        store.commit_context_cycle(&plan),
        Err(AgentStoreError::ConversationBusy)
    );
    assert!(database.delete_conversation(id).is_err());
    assert_eq!(store.context(id).unwrap(), created.context);
    database
        .finish_message(message.id, MessageStatus::Complete, "Done", "", None, None)
        .unwrap();
    assert!(store.commit_context_cycle(&plan).is_ok());
}

#[test]
fn keeping_project_conversations_detaches_even_archived_rows_and_invalidates_facts_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let working_file = directory.path().join("work.txt");
    std::fs::write(&working_file, "Keep user work").unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store
        .record_project("Project", "first", directory.path().to_str().unwrap())
        .unwrap();
    let first = store
        .create_project_conversation(Some(project.id), "fixture")
        .unwrap();
    let second = store
        .create_project_conversation(Some(project.id), "fixture")
        .unwrap();
    let attachment = attach(&database, first.conversation.id, b"\x89PNG\r\n\x1a\n");
    database
        .set_conversation_archived(second.conversation.id, true)
        .unwrap();
    let grant = store
        .record_fact(
            first.conversation.id,
            1,
            None,
            "grant",
            &ContextFact::PermissionGrant {
                operation: "read".into(),
                constraints_fingerprint: "a".repeat(64),
            },
        )
        .unwrap();
    let plan = store
        .prepare_project_removal("remove-keep", project.id, ProjectRemovalMode::Keep)
        .unwrap();
    assert_eq!(plan.conversations().len(), 2);
    let result = store.commit_context_cycle(&plan).unwrap();
    assert_eq!(result.removed_project_id(), Some(project.id));
    assert_eq!(result.contexts().len(), 2);
    for created in [&first, &second] {
        let context = store.context(created.conversation.id).unwrap();
        assert_eq!(context.project_id, None);
        assert_eq!(context.revision, 2);
    }
    assert_eq!(
        store.active_facts(first.conversation.id, 1),
        Err(AgentStoreError::StaleContext)
    );
    assert!(
        store
            .active_facts(first.conversation.id, 2)
            .unwrap()
            .is_empty()
    );
    assert_eq!(store.fact(first.conversation.id, grant.id).unwrap(), grant);
    assert_eq!(store.project(project.id), Err(AgentStoreError::NotFound));
    assert_eq!(
        database.attachment_data(attachment.id).unwrap(),
        "data:image/png;base64,iVBORw0KGgo="
    );
    assert_eq!(
        std::fs::read_to_string(&working_file).unwrap(),
        "Keep user work"
    );
    assert_eq!(store.commit_context_cycle(&plan).unwrap(), result);
}

#[test]
fn deleting_project_conversations_removes_owned_records_but_keeps_external_snapshots_and_work_files()
 {
    let directory = tempfile::tempdir().unwrap();
    let working_file = directory.path().join("work.txt");
    std::fs::write(&working_file, "Keep user work").unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store
        .record_project("Project", "first", directory.path().to_str().unwrap())
        .unwrap();
    let source = store
        .create_project_conversation(Some(project.id), "fixture")
        .unwrap();
    let other = store.create_project_conversation(None, "fixture").unwrap();
    let owned_attachment = attach(&database, source.conversation.id, b"\x89PNG\r\n\x1a\n");
    let independent_attachment = attach(&database, other.conversation.id, b"\x89PNG\r\n\x1a\n");
    database
        .append_message(
            source.conversation.id,
            MessageRole::User,
            "Independent reference",
            "",
            None,
            MessageStatus::Complete,
        )
        .unwrap();
    let snapshot = store
        .capture_snapshot(source.conversation.id, other.conversation.id)
        .unwrap();
    let owned_snapshot = store
        .capture_snapshot(other.conversation.id, source.conversation.id)
        .unwrap();
    let fact = store
        .record_fact(
            source.conversation.id,
            1,
            None,
            "network",
            &ContextFact::NetworkEvent {
                destination: "https://example.test/".into(),
                state: NetworkEventState::Requested,
                http_status: None,
            },
        )
        .unwrap();
    let run = store.start_run(source.conversation.id, "finished").unwrap();
    store.finish_run(run.id).unwrap();
    let plan = store
        .prepare_project_removal("remove-delete", project.id, ProjectRemovalMode::Delete)
        .unwrap();
    let result = store.commit_context_cycle(&plan).unwrap();
    assert_eq!(result.deleted_conversation_ids(), &[source.conversation.id]);
    assert_eq!(
        store.context(source.conversation.id),
        Err(AgentStoreError::NotFound)
    );
    assert_eq!(
        store.fact(source.conversation.id, fact.id),
        Err(AgentStoreError::NotFound)
    );
    assert_eq!(store.run(run.id), Err(AgentStoreError::NotFound));
    assert!(store.events(source.conversation.id).unwrap().is_empty());
    assert_eq!(
        store.snapshot(source.conversation.id, owned_snapshot.id),
        Err(AgentStoreError::NotFound)
    );
    assert_eq!(
        store.snapshot(other.conversation.id, snapshot.id).unwrap(),
        snapshot
    );
    assert_eq!(store.context(other.conversation.id).unwrap(), other.context);
    assert_eq!(store.project(project.id), Err(AgentStoreError::NotFound));
    assert!(database.attachment(owned_attachment.id).is_err());
    assert!(database.attachment_data(owned_attachment.id).is_err());
    assert_eq!(
        database.attachment_data(independent_attachment.id).unwrap(),
        "data:image/png;base64,iVBORw0KGgo="
    );
    assert_eq!(
        std::fs::read_to_string(&working_file).unwrap(),
        "Keep user work"
    );
    assert_eq!(store.commit_context_cycle(&plan).unwrap(), result);
}

fn attach(
    database: &Database,
    conversation_id: i64,
    data: &[u8],
) -> crate::foundry::db::Attachment {
    let message = database
        .append_message(
            conversation_id,
            MessageRole::User,
            "Image",
            "",
            None,
            MessageStatus::Complete,
        )
        .unwrap();
    database
        .add_attachment(
            message.id,
            "image",
            &crate::foundry::attachments::StagedFile {
                data: data.to_vec(),
                meta: crate::foundry::attachments::StagedAttachment {
                    token: "fixture".into(),
                    mime: "image/png".into(),
                    bytes: data.len() as u64,
                    width: None,
                    height: None,
                },
            },
        )
        .unwrap()
}

#[test]
fn conversation_deletion_removes_owned_payloads_but_preserves_other_copies_snapshots_and_source_files()
 {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("source.png");
    std::fs::write(&file, b"\x89PNG\r\n\x1a\n").unwrap();
    let copied = std::fs::read(&file).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let source = store.create_project_conversation(None, "fixture").unwrap();
    let other = store.create_project_conversation(None, "fixture").unwrap();
    let owned = attach(&database, source.conversation.id, &copied);
    let independent = attach(&database, other.conversation.id, &copied);
    std::fs::write(&file, "Changed external source").unwrap();
    let snapshot = store
        .capture_snapshot(source.conversation.id, other.conversation.id)
        .unwrap();
    let plan = store
        .prepare_conversation_deletion("delete-one", source.conversation.id)
        .unwrap();
    let result = store.commit_context_cycle(&plan).unwrap();
    assert_eq!(result.deleted_conversation_ids(), &[source.conversation.id]);
    assert!(database.attachment(owned.id).is_err());
    assert!(database.attachment_data(owned.id).is_err());
    assert_eq!(
        database.attachment(independent.id).unwrap().path,
        independent.path
    );
    assert_eq!(
        database.attachment_data(independent.id).unwrap(),
        "data:image/png;base64,iVBORw0KGgo="
    );
    assert_eq!(
        store.snapshot(other.conversation.id, snapshot.id).unwrap(),
        snapshot
    );
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "Changed external source"
    );
    assert_eq!(store.commit_context_cycle(&plan).unwrap(), result);
    assert!(database.attachment_data(owned.id).is_err());
    assert_eq!(
        database.attachment_data(independent.id).unwrap(),
        "data:image/png;base64,iVBORw0KGgo="
    );
}

#[test]
fn pending_project_removal_never_expands_its_roster_when_a_conversation_is_added_or_moved_in() {
    for mode in [ProjectRemovalMode::Keep, ProjectRemovalMode::Delete] {
        for move_existing in [false, true] {
            let database = Database::open_in_memory().unwrap();
            let store = database.agent_store();
            let project = store
                .record_project("Project", "first", "/project")
                .unwrap();
            let original = store
                .create_project_conversation(Some(project.id), "fixture")
                .unwrap();
            let plan = store
                .prepare_project_removal("remove", project.id, mode)
                .unwrap();
            let newcomer = if move_existing {
                let standalone = store.create_project_conversation(None, "fixture").unwrap();
                let moving = store
                    .prepare_conversation_move("move", standalone.conversation.id, Some(project.id))
                    .unwrap();
                store.commit_context_cycle(&moving).unwrap();
                standalone.conversation.id
            } else {
                store
                    .create_project_conversation(Some(project.id), "fixture")
                    .unwrap()
                    .conversation
                    .id
            };
            let repeated = store
                .prepare_project_removal("remove", project.id, mode)
                .unwrap();
            assert_eq!(repeated.conversations(), plan.conversations());
            assert_eq!(
                store.commit_context_cycle(&plan),
                Err(AgentStoreError::StaleContext)
            );
            assert_eq!(
                store.commit_context_cycle(&repeated),
                Err(AgentStoreError::StaleContext)
            );
            assert_eq!(
                store.context(original.conversation.id).unwrap(),
                original.context
            );
            assert_eq!(
                store.context(newcomer).unwrap().project_id,
                Some(project.id)
            );
            assert_eq!(store.project(project.id).unwrap(), project);
        }
    }
}

#[test]
fn stale_conversation_plans_and_project_rosters_reject_context_changes_even_after_returning_to_origin()
 {
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store
        .record_project("Project", "first", "/project")
        .unwrap();
    let other_project = store.record_project("Other", "second", "/other").unwrap();
    let created = store
        .create_project_conversation(Some(project.id), "fixture")
        .unwrap();
    let id = created.conversation.id;
    let moving = store
        .prepare_conversation_move("pending-move", id, Some(other_project.id))
        .unwrap();
    let deletion = store
        .prepare_conversation_deletion("pending-delete", id)
        .unwrap();
    let removal = store
        .prepare_project_removal("pending-remove", project.id, ProjectRemovalMode::Delete)
        .unwrap();
    let leave = store.prepare_conversation_move("leave", id, None).unwrap();
    store.commit_context_cycle(&leave).unwrap();
    assert_eq!(
        store.commit_context_cycle(&removal),
        Err(AgentStoreError::StaleContext)
    );
    let enter = store
        .prepare_conversation_move("enter", id, Some(project.id))
        .unwrap();
    store.commit_context_cycle(&enter).unwrap();
    for plan in [&moving, &deletion, &removal] {
        assert_eq!(
            store.commit_context_cycle(plan),
            Err(AgentStoreError::StaleContext)
        );
    }
    assert_eq!(store.context(id).unwrap().revision, 3);
    assert_eq!(store.context(id).unwrap().project_id, Some(project.id));
}

#[cfg(target_os = "macos")]
#[test]
fn move_plans_bind_destination_project_revision_and_label_changes_do_not_invalidate_them() {
    use crate::foundry::resources::RootHandle;
    let original = tempfile::tempdir().unwrap();
    let replacement = tempfile::tempdir().unwrap();
    let root = RootHandle::enroll(original.path()).unwrap();
    let replacement_root = RootHandle::enroll(replacement.path()).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store.create_project(&root, None).unwrap();
    let created = store.create_project_conversation(None, "fixture").unwrap();
    let plan = store
        .prepare_conversation_move("move", created.conversation.id, Some(project.id))
        .unwrap();
    assert_eq!(plan.project_binding_revision(project.id), Some(1));
    store
        .relocate_project(project.id, &replacement_root)
        .unwrap();
    store.relocate_project(project.id, &root).unwrap();
    assert_eq!(
        store.commit_context_cycle(&plan),
        Err(AgentStoreError::StaleContext)
    );
    assert_eq!(
        store.context(created.conversation.id).unwrap(),
        created.context
    );
    let current = store
        .prepare_conversation_move("current", created.conversation.id, Some(project.id))
        .unwrap();
    store.rename_project(project.id, "New label").unwrap();
    assert!(store.commit_context_cycle(&current).is_ok());
}

#[test]
fn one_busy_conversation_blocks_project_removal_atomically_and_each_operation_rechecks_active_runs()
{
    for mode in [ProjectRemovalMode::Keep, ProjectRemovalMode::Delete] {
        let database = Database::open_in_memory().unwrap();
        let store = database.agent_store();
        let project = store
            .record_project("Project", "first", "/project")
            .unwrap();
        let first = store
            .create_project_conversation(Some(project.id), "fixture")
            .unwrap();
        let second = store
            .create_project_conversation(Some(project.id), "fixture")
            .unwrap();
        let removal = store
            .prepare_project_removal("remove", project.id, mode)
            .unwrap();
        let attachment = attach(&database, first.conversation.id, b"\x89PNG\r\n\x1a\n");
        let moving = store
            .prepare_conversation_move("move", second.conversation.id, None)
            .unwrap();
        let deletion = store
            .prepare_conversation_deletion("delete", second.conversation.id)
            .unwrap();
        let run = store.start_run(second.conversation.id, "active").unwrap();
        for plan in [&removal, &moving, &deletion] {
            assert_eq!(
                store.commit_context_cycle(plan),
                Err(AgentStoreError::ConversationBusy)
            );
        }
        assert!(matches!(
            store.prepare_project_removal("busy-remove", project.id, mode),
            Err(AgentStoreError::ConversationBusy)
        ));
        assert!(matches!(
            store.prepare_conversation_move("busy-move", second.conversation.id, None),
            Err(AgentStoreError::ConversationBusy)
        ));
        assert!(matches!(
            store.prepare_conversation_deletion("busy-delete", second.conversation.id),
            Err(AgentStoreError::ConversationBusy)
        ));
        assert_eq!(store.context(first.conversation.id).unwrap(), first.context);
        assert_eq!(
            store.context(second.conversation.id).unwrap(),
            second.context
        );
        assert_eq!(store.project(project.id).unwrap(), project);
        assert_eq!(
            database.attachment_data(attachment.id).unwrap(),
            "data:image/png;base64,iVBORw0KGgo="
        );
        store.finish_run(run.id).unwrap();
        assert!(store.commit_context_cycle(&removal).is_ok());
        if mode == ProjectRemovalMode::Keep {
            assert_eq!(
                database.attachment_data(attachment.id).unwrap(),
                "data:image/png;base64,iVBORw0KGgo="
            );
        } else {
            assert!(database.attachment_data(attachment.id).is_err());
        }
    }
}

#[test]
fn uncertain_claims_survive_restart_and_block_all_context_cycles_and_legacy_deletion() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("agent.sqlite");
    let database = Database::open(&path).unwrap();
    let store = database.agent_store();
    let project = store
        .record_project("Project", "first", "/project")
        .unwrap();
    let created = store
        .create_project_conversation(Some(project.id), "fixture")
        .unwrap();
    let id = created.conversation.id;
    let moving = store.prepare_conversation_move("move", id, None).unwrap();
    let deletion = store.prepare_conversation_deletion("delete", id).unwrap();
    let keeping = store
        .prepare_project_removal("keep", project.id, ProjectRemovalMode::Keep)
        .unwrap();
    let removing = store
        .prepare_project_removal("remove", project.id, ProjectRemovalMode::Delete)
        .unwrap();
    let run = store.start_run(id, "uncertain").unwrap();
    let arguments = serde_json::json!({"path":"file.txt"});
    let action = store
        .propose_call(run.id, "read", "read_file", &arguments)
        .unwrap();
    store
        .decide_call(&action, &arguments, ApprovalDecision::Approved)
        .unwrap();
    let _claim = store.claim_call(&action, &arguments).unwrap();
    drop(database);
    let reopened = Database::open(&path).unwrap();
    let store = reopened.agent_store();
    assert_eq!(
        store.action(action.id).unwrap().state,
        ActionState::Interrupted
    );
    for plan in [&moving, &deletion, &keeping, &removing] {
        assert_eq!(
            store.commit_context_cycle(plan),
            Err(AgentStoreError::ConversationBusy)
        );
    }
    assert!(matches!(
        store.prepare_conversation_move("new-move", id, None),
        Err(AgentStoreError::ConversationBusy)
    ));
    assert!(matches!(
        store.prepare_conversation_deletion("new-delete", id),
        Err(AgentStoreError::ConversationBusy)
    ));
    for mode in [ProjectRemovalMode::Keep, ProjectRemovalMode::Delete] {
        assert!(matches!(
            store.prepare_project_removal("new-remove", project.id, mode),
            Err(AgentStoreError::ConversationBusy)
        ));
    }
    assert!(reopened.delete_conversation(id).is_err());
    assert_eq!(store.context(id).unwrap(), created.context);
    assert_eq!(store.project(project.id).unwrap(), project);
}

#[test]
fn moving_invalidates_old_facts_and_delayed_action_approvals_but_preserves_revision_audit_history()
{
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("agent.sqlite");
    let database = Database::open(&path).unwrap();
    let store = database.agent_store();
    let project = store
        .record_project("Project", "first", "/project")
        .unwrap();
    let created = store.create_project_conversation(None, "fixture").unwrap();
    let id = created.conversation.id;
    let plan = store
        .prepare_conversation_move("move", id, Some(project.id))
        .unwrap();
    let facts = [
        ContextFact::PermissionGrant {
            operation: "read".into(),
            constraints_fingerprint: "a".repeat(64),
        },
        ContextFact::AuthorizedRoot {
            identity_key: "root:1".into(),
            display_path: "/old".into(),
        },
        ContextFact::InstructionApproval {
            identity_key: "file:1".into(),
            content_fingerprint: "b".repeat(64),
        },
        ContextFact::NetworkEvent {
            destination: "https://example.test/".into(),
            state: NetworkEventState::Requested,
            http_status: None,
        },
    ];
    for (index, fact) in facts.iter().enumerate() {
        store
            .record_fact(id, 1, None, &format!("fact-{index}"), fact)
            .unwrap();
    }
    let run = store.start_run(id, "unclaimed").unwrap();
    let arguments = serde_json::json!({"path":"file.txt"});
    let pending = store
        .propose_call(run.id, "pending", "read_file", &arguments)
        .unwrap();
    let approved = store
        .propose_call(run.id, "approved", "read_file", &arguments)
        .unwrap();
    store
        .decide_call(&approved, &arguments, ApprovalDecision::Approved)
        .unwrap();
    drop(database);
    let reopened = Database::open(&path).unwrap();
    let store = reopened.agent_store();
    let historical_events = store.events(id).unwrap();
    let historical_facts = store.facts(id).unwrap();
    store.commit_context_cycle(&plan).unwrap();
    assert_eq!(store.context(id).unwrap().revision, 2);
    assert!(store.active_facts(id, 2).unwrap().is_empty());
    assert_eq!(
        store.active_facts(id, 1),
        Err(AgentStoreError::StaleContext)
    );
    assert_eq!(store.facts(id).unwrap(), historical_facts);
    assert_eq!(store.events(id).unwrap(), historical_events);
    assert_eq!(store.run(run.id).unwrap().context_revision, 1);
    assert_eq!(
        store.decide_call(&pending, &arguments, ApprovalDecision::Approved),
        Err(AgentStoreError::StaleContext)
    );
    assert_eq!(
        store.decide_call(&approved, &arguments, ApprovalDecision::Approved),
        Err(AgentStoreError::StaleContext)
    );
    assert!(matches!(
        store.claim_call(&approved, &arguments),
        Err(AgentStoreError::StaleContext)
    ));
    assert_eq!(
        store.record_fact(id, 1, None, "late", &facts[0]),
        Err(AgentStoreError::StaleContext)
    );
}

#[test]
fn durable_move_receipts_return_the_original_result_without_reapplying_after_later_moves() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("agent.sqlite");
    let database = Database::open(&path).unwrap();
    let store = database.agent_store();
    let project = store
        .record_project("Project", "first", "/project")
        .unwrap();
    let created = store.create_project_conversation(None, "fixture").unwrap();
    let id = created.conversation.id;
    let move_plan = store
        .prepare_conversation_move("move", id, Some(project.id))
        .unwrap();
    let moved = store.commit_context_cycle(&move_plan).unwrap();
    let leave = store.prepare_conversation_move("leave", id, None).unwrap();
    store.commit_context_cycle(&leave).unwrap();
    drop(database);
    let reopened = Database::open(&path).unwrap();
    let store = reopened.agent_store();
    let repeated = store
        .prepare_conversation_move("move", id, Some(project.id))
        .unwrap();
    assert_eq!(store.commit_context_cycle(&repeated).unwrap(), moved);
    assert_eq!(store.context(id).unwrap().project_id, None);
    assert_eq!(store.context(id).unwrap().revision, 3);
    assert!(matches!(
        store.prepare_conversation_move("move", id, None),
        Err(AgentStoreError::Conflict)
    ));
    assert!(matches!(
        store.prepare_conversation_deletion("move", id),
        Err(AgentStoreError::Conflict)
    ));
}

#[test]
fn durable_removal_and_deletion_retries_do_not_target_new_conversations_or_recreated_projects() {
    for mode in [ProjectRemovalMode::Keep, ProjectRemovalMode::Delete] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("agent.sqlite");
        let database = Database::open(&path).unwrap();
        let store = database.agent_store();
        let project = store
            .record_project("Project", "first", "/project")
            .unwrap();
        let created = store
            .create_project_conversation(Some(project.id), "fixture")
            .unwrap();
        let standalone = store.create_project_conversation(None, "fixture").unwrap();
        let project_attachment = attach(&database, created.conversation.id, b"\x89PNG\r\n\x1a\n");
        let standalone_attachment =
            attach(&database, standalone.conversation.id, b"\x89PNG\r\n\x1a\n");
        let remove = store
            .prepare_project_removal("remove", project.id, mode)
            .unwrap();
        let removed = store.commit_context_cycle(&remove).unwrap();
        let delete = store
            .prepare_conversation_deletion("delete", standalone.conversation.id)
            .unwrap();
        let deleted = store.commit_context_cycle(&delete).unwrap();
        let replacement = store
            .record_project("Replacement", "first", "/project")
            .unwrap();
        let newcomer = store
            .create_project_conversation(Some(replacement.id), "fixture")
            .unwrap();
        let newcomer_attachment = attach(&database, newcomer.conversation.id, b"\x89PNG\r\n\x1a\n");
        drop(database);
        let reopened = Database::open(&path).unwrap();
        let store = reopened.agent_store();
        let remove = store
            .prepare_project_removal("remove", project.id, mode)
            .unwrap();
        let delete = store
            .prepare_conversation_deletion("delete", standalone.conversation.id)
            .unwrap();
        assert_eq!(store.commit_context_cycle(&remove).unwrap(), removed);
        assert_eq!(store.commit_context_cycle(&delete).unwrap(), deleted);
        assert!(reopened.attachment_data(standalone_attachment.id).is_err());
        assert_eq!(
            reopened.attachment_data(newcomer_attachment.id).unwrap(),
            "data:image/png;base64,iVBORw0KGgo="
        );
        assert_eq!(
            store.context(newcomer.conversation.id).unwrap(),
            newcomer.context
        );
        assert_eq!(store.project(replacement.id).unwrap(), replacement);
        assert!(matches!(
            store.prepare_project_removal("remove", replacement.id, mode),
            Err(AgentStoreError::Conflict)
        ));
        let changed_mode = if mode == ProjectRemovalMode::Keep {
            ProjectRemovalMode::Delete
        } else {
            ProjectRemovalMode::Keep
        };
        assert!(matches!(
            store.prepare_project_removal("remove", project.id, changed_mode),
            Err(AgentStoreError::Conflict)
        ));
        if mode == ProjectRemovalMode::Keep {
            assert_eq!(
                store.context(created.conversation.id).unwrap().project_id,
                None
            );
            assert_eq!(store.context(created.conversation.id).unwrap().revision, 2);
            assert_eq!(
                reopened.attachment_data(project_attachment.id).unwrap(),
                "data:image/png;base64,iVBORw0KGgo="
            );
        } else {
            assert_eq!(
                store.context(created.conversation.id),
                Err(AgentStoreError::NotFound)
            );
            assert!(reopened.attachment_data(project_attachment.id).is_err());
        }
    }
}

#[test]
fn prepared_receipts_survive_restart_without_recapturing_an_expanded_roster() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("agent.sqlite");
    let database = Database::open(&path).unwrap();
    let store = database.agent_store();
    let project = store
        .record_project("Project", "first", "/project")
        .unwrap();
    let original = store
        .create_project_conversation(Some(project.id), "fixture")
        .unwrap();
    let plan = store
        .prepare_project_removal("remove", project.id, ProjectRemovalMode::Delete)
        .unwrap();
    drop(database);
    let reopened = Database::open(&path).unwrap();
    let store = reopened.agent_store();
    let newcomer = store
        .create_project_conversation(Some(project.id), "fixture")
        .unwrap();
    let repeated = store
        .prepare_project_removal("remove", project.id, ProjectRemovalMode::Delete)
        .unwrap();
    assert_eq!(repeated.conversations(), plan.conversations());
    assert_eq!(repeated.conversations(), &[original.context.clone()]);
    assert_eq!(
        store.commit_context_cycle(&repeated),
        Err(AgentStoreError::StaleContext)
    );
    assert_eq!(
        store.context(original.conversation.id).unwrap(),
        original.context
    );
    assert_eq!(
        store.context(newcomer.conversation.id).unwrap(),
        newcomer.context
    );
}

#[test]
fn plans_cannot_be_committed_to_another_database_even_if_request_keys_and_record_ids_match() {
    let first = Database::open_in_memory().unwrap();
    let second = Database::open_in_memory().unwrap();
    let mut plans = Vec::new();
    for database in [&first, &second] {
        let store = database.agent_store();
        let project = store
            .record_project("Project", "first", "/project")
            .unwrap();
        let created = store
            .create_project_conversation(Some(project.id), "fixture")
            .unwrap();
        plans.push(
            store
                .prepare_conversation_deletion("delete", created.conversation.id)
                .unwrap(),
        );
    }
    assert_eq!(
        second.agent_store().commit_context_cycle(&plans[0]),
        Err(AgentStoreError::Conflict)
    );
    assert!(first.agent_store().commit_context_cycle(&plans[0]).is_ok());
    assert!(second.agent_store().commit_context_cycle(&plans[1]).is_ok());
}
