#![cfg(target_os = "macos")]

use std::sync::Arc;

use foundryui_lib::foundry::agent_commands::AgentCommandError;
use foundryui_lib::foundry::db::agent::AgentStoreError;
use foundryui_lib::foundry::permission_commands::commit_selected_authorization_root;
use foundryui_lib::foundry::resources::RootHandle;
use foundryui_lib::foundry::{AppState, db::Database};

#[test]
fn native_folder_grant_keeps_conversation_scope_and_rejects_a_selection_pending_during_revocation()
{
    let database = Arc::new(Database::open_in_memory().expect("database"));
    let conversation = database
        .create_conversation("fixture-model")
        .expect("conversation");
    let other = database
        .create_conversation("fixture-model")
        .expect("other conversation");
    let folder = tempfile::tempdir().expect("selected folder");
    let root = RootHandle::enroll(folder.path()).expect("native root");
    let state = AppState::new();
    state.install_database(database.clone());
    let initial = database
        .agent_store()
        .authorization_revision(conversation.id)
        .expect("initial revision");
    let grant = commit_selected_authorization_root(&state, &initial, &root).expect("grant");
    assert_eq!(grant.conversation_id, conversation.id);
    assert!(
        database
            .agent_store()
            .list_grants(other.id)
            .expect("other grants")
            .is_empty()
    );
    let pending = database
        .agent_store()
        .authorization_revision(conversation.id)
        .expect("pending selection");
    database
        .agent_store()
        .revoke_grant(conversation.id, grant.id)
        .expect("revoke");

    assert_eq!(
        commit_selected_authorization_root(&state, &pending, &root).expect_err("stale selection"),
        AgentCommandError::Store(AgentStoreError::StaleContext)
    );
    assert!(
        database
            .agent_store()
            .list_grants(conversation.id)
            .expect("remaining grants")
            .is_empty()
    );
    assert!(folder.path().is_dir());
}
