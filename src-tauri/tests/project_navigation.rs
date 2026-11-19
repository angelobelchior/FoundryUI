#![cfg(target_os = "macos")]

use foundryui_lib::foundry::AppState;
use foundryui_lib::foundry::db::Database;
use foundryui_lib::foundry::project_commands::{
    ProjectCommandError, register_selected_project, relocate_selected_project,
};
use foundryui_lib::foundry::resources::RootHandle;
use std::sync::Arc;

#[test]
fn native_project_selection_reports_creation_or_existing_identity_without_renaming_it() {
    let database = Database::open_in_memory().expect("database");
    let directory = tempfile::tempdir().expect("selected folder");
    let root = RootHandle::enroll(directory.path()).expect("selected root");
    let created = register_selected_project(&database, &root, Some("Workspace")).expect("create");
    let existing =
        register_selected_project(&database, &root, Some("Other label")).expect("existing");

    assert!(created.created);
    assert!(!existing.created);
    assert_eq!(created.project.id, existing.project.id);
    assert_eq!(existing.project.name, "Workspace");
    let payload = serde_json::to_value(existing).expect("IPC JSON");
    assert_eq!(payload["created"], false);
    assert_eq!(payload["name"], "Workspace");
    assert!(payload.get("project").is_none());
    assert!(payload.get("rootIdentityKey").is_none());
}

#[test]
fn native_relocation_refuses_a_live_generation_before_legacy_streaming_rows_exist() {
    let database = Arc::new(Database::open_in_memory().expect("database"));
    let original = tempfile::tempdir().expect("original folder");
    let destination = tempfile::tempdir().expect("destination folder");
    let root = RootHandle::enroll(original.path()).expect("root");
    let project = database
        .agent_store()
        .create_project(&root, Some("Workspace"))
        .expect("project");
    let conversation = database
        .agent_store()
        .create_project_conversation(Some(project.id), "fixture-model")
        .expect("conversation");
    let state = AppState::new();
    state.install_database(database.clone());
    let generation = state
        .generations
        .begin(conversation.conversation.id)
        .expect("generation");
    let selected = RootHandle::enroll(destination.path()).expect("selected root");

    assert_eq!(
        relocate_selected_project(&state, &project, &selected).expect_err("busy"),
        ProjectCommandError::Busy
    );
    assert!(state.generations.cancel(&generation.request_id));
    assert_eq!(
        relocate_selected_project(&state, &project, &selected).expect_err("still busy"),
        ProjectCommandError::Busy
    );
    assert_eq!(
        database
            .agent_store()
            .project(project.id)
            .expect("unchanged"),
        project
    );
    drop(generation);

    let relocated = relocate_selected_project(&state, &project, &selected).expect("relocated");
    assert_eq!(relocated.binding_revision, project.binding_revision + 1);
    assert_eq!(
        database
            .agent_store()
            .context(conversation.conversation.id)
            .expect("context")
            .revision,
        2
    );
    assert!(original.path().is_dir());
    assert!(destination.path().is_dir());
}
