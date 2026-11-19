#![cfg(target_os = "macos")]

use foundryui_lib::foundry::AppState;
use foundryui_lib::foundry::db::Database;
use foundryui_lib::foundry::project_commands::ProjectCommandError;
use foundryui_lib::foundry::project_selection::{
    begin_selected_project, confirm_selected_project, discard_selected_project,
};
use foundryui_lib::foundry::resources::RootHandle;
use std::sync::Arc;

fn fixture() -> (AppState, Arc<Database>) {
    let database = Arc::new(Database::open_in_memory().unwrap());
    let state = AppState::new();
    state.install_database(database.clone());
    (state, database)
}

#[test]
fn selected_folder_is_only_persisted_after_editable_name_confirmation() {
    let (state, database) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let folder = directory.path().join("Folder default");
    std::fs::create_dir(&folder).unwrap();
    let existing_file = folder.join("keep.txt");
    std::fs::write(&existing_file, b"unchanged user fixture").unwrap();
    let root = RootHandle::enroll(&folder).unwrap();
    let preview = begin_selected_project(&state, root).unwrap();
    assert_eq!(preview.default_name, "Folder default");
    assert!(preview.existing_project.is_none());
    assert!(database.agent_store().list_projects().unwrap().is_empty());
    let payload = serde_json::to_value(&preview).unwrap();
    assert!(payload["selectionId"].is_string());
    assert_eq!(payload["defaultName"], "Folder default");
    assert!(payload["existingProject"].is_null());
    assert!(payload.get("rootIdentityKey").is_none());

    let selected = confirm_selected_project(&state, &preview.selection_id, "Chosen label").unwrap();
    assert!(selected.created);
    assert_eq!(selected.project.name, "Chosen label");
    assert_eq!(selected.project.folder_path, preview.folder_path);
    assert_eq!(database.agent_store().list_projects().unwrap().len(), 1);
    assert!(folder.is_dir());
    assert_eq!(
        std::fs::read(existing_file).unwrap(),
        b"unchanged user fixture"
    );
}

#[test]
fn cancelled_folder_selection_drops_its_token_without_creating_metadata() {
    let (state, database) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let preview =
        begin_selected_project(&state, RootHandle::enroll(directory.path()).unwrap()).unwrap();
    discard_selected_project(&state, &preview.selection_id).unwrap();
    assert!(matches!(
        confirm_selected_project(&state, &preview.selection_id, "Never created"),
        Err(ProjectCommandError::InvalidSelection)
    ));
    assert!(database.agent_store().list_projects().unwrap().is_empty());
    assert!(directory.path().is_dir());
}

#[test]
fn discarding_evicted_repeated_or_unknown_selections_is_idempotent() {
    let (state, database) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let root = Arc::new(RootHandle::enroll(directory.path()).unwrap());
    let evicted = begin_selected_project(&state, root.clone()).unwrap();
    for _ in 0..32 {
        begin_selected_project(&state, root.clone()).unwrap();
    }
    for token in [&evicted.selection_id, "already-pruned-expired-like-token"] {
        discard_selected_project(&state, token).unwrap();
        discard_selected_project(&state, token).unwrap();
        assert!(matches!(
            confirm_selected_project(&state, token, "Must not register"),
            Err(ProjectCommandError::InvalidSelection)
        ));
    }
    let pending = begin_selected_project(&state, root).unwrap();
    discard_selected_project(&state, &pending.selection_id).unwrap();
    discard_selected_project(&state, &pending.selection_id).unwrap();
    assert!(matches!(
        confirm_selected_project(&state, &pending.selection_id, "Must not register"),
        Err(ProjectCommandError::InvalidSelection)
    ));
    assert!(database.agent_store().list_projects().unwrap().is_empty());
}

#[test]
fn discarding_absent_selection_does_not_require_an_installed_database() {
    let state = AppState::new();
    discard_selected_project(&state, "already-pruned-token").unwrap();
    discard_selected_project(&state, "already-pruned-token").unwrap();
    assert!(matches!(
        confirm_selected_project(&state, "already-pruned-token", "Must not register"),
        Err(ProjectCommandError::InvalidSelection)
    ));
    assert!(state.database().is_err());
}

#[test]
fn existing_project_binding_changes_cannot_silently_create_a_different_project() {
    let (state, database) = fixture();
    let original = tempfile::tempdir().unwrap();
    let replacement = tempfile::tempdir().unwrap();
    let root = RootHandle::enroll(original.path()).unwrap();
    let project = database
        .agent_store()
        .create_project(&root, Some("Original label"))
        .unwrap();
    let preview = begin_selected_project(&state, root).unwrap();
    assert_eq!(preview.existing_project.as_ref().unwrap().id, project.id);
    database
        .agent_store()
        .relocate_project(project.id, &RootHandle::enroll(replacement.path()).unwrap())
        .unwrap();

    assert!(matches!(
        confirm_selected_project(&state, &preview.selection_id, "Do not create"),
        Err(ProjectCommandError::StaleContext)
    ));
    let projects = database.agent_store().list_projects().unwrap();
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, project.id);
    assert_eq!(projects[0].name, "Original label");
}

#[test]
fn invalid_names_can_be_corrected_but_successful_tokens_cannot_be_reused() {
    let (state, database) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let preview =
        begin_selected_project(&state, RootHandle::enroll(directory.path()).unwrap()).unwrap();
    for name in [
        "".to_string(),
        "   ".to_string(),
        "bad\nname".to_string(),
        "x".repeat(129),
    ] {
        assert!(matches!(
            confirm_selected_project(&state, &preview.selection_id, &name),
            Err(ProjectCommandError::InvalidInput)
        ));
        assert!(database.agent_store().list_projects().unwrap().is_empty());
    }
    let selected =
        confirm_selected_project(&state, &preview.selection_id, "  Corrected label  ").unwrap();
    assert_eq!(selected.project.name, "Corrected label");
    assert!(matches!(
        confirm_selected_project(&state, &preview.selection_id, "Again"),
        Err(ProjectCommandError::InvalidSelection)
    ));
    assert_eq!(database.agent_store().list_projects().unwrap().len(), 1);
}

#[test]
fn replaced_or_revoked_selected_roots_never_create_project_metadata() {
    let (state, database) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let selected_path = directory.path().join("selected");
    std::fs::create_dir(&selected_path).unwrap();
    let preview =
        begin_selected_project(&state, RootHandle::enroll(&selected_path).unwrap()).unwrap();
    std::fs::rename(&selected_path, directory.path().join("moved-original")).unwrap();
    std::fs::create_dir(&selected_path).unwrap();
    assert!(matches!(
        confirm_selected_project(&state, &preview.selection_id, "Replacement"),
        Err(ProjectCommandError::FolderUnavailable { .. })
    ));
    let root = Arc::new(RootHandle::enroll(&selected_path).unwrap());
    let revoked = begin_selected_project(&state, root.clone()).unwrap();
    root.revoke();
    assert!(matches!(
        confirm_selected_project(&state, &revoked.selection_id, "Revoked"),
        Err(ProjectCommandError::FolderUnavailable { .. })
    ));
    assert!(database.agent_store().list_projects().unwrap().is_empty());
}

#[test]
fn a_new_selection_registered_by_another_caller_opens_existing_without_renaming() {
    let (state, database) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let preview =
        begin_selected_project(&state, RootHandle::enroll(directory.path()).unwrap()).unwrap();
    let existing = database
        .agent_store()
        .create_project(
            &RootHandle::enroll(directory.path()).unwrap(),
            Some("Other caller label"),
        )
        .unwrap();
    let result =
        confirm_selected_project(&state, &preview.selection_id, "Pending edited label").unwrap();
    assert!(!result.created);
    assert_eq!(result.project.id, existing.id);
    assert_eq!(result.project.name, "Other caller label");
    assert_eq!(database.agent_store().list_projects().unwrap().len(), 1);
}

#[test]
fn pending_selection_cannot_be_committed_into_a_different_database_instance() {
    let (state, database) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let preview =
        begin_selected_project(&state, RootHandle::enroll(directory.path()).unwrap()).unwrap();
    let other = Arc::new(Database::open_in_memory().unwrap());
    state.install_database(other.clone());
    assert!(matches!(
        confirm_selected_project(&state, &preview.selection_id, "Wrong database"),
        Err(ProjectCommandError::StaleContext)
    ));
    assert!(database.agent_store().list_projects().unwrap().is_empty());
    assert!(other.agent_store().list_projects().unwrap().is_empty());
}

#[test]
fn existing_selection_allows_label_only_changes_without_renaming_on_open() {
    let (state, database) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let root = RootHandle::enroll(directory.path()).unwrap();
    let project = database
        .agent_store()
        .create_project(&root, Some("Original label"))
        .unwrap();
    let preview = begin_selected_project(&state, root).unwrap();
    assert_eq!(preview.default_name, "Original label");
    assert_eq!(
        preview.existing_project.as_ref().unwrap().name,
        "Original label"
    );
    database
        .agent_store()
        .rename_project(project.id, "Cosmetic update")
        .unwrap();
    let selected =
        confirm_selected_project(&state, &preview.selection_id, "Must not rename").unwrap();
    assert!(!selected.created);
    assert_eq!(selected.project.id, project.id);
    assert_eq!(selected.project.name, "Cosmetic update");
}

#[test]
fn deleted_or_away_and_back_existing_bindings_remain_stale() {
    let (state, database) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let replacement = tempfile::tempdir().unwrap();
    let root = RootHandle::enroll(directory.path()).unwrap();
    let project = database
        .agent_store()
        .create_project(&root, Some("Original"))
        .unwrap();
    let preview =
        begin_selected_project(&state, RootHandle::enroll(directory.path()).unwrap()).unwrap();
    let removal = database
        .agent_store()
        .prepare_project_removal(
            "remove-previewed",
            project.id,
            foundryui_lib::foundry::db::agent::ProjectRemovalMode::Keep,
        )
        .unwrap();
    database
        .agent_store()
        .commit_context_cycle(&removal)
        .unwrap();
    let recreated = database
        .agent_store()
        .create_project(&root, Some("Recreated"))
        .unwrap();
    assert_ne!(recreated.id, project.id);
    assert!(matches!(
        confirm_selected_project(&state, &preview.selection_id, "Never create"),
        Err(ProjectCommandError::StaleContext)
    ));
    let preview =
        begin_selected_project(&state, RootHandle::enroll(directory.path()).unwrap()).unwrap();
    database
        .agent_store()
        .relocate_project(
            recreated.id,
            &RootHandle::enroll(replacement.path()).unwrap(),
        )
        .unwrap();
    database
        .agent_store()
        .relocate_project(recreated.id, &root)
        .unwrap();
    assert!(matches!(
        confirm_selected_project(&state, &preview.selection_id, "Never reopen"),
        Err(ProjectCommandError::StaleContext)
    ));
    assert_eq!(database.agent_store().list_projects().unwrap().len(), 1);
    assert_eq!(
        database.agent_store().project(recreated.id).unwrap().name,
        "Recreated"
    );
}

#[test]
fn pending_native_selections_are_bounded_and_evicted_tokens_cannot_create_projects() {
    let (state, database) = fixture();
    let directory = tempfile::tempdir().unwrap();
    let root = Arc::new(RootHandle::enroll(directory.path()).unwrap());
    let mut selections = Vec::new();
    for _ in 0..33 {
        selections.push(begin_selected_project(&state, root.clone()).unwrap());
    }
    let tokens: std::collections::HashSet<_> = selections
        .iter()
        .map(|selection| &selection.selection_id)
        .collect();
    assert_eq!(tokens.len(), 33);
    assert!(matches!(
        confirm_selected_project(&state, &selections[0].selection_id, "Evicted"),
        Err(ProjectCommandError::InvalidSelection)
    ));
    assert!(database.agent_store().list_projects().unwrap().is_empty());
    let selected =
        confirm_selected_project(&state, &selections[32].selection_id, "Newest selection").unwrap();
    assert!(selected.created);
    assert_eq!(selected.project.name, "Newest selection");
    assert_eq!(database.agent_store().list_projects().unwrap().len(), 1);
}
