use super::*;
use crate::foundry::db::{MessageRole, MessageStatus};
use crate::foundry::resources::RootHandle;
use serde_json::json;

#[test]
fn availability_uses_the_project_record_snapshot_instead_of_a_new_database_binding() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original");
    let replacement = directory.path().join("replacement");
    std::fs::create_dir(&original).unwrap();
    std::fs::create_dir(&replacement).unwrap();
    let root = RootHandle::enroll(&original).unwrap();
    let replacement_root = RootHandle::enroll(&replacement).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let original_record = store.create_project(&root, None).unwrap();
    assert_eq!(
        original_record.availability(),
        ProjectAvailability::Available
    );
    let current_record = store
        .relocate_project(original_record.id, &replacement_root)
        .unwrap();
    std::fs::remove_dir(&original).unwrap();
    assert_eq!(original_record.availability(), ProjectAvailability::Missing);
    assert_eq!(
        current_record.availability(),
        ProjectAvailability::Available
    );
    assert_eq!(
        store.project_availability(original_record.id).unwrap(),
        ProjectAvailability::Available
    );
    assert_eq!(store.project(original_record.id).unwrap(), current_record);
}

#[test]
fn picker_binding_is_stale_after_relocation_away_and_back_to_the_same_root() {
    let original = tempfile::tempdir().unwrap();
    let replacement = tempfile::tempdir().unwrap();
    let root = RootHandle::enroll(original.path()).unwrap();
    let replacement_root = RootHandle::enroll(replacement.path()).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let expected = store.create_project(&root, None).unwrap();
    assert_eq!(expected.binding_revision, 1);
    let created = store
        .create_project_conversation(Some(expected.id), "fixture-model")
        .unwrap();
    store
        .relocate_project(expected.id, &replacement_root)
        .unwrap();
    let returned = store.relocate_project(expected.id, &root).unwrap();
    assert_eq!(returned.root_identity_key, expected.root_identity_key);
    assert_eq!(returned.display_path, expected.display_path);
    assert_eq!(returned.binding_revision, 3);
    assert_eq!(
        store.relocate_project_if_current(&expected, &replacement_root),
        Err(AgentStoreError::StaleContext)
    );
    assert_eq!(store.project(expected.id).unwrap(), returned);
    assert_eq!(store.context(created.conversation.id).unwrap().revision, 3);
}

#[test]
fn relocation_refuses_linked_streaming_chat_but_not_unrelated_streams() {
    let original = tempfile::tempdir().unwrap();
    let replacement = tempfile::tempdir().unwrap();
    let root = RootHandle::enroll(original.path()).unwrap();
    let replacement_root = RootHandle::enroll(replacement.path()).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store.create_project(&root, None).unwrap();
    let first = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .unwrap();
    let second = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .unwrap();
    let standalone = store
        .create_project_conversation(None, "fixture-model")
        .unwrap();
    let streaming = database
        .append_message(
            second.conversation.id,
            MessageRole::Assistant,
            "Partial",
            "",
            None,
            MessageStatus::Streaming,
        )
        .unwrap();
    database
        .append_message(
            standalone.conversation.id,
            MessageRole::Assistant,
            "Unrelated",
            "",
            None,
            MessageStatus::Streaming,
        )
        .unwrap();
    assert_eq!(
        store.relocate_project_if_current(&project, &replacement_root),
        Err(AgentStoreError::ConversationBusy)
    );
    assert_eq!(store.project(project.id).unwrap(), project);
    assert_eq!(store.context(first.conversation.id).unwrap(), first.context);
    assert_eq!(
        store.context(second.conversation.id).unwrap(),
        second.context
    );
    database
        .finish_message(
            streaming.id,
            MessageStatus::Complete,
            "Finished",
            "",
            None,
            None,
        )
        .unwrap();
    assert!(
        store
            .relocate_project_if_current(&project, &replacement_root)
            .is_ok()
    );
    assert_eq!(store.context(first.conversation.id).unwrap().revision, 2);
    assert_eq!(store.context(second.conversation.id).unwrap().revision, 2);
}

#[test]
fn conversation_listing_is_scoped_by_optional_project_and_archive_state() {
    let first_dir = tempfile::tempdir().unwrap();
    let second_dir = tempfile::tempdir().unwrap();
    let first_root = RootHandle::enroll(first_dir.path()).unwrap();
    let second_root = RootHandle::enroll(second_dir.path()).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let first_project = store.create_project(&first_root, None).unwrap();
    let second_project = store.create_project(&second_root, None).unwrap();
    let first = store
        .create_project_conversation(Some(first_project.id), "first-model")
        .unwrap();
    let archived = store
        .create_project_conversation(Some(first_project.id), "archived-model")
        .unwrap();
    let second = store
        .create_project_conversation(Some(second_project.id), "second-model")
        .unwrap();
    let standalone = store
        .create_project_conversation(None, "standalone-model")
        .unwrap();
    let legacy = database.create_conversation("legacy-model").unwrap();
    database
        .append_message(
            first.conversation.id,
            MessageRole::User,
            "Only first project",
            "",
            None,
            MessageStatus::Complete,
        )
        .unwrap();
    database
        .set_conversation_archived(archived.conversation.id, true)
        .unwrap();
    let listed = store
        .list_project_conversations(Some(first_project.id), false)
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].conversation.id, first.conversation.id);
    assert_eq!(listed[0].context, first.context);
    assert_eq!(store.history(listed[0].conversation.id).unwrap().len(), 1);
    let listed = store
        .list_project_conversations(Some(first_project.id), true)
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].conversation.id, archived.conversation.id);
    assert!(listed[0].conversation.archived);
    let listed = store
        .list_project_conversations(Some(second_project.id), false)
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].conversation.id, second.conversation.id);
    assert!(store.history(listed[0].conversation.id).unwrap().is_empty());
    let mut listed = store.list_project_conversations(None, false).unwrap();
    listed.sort_by_key(|created| created.conversation.id);
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].conversation.id, standalone.conversation.id);
    assert_eq!(listed[1].conversation.id, legacy.id);
    for created in listed {
        assert_eq!(created.context.project_id, None);
        assert_eq!(created.context.mode, AgentMode::Review);
        assert!(store.history(created.conversation.id).unwrap().is_empty());
    }
    assert!(
        store
            .list_project_conversations(None, true)
            .unwrap()
            .is_empty()
    );
    assert_eq!(database.list_conversations(false).unwrap().len(), 4);
    assert!(matches!(
        store.list_project_conversations(Some(-1), false),
        Err(AgentStoreError::NotFound)
    ));
    std::fs::rename(first_dir.path(), second_dir.path().join("moved-first")).unwrap();
    assert_eq!(
        store
            .list_project_conversations(Some(first_project.id), false)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn selected_equivalent_roots_share_one_project_without_renaming_it() {
    let directory = tempfile::tempdir().unwrap();
    let root = RootHandle::enroll(directory.path()).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store.create_project(&root, Some("  My project  ")).unwrap();
    let equivalent = RootHandle::enroll(&directory.path().join(".")).unwrap();
    assert_eq!(
        store.create_project(&equivalent, Some("Ignored")).unwrap(),
        project
    );
    assert_eq!(project.name, "My project");
    assert_eq!(store.project(project.id).unwrap(), project);
    assert_eq!(store.list_projects().unwrap(), vec![project]);
}

#[test]
fn locating_the_same_moved_folder_updates_its_path_and_invalidates_old_picker_binding() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original");
    let moved = directory.path().join("moved");
    std::fs::create_dir(&original).unwrap();
    let root = RootHandle::enroll(&original).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let expected = store.create_project(&root, None).unwrap();
    let created = store
        .create_project_conversation(Some(expected.id), "fixture-model")
        .unwrap();
    std::fs::rename(&original, &moved).unwrap();
    let moved_root = RootHandle::enroll(&moved).unwrap();
    let relocated = store
        .relocate_project_if_current(&expected, &moved_root)
        .unwrap();
    assert_eq!(relocated.root_identity_key, expected.root_identity_key);
    assert_ne!(relocated.display_path, expected.display_path);
    assert_eq!(store.context(created.conversation.id).unwrap().revision, 2);
    assert_eq!(
        store.relocate_project_if_current(&expected, &moved_root),
        Err(AgentStoreError::StaleContext)
    );
    assert_eq!(
        store.project_availability(expected.id).unwrap(),
        ProjectAvailability::Available
    );
}

#[test]
fn relocation_rejects_unrepresentable_selected_paths_without_mutating_contexts() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("control\npath");
    std::fs::create_dir(&path).unwrap();
    let unsupported = RootHandle::enroll(&path).unwrap();
    let root = RootHandle::enroll(directory.path()).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store.create_project(&root, None).unwrap();
    let created = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .unwrap();
    assert_eq!(
        store.relocate_project(project.id, &unsupported),
        Err(AgentStoreError::InvalidInput)
    );
    assert_eq!(store.project(project.id).unwrap(), project);
    assert_eq!(
        store.context(created.conversation.id).unwrap(),
        created.context
    );
}

#[test]
fn denied_roots_have_explicit_availability_and_keep_project_records_readable() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    std::fs::create_dir(&path).unwrap();
    let root = RootHandle::enroll(&path).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store.create_project(&root, None).unwrap();
    let permissions = std::fs::metadata(&path).unwrap().permissions();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0)).unwrap();
    let availability = store.project_availability(project.id);
    let blocked = store.open_project_root(project.id).is_err();
    let readable = store.project(project.id);
    std::fs::set_permissions(&path, permissions).unwrap();
    assert_eq!(availability.unwrap(), ProjectAvailability::Denied);
    assert!(blocked);
    assert_eq!(readable.unwrap(), project);
    assert_eq!(
        serde_json::to_string(&ProjectAvailability::Denied).unwrap(),
        "\"denied\""
    );
}

#[test]
fn revoked_or_changed_selection_cannot_create_or_replace_a_project_binding() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("selected");
    std::fs::create_dir(&path).unwrap();
    let selected = RootHandle::enroll(&path).unwrap();
    let root = RootHandle::enroll(directory.path()).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store.create_project(&root, None).unwrap();
    std::fs::rename(&path, directory.path().join("moved")).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(
        store.create_project(&selected, None),
        Err(AgentStoreError::InvalidInput)
    );
    assert_eq!(
        store.relocate_project(project.id, &selected),
        Err(AgentStoreError::InvalidInput)
    );
    let current = RootHandle::enroll(&path).unwrap();
    current.revoke();
    assert_eq!(
        store.create_project(&current, None),
        Err(AgentStoreError::InvalidInput)
    );
    assert_eq!(
        store.relocate_project(project.id, &current),
        Err(AgentStoreError::InvalidInput)
    );
    assert_eq!(store.list_projects().unwrap(), vec![project]);
}

#[test]
fn relocation_refuses_uncertain_claimed_work_after_database_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let original = directory.path().join("original");
    let replacement = directory.path().join("replacement");
    std::fs::create_dir(&original).unwrap();
    std::fs::create_dir(&replacement).unwrap();
    let root = RootHandle::enroll(&original).unwrap();
    let replacement_root = RootHandle::enroll(&replacement).unwrap();
    let path = directory.path().join("store.sqlite");
    let database = Database::open(&path).unwrap();
    let store = database.agent_store();
    let project = store.create_project(&root, None).unwrap();
    let created = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .unwrap();
    let run = store
        .start_run(created.conversation.id, "uncertain")
        .unwrap();
    let arguments = json!({"path":"fixture"});
    let call = store
        .propose_call(run.id, "read", "read_file", &arguments)
        .unwrap();
    store
        .decide_call(&call, &arguments, ApprovalDecision::Approved)
        .unwrap();
    let _claim = store.claim_call(&call, &arguments).unwrap();
    drop(database);
    let reopened = Database::open(&path).unwrap();
    let store = reopened.agent_store();
    assert_eq!(
        store.action(call.id).unwrap().state,
        ActionState::Interrupted
    );
    assert_eq!(
        store.relocate_project(project.id, &replacement_root),
        Err(AgentStoreError::ConversationBusy)
    );
    assert_eq!(store.project(project.id).unwrap(), project);
    assert_eq!(
        store.context(created.conversation.id).unwrap(),
        created.context
    );
    assert!(!store.events(created.conversation.id).unwrap().is_empty());
}

#[test]
fn delayed_picker_relocation_rejects_stale_binding_but_accepts_label_only_changes() {
    let original = tempfile::tempdir().unwrap();
    let replacement = tempfile::tempdir().unwrap();
    let root = RootHandle::enroll(original.path()).unwrap();
    let replacement_root = RootHandle::enroll(replacement.path()).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let expected = store.create_project(&root, None).unwrap();
    let created = store
        .create_project_conversation(Some(expected.id), "fixture-model")
        .unwrap();
    store.rename_project(expected.id, "New label").unwrap();
    let relocated = store
        .relocate_project_if_current(&expected, &replacement_root)
        .unwrap();
    assert_eq!(relocated.name, "New label");
    assert_eq!(
        store.relocate_project_if_current(&expected, &root),
        Err(AgentStoreError::StaleContext)
    );
    assert_eq!(store.project(expected.id).unwrap(), relocated);
    assert_eq!(store.context(created.conversation.id).unwrap().revision, 2);
}

#[test]
fn relocation_refuses_active_work_in_any_linked_conversation_atomically() {
    let original = tempfile::tempdir().unwrap();
    let replacement = tempfile::tempdir().unwrap();
    let root = RootHandle::enroll(original.path()).unwrap();
    let replacement_root = RootHandle::enroll(replacement.path()).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store.create_project(&root, None).unwrap();
    let first = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .unwrap();
    let second = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .unwrap();
    let run = store.start_run(second.conversation.id, "active").unwrap();
    assert_eq!(
        store.relocate_project(project.id, &replacement_root),
        Err(AgentStoreError::ConversationBusy)
    );
    assert_eq!(store.project(project.id).unwrap(), project);
    assert_eq!(store.context(first.conversation.id).unwrap(), first.context);
    assert_eq!(
        store.context(second.conversation.id).unwrap(),
        second.context
    );
    store.finish_run(run.id).unwrap();
    assert!(
        store
            .relocate_project(project.id, &replacement_root)
            .is_ok()
    );
}

#[test]
fn relocation_rejects_another_projects_root_without_changing_either_binding() {
    let first_dir = tempfile::tempdir().unwrap();
    let second_dir = tempfile::tempdir().unwrap();
    let first_root = RootHandle::enroll(first_dir.path()).unwrap();
    let second_root = RootHandle::enroll(second_dir.path()).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let first = store.create_project(&first_root, Some("First")).unwrap();
    let second = store.create_project(&second_root, Some("Second")).unwrap();
    let created = store
        .create_project_conversation(Some(first.id), "fixture-model")
        .unwrap();
    assert_eq!(
        store.relocate_project(first.id, &second_root),
        Err(AgentStoreError::Conflict)
    );
    assert_eq!(store.project(first.id).unwrap(), first);
    assert_eq!(store.project(second.id).unwrap(), second);
    assert_eq!(
        store.context(created.conversation.id).unwrap(),
        created.context
    );
}

#[test]
fn explicit_relocation_updates_binding_and_invalidates_only_linked_contexts() {
    let original = tempfile::tempdir().unwrap();
    let replacement = tempfile::tempdir().unwrap();
    let root = RootHandle::enroll(original.path()).unwrap();
    let replacement_root = RootHandle::enroll(replacement.path()).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store.create_project(&root, Some("Keep label")).unwrap();
    let first = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .unwrap();
    let second = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .unwrap();
    let standalone = store
        .create_project_conversation(None, "fixture-model")
        .unwrap();
    database
        .append_message(
            first.conversation.id,
            MessageRole::User,
            "Keep history",
            "",
            None,
            MessageStatus::Complete,
        )
        .unwrap();
    let unchanged = store.relocate_project(project.id, &root).unwrap();
    assert_eq!(unchanged, project);
    assert_eq!(unchanged.binding_revision, 1);
    assert_eq!(store.context(first.conversation.id).unwrap(), first.context);
    let relocated = store
        .relocate_project(project.id, &replacement_root)
        .unwrap();
    assert_eq!(relocated.name, "Keep label");
    assert_eq!(
        relocated.root_identity_key,
        replacement_root.identity().unwrap().storage_key()
    );
    assert_eq!(
        relocated.display_path,
        replacement.path().canonicalize().unwrap().to_str().unwrap()
    );
    for created in [&first, &second] {
        let current = store.context(created.conversation.id).unwrap();
        assert_eq!(current.revision, 2);
        assert_eq!(current.project_id, Some(project.id));
        assert_eq!(current.mode, AgentMode::Review);
        assert_eq!(
            store.advance_context(created.conversation.id, created.context.revision),
            Err(AgentStoreError::StaleContext)
        );
    }
    assert_eq!(
        store.context(standalone.conversation.id).unwrap(),
        standalone.context
    );
    assert_eq!(store.history(first.conversation.id).unwrap().len(), 1);
    assert_eq!(
        store
            .open_project_root(project.id)
            .unwrap()
            .identity()
            .unwrap(),
        replacement_root.identity().unwrap()
    );
    assert!(original.path().is_dir());
}

#[test]
fn missing_or_replaced_roots_block_access_but_keep_internal_history_queryable() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("project");
    let moved = directory.path().join("moved");
    std::fs::create_dir(&path).unwrap();
    let root = RootHandle::enroll(&path).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store.create_project(&root, None).unwrap();
    let created = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .unwrap();
    database
        .append_message(
            created.conversation.id,
            MessageRole::User,
            "Retained history",
            "",
            None,
            MessageStatus::Complete,
        )
        .unwrap();
    let source = store
        .create_project_conversation(None, "fixture-model")
        .unwrap();
    database
        .append_message(
            source.conversation.id,
            MessageRole::User,
            "Attached reference",
            "",
            None,
            MessageStatus::Complete,
        )
        .unwrap();
    let snapshot = store
        .capture_snapshot(source.conversation.id, created.conversation.id)
        .unwrap();
    assert_eq!(
        store.project_availability(project.id).unwrap(),
        ProjectAvailability::Available
    );
    assert_eq!(
        store
            .open_project_root(project.id)
            .unwrap()
            .identity()
            .unwrap(),
        root.identity().unwrap()
    );
    std::fs::rename(&path, &moved).unwrap();
    assert_eq!(
        store.project_availability(project.id).unwrap(),
        ProjectAvailability::Missing
    );
    assert!(store.open_project_root(project.id).is_err());
    std::fs::create_dir(&path).unwrap();
    assert_eq!(
        store.project_availability(project.id).unwrap(),
        ProjectAvailability::Changed
    );
    assert!(matches!(
        store.open_project_root(project.id),
        Err(AgentStoreError::StaleContext)
    ));
    assert_eq!(store.project(project.id).unwrap(), project);
    assert_eq!(store.list_projects().unwrap(), vec![project]);
    assert_eq!(store.history(created.conversation.id).unwrap().len(), 1);
    assert_eq!(
        store
            .snapshot(created.conversation.id, snapshot.id)
            .unwrap(),
        snapshot
    );
    assert_eq!(
        store.context(created.conversation.id).unwrap(),
        created.context
    );
    assert!(
        store
            .create_project_conversation(created.context.project_id, "fixture-model")
            .is_ok()
    );
    assert_eq!(
        store.project_availability(-1),
        Err(AgentStoreError::NotFound)
    );
}

#[test]
fn optional_project_conversations_start_in_review_with_independent_histories() {
    let directory = tempfile::tempdir().unwrap();
    let root = RootHandle::enroll(directory.path()).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store.create_project(&root, None).unwrap();
    let first = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .unwrap();
    let second = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .unwrap();
    let standalone = store
        .create_project_conversation(None, "other-model")
        .unwrap();
    assert_ne!(first.conversation.id, second.conversation.id);
    for created in [&first, &second, &standalone] {
        assert_eq!(created.context.mode, AgentMode::Review);
        assert_eq!(created.context.revision, 1);
        assert_eq!(created.context.conversation_id, created.conversation.id);
        assert_eq!(
            store.context(created.conversation.id).unwrap(),
            created.context
        );
    }
    assert_eq!(first.context.project_id, Some(project.id));
    assert_eq!(second.context.project_id, Some(project.id));
    assert_eq!(standalone.context.project_id, None);
    assert_eq!(standalone.conversation.model_alias, "other-model");
    database
        .append_message(
            first.conversation.id,
            MessageRole::User,
            "Only first",
            "",
            None,
            MessageStatus::Complete,
        )
        .unwrap();
    assert_eq!(store.history(first.conversation.id).unwrap().len(), 1);
    assert!(store.history(second.conversation.id).unwrap().is_empty());
    assert!(
        store
            .history(standalone.conversation.id)
            .unwrap()
            .is_empty()
    );
    let count = database.list_conversations(false).unwrap().len();
    assert!(matches!(
        store.create_project_conversation(Some(-1), "fixture-model"),
        Err(AgentStoreError::NotFound)
    ));
    assert_eq!(database.list_conversations(false).unwrap().len(), count);
}

#[test]
fn project_names_default_to_the_folder_and_rename_only_changes_the_label() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("source-folder");
    std::fs::create_dir(&path).unwrap();
    let root = RootHandle::enroll(&path).unwrap();
    let database = Database::open_in_memory().unwrap();
    let store = database.agent_store();
    let project = store.create_project(&root, None).unwrap();
    assert_eq!(project.name, "source-folder");
    let renamed = store
        .rename_project(project.id, "  Friendly label  ")
        .unwrap();
    assert_eq!(renamed.name, "Friendly label");
    assert_eq!(renamed.binding_revision, project.binding_revision);
    assert_eq!(renamed.display_path, project.display_path);
    assert_eq!(renamed.root_identity_key, project.root_identity_key);
    assert!(path.is_dir());
    assert!(!directory.path().join("Friendly label").exists());
    assert_eq!(store.project(project.id).unwrap(), renamed);
    assert_eq!(
        store.rename_project(project.id, "\n"),
        Err(AgentStoreError::InvalidInput)
    );
    assert_eq!(
        store.rename_project(project.id, "bad\nname"),
        Err(AgentStoreError::InvalidInput)
    );
    assert_eq!(
        store.rename_project(project.id, &"x".repeat(129)),
        Err(AgentStoreError::InvalidInput)
    );
    assert_eq!(
        store.rename_project(-1, "Valid"),
        Err(AgentStoreError::NotFound)
    );
}
