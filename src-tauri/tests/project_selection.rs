#![cfg(target_os = "macos")]

use foundryui_lib::foundry::db::Database;
use foundryui_lib::foundry::db::agent::AgentStoreError;
use foundryui_lib::foundry::project_commands::ProjectCommandError;
use foundryui_lib::foundry::project_commands::selected_project_root;
use foundryui_lib::foundry::project_commands::{
    ProjectContextView, ProjectConversationView, ProjectView,
};
use foundryui_lib::foundry::resources::RootHandle;
use tauri_plugin_dialog::FilePath;

#[test]
fn cancelled_native_selection_produces_no_root() {
    assert!(
        selected_project_root(None)
            .expect("cancelled selection")
            .is_none()
    );
}

#[test]
fn native_selection_validates_local_absolute_paths_without_silently_discarding_uri_fields() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let uri = reqwest::Url::from_directory_path(folder.path()).expect("file URI");
    let mut query = uri.clone();
    query.set_query(Some("token=fixture-secret"));
    let mut fragment = uri.clone();
    fragment.set_fragment(Some("fixture-secret"));
    for selection in [
        FilePath::Path(".".into()),
        FilePath::Url(query),
        FilePath::Url(fragment),
        FilePath::Url(
            reqwest::Url::parse("https://example.invalid/?token=fixture-secret").expect("URL"),
        ),
    ] {
        let error = selected_project_root(Some(selection))
            .err()
            .expect("invalid selection");
        assert!(!format!("{error:?}").contains("fixture-secret"));
    }
    let root = selected_project_root(Some(FilePath::Url(uri)))
        .expect("local selection")
        .expect("root");
    assert_eq!(
        root.display_path(),
        std::fs::canonicalize(folder.path()).expect("canonical path")
    );
}

#[test]
fn conversation_ipc_projection_matches_the_camel_case_client_contract() {
    let database = Database::open_in_memory().expect("database");
    let standalone = database
        .agent_store()
        .create_project_conversation(None, "fixture-model")
        .expect("conversation");
    let id = standalone.conversation.id;
    let view: ProjectConversationView = standalone.into();
    let value = serde_json::to_value(view).expect("IPC JSON");
    assert_eq!(value["context"]["conversationId"], id);
    assert!(value["context"]["projectId"].is_null());
    assert_eq!(value["context"]["revision"], 1);
    assert_eq!(value["context"]["mode"], "review");
    assert!(value["context"].get("conversation_id").is_none());
    let context = ProjectContextView::from(database.agent_store().context(id).expect("context"));
    assert_eq!(
        serde_json::to_value(context).expect("JSON"),
        value["context"]
    );
}

#[test]
fn project_ipc_errors_preserve_machine_readable_domain_codes() {
    for (error, expected) in [
        (AgentStoreError::InvalidInput, "ProjectInvalidInput"),
        (AgentStoreError::NotFound, "ProjectNotFound"),
        (AgentStoreError::Conflict, "ProjectConflict"),
        (AgentStoreError::StaleContext, "ProjectStaleContext"),
        (AgentStoreError::ConversationBusy, "ProjectBusy"),
        (AgentStoreError::Storage, "ProjectStorage"),
    ] {
        let value = serde_json::to_value(ProjectCommandError::from(error)).expect("error JSON");
        assert_eq!(value["code"], expected);
        assert_eq!(value["messageKey"], format!("errors.{expected}"));
        assert!(value["message"].is_string());
    }
}

#[test]
fn project_ipc_availability_and_path_belong_to_the_same_binding_snapshot() {
    let folder = tempfile::tempdir().expect("temporary directory");
    let first = folder.path().join("first");
    let second = folder.path().join("second");
    std::fs::create_dir(&first).expect("first");
    std::fs::create_dir(&second).expect("second");
    let database = Database::open_in_memory().expect("database");
    let old = database
        .agent_store()
        .create_project(&RootHandle::enroll(&first).expect("first root"), None)
        .expect("project");
    let updated = database
        .agent_store()
        .relocate_project(old.id, &RootHandle::enroll(&second).expect("second root"))
        .expect("relocate");
    std::fs::remove_dir(&first).expect("remove old root");
    let old_path = old.display_path.clone();
    let old_view: ProjectView = old.into();
    assert_eq!(old_view.folder_path, old_path);
    assert_eq!(old_view.binding_revision, 1);
    assert_eq!(
        serde_json::to_value(old_view).expect("JSON")["availability"],
        "missing"
    );
    let current: ProjectView = updated.into();
    assert_eq!(current.binding_revision, 2);
    assert_eq!(
        serde_json::to_value(current).expect("JSON")["availability"],
        "available"
    );
}
