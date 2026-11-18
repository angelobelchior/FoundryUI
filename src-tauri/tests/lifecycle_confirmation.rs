use std::sync::Arc;

use foundryui_lib::foundry::AppState;
use foundryui_lib::foundry::attachments::{StagedAttachment, StagedFile};
use foundryui_lib::foundry::db::agent::ProjectRemovalMode;
use foundryui_lib::foundry::db::{Attachment, Database, MessageRole, MessageStatus};
use foundryui_lib::foundry::lifecycle_commands::commit_idle_context_cycle;
use foundryui_lib::foundry::project_commands::ProjectCommandError;
use foundryui_lib::foundry::resources::RootHandle;

#[test]
fn confirmed_cycle_waits_for_generation_guard_even_after_cancellation() {
    let database = Arc::new(Database::open_in_memory().expect("database"));
    let state = AppState::new();
    state.install_database(database.clone());
    let conversation = database
        .agent_store()
        .create_project_conversation(None, "fixture-model")
        .expect("conversation");
    let id = conversation.conversation.id;
    let attachment = attach(&database, id, &staged_copy());
    let plan = database
        .agent_store()
        .prepare_conversation_deletion("confirmed-delete", id)
        .expect("captured plan");
    let generation = state.generations.begin(id).expect("generation");

    assert!(matches!(
        commit_idle_context_cycle(&state, &plan),
        Err(ProjectCommandError::Busy)
    ));
    assert!(state.generations.cancel(&generation.request_id));
    assert!(matches!(
        commit_idle_context_cycle(&state, &plan),
        Err(ProjectCommandError::Busy)
    ));
    assert!(database.conversation(id).is_ok());
    assert_eq!(
        database
            .attachment_data(attachment.id)
            .expect("retained payload"),
        "data:image/png;base64,Y29weQ=="
    );

    drop(generation);
    let result = commit_idle_context_cycle(&state, &plan).expect("committed exact plan");
    assert_eq!(result.deleted_conversation_ids, vec![id]);
    assert!(result.contexts.is_empty());
    assert_eq!(result.removed_project_id, None);
    assert!(database.conversation(id).is_err());
    assert!(database.attachment(attachment.id).is_err());
    assert!(database.attachment_data(attachment.id).is_err());
    let json = serde_json::to_value(result).expect("IPC view");
    assert_eq!(json["deletedConversationIds"], serde_json::json!([id]));
    assert!(json.get("deleted_conversation_ids").is_none());
    assert!(json.get("attachmentPaths").is_none());
    assert!(json.get("data").is_none());
}

#[test]
fn confirmed_project_deletion_removes_owned_payloads_but_retains_independent_copies_and_snapshots()
{
    let directory = tempfile::tempdir().expect("project folder");
    let source_file = directory.path().join("source.png");
    std::fs::write(&source_file, "external project source").expect("source file");
    let database = Arc::new(Database::open_in_memory().expect("database"));
    let state = AppState::new();
    state.install_database(database.clone());
    let store = database.agent_store();
    let project = store
        .create_project(
            &RootHandle::enroll(directory.path()).expect("project root"),
            Some("Project"),
        )
        .expect("project");
    let source = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .expect("source")
        .conversation
        .id;
    let survivor = store
        .create_project_conversation(None, "fixture-model")
        .expect("survivor")
        .conversation
        .id;
    let staged = staged_copy();
    let owned = attach(&database, source, &staged);
    let independent = attach(&database, survivor, &staged);
    assert_ne!(owned.id, independent.id);
    assert_ne!(owned.path, independent.path);
    let snapshot = store.capture_snapshot(source, survivor).expect("snapshot");
    let plan = store
        .prepare_project_removal("delete-project", project.id, ProjectRemovalMode::Delete)
        .expect("captured deletion");

    let result =
        commit_idle_context_cycle(&state, &plan).expect("committed without attachment store");

    assert_eq!(result.removed_project_id, Some(project.id));
    assert_eq!(result.deleted_conversation_ids, vec![source]);
    assert!(database.attachment(owned.id).is_err());
    assert!(database.attachment_data(owned.id).is_err());
    assert_eq!(
        database
            .attachment_data(independent.id)
            .expect("independent payload"),
        "data:image/png;base64,Y29weQ=="
    );
    assert_eq!(
        database
            .attachment(independent.id)
            .expect("independent metadata")
            .path,
        format!("attachment:{}", independent.id)
    );
    assert_eq!(
        std::fs::read_to_string(source_file).expect("source"),
        "external project source"
    );
    assert!(directory.path().is_dir());
    assert!(store.project(project.id).is_err());
    assert!(database.conversation(survivor).is_ok());
    assert_eq!(
        store
            .snapshot(survivor, snapshot.id)
            .expect("retained snapshot"),
        snapshot
    );
}

#[test]
fn replayed_commit_never_deletes_another_rows_independent_payload() {
    let database = Arc::new(Database::open_in_memory().expect("database"));
    let state = AppState::new();
    state.install_database(database.clone());
    let source = database
        .agent_store()
        .create_project_conversation(None, "fixture-model")
        .expect("source")
        .conversation
        .id;
    let survivor = database
        .agent_store()
        .create_project_conversation(None, "fixture-model")
        .expect("survivor")
        .conversation
        .id;
    let staged = staged_copy();
    let owned = attach(&database, source, &staged);
    let plan = database
        .agent_store()
        .prepare_conversation_deletion("replayed-deletion", source)
        .expect("plan");

    commit_idle_context_cycle(&state, &plan).expect("initial commit");
    assert!(database.attachment_data(owned.id).is_err());
    let independent = attach(&database, survivor, &staged);
    assert_ne!(owned.id, independent.id);

    let result = commit_idle_context_cycle(&state, &plan).expect("idempotent replay");

    assert_eq!(result.deleted_conversation_ids, vec![source]);
    assert!(database.attachment_data(owned.id).is_err());
    assert_eq!(
        database
            .attachment_data(independent.id)
            .expect("independent payload after replay"),
        "data:image/png;base64,Y29weQ=="
    );
    assert!(database.conversation(survivor).is_ok());
}

#[test]
fn path_shaped_attachment_tokens_never_read_or_delete_external_source_files() {
    let directory = tempfile::tempdir().expect("source directory");
    let source_file = directory.path().join("att-103-1.png");
    std::fs::write(&source_file, "external content, not copied bytes").expect("source file");
    let database = Arc::new(Database::open_in_memory().expect("database"));
    let state = AppState::new();
    state.install_database(database.clone());
    let conversation = database
        .agent_store()
        .create_project_conversation(None, "fixture-model")
        .expect("conversation")
        .conversation
        .id;
    let mut staged = staged_copy();
    staged.meta.token = source_file.to_str().expect("source path").to_string();
    let attachment = attach(&database, conversation, &staged);
    assert_eq!(attachment.path, format!("attachment:{}", attachment.id));
    assert_eq!(
        database
            .attachment_data(attachment.id)
            .expect("explicit staged bytes only"),
        "data:image/png;base64,Y29weQ=="
    );
    let plan = database
        .agent_store()
        .prepare_conversation_deletion("path-shaped-token", conversation)
        .expect("plan");

    commit_idle_context_cycle(&state, &plan).expect("committed");
    commit_idle_context_cycle(&state, &plan).expect("replayed");

    assert!(database.attachment_data(attachment.id).is_err());
    assert_eq!(
        std::fs::read_to_string(source_file).expect("untouched source file"),
        "external content, not copied bytes"
    );
}

#[test]
fn confirmed_project_plan_rejects_new_conversations_instead_of_expanding_its_targets() {
    let directory = tempfile::tempdir().expect("project folder");
    let database = Arc::new(Database::open_in_memory().expect("database"));
    let state = AppState::new();
    state.install_database(database.clone());
    let store = database.agent_store();
    let project = store
        .create_project(
            &RootHandle::enroll(directory.path()).expect("root"),
            Some("Project"),
        )
        .expect("project");
    let original = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .expect("original")
        .conversation
        .id;
    let owned = attach(&database, original, &staged_copy());
    let plan = store
        .prepare_project_removal("exact-roster", project.id, ProjectRemovalMode::Delete)
        .expect("captured plan");
    let newcomer = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .expect("newcomer")
        .conversation
        .id;

    assert!(matches!(
        commit_idle_context_cycle(&state, &plan),
        Err(ProjectCommandError::StaleContext)
    ));
    assert!(database.conversation(original).is_ok());
    assert!(database.conversation(newcomer).is_ok());
    assert!(store.project(project.id).is_ok());
    assert_eq!(
        database
            .attachment_data(owned.id)
            .expect("payload retained on rejected commit"),
        "data:image/png;base64,Y29weQ=="
    );
}

fn staged_copy() -> StagedFile {
    StagedFile {
        data: b"copy".to_vec(),
        meta: StagedAttachment {
            token: "fixture".into(),
            mime: "image/png".into(),
            bytes: 4,
            width: None,
            height: None,
        },
    }
}

fn attach(database: &Database, conversation_id: i64, staged: &StagedFile) -> Attachment {
    let message = database
        .append_message(
            conversation_id,
            MessageRole::User,
            "Attached image",
            "",
            None,
            MessageStatus::Complete,
        )
        .expect("message");
    database
        .add_attachment(message.id, "image", staged)
        .expect("attachment")
}
