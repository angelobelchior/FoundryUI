use serde::Deserialize;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogResult};

use super::AppState;
use super::agent_commands::{AgentCommandError, AgentCommandResult, native_choice, single_line};
use super::db::Database;
use super::db::agent::{
    AgentStoreError, InstructionDiscovery, InstructionPreview, InstructionSource, InstructionTarget,
};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionDecisionInput {
    pub conversation_id: i64,
    pub context_revision: i64,
    pub authorization_revision: i64,
    pub mode_revision: i64,
    pub root_id: String,
    pub path: String,
    pub identity_key: String,
    pub version_key: String,
    pub content_fingerprint: String,
    pub approve: bool,
}

fn expected_preview(
    database: &Database,
    input: &InstructionDecisionInput,
) -> AgentCommandResult<InstructionPreview> {
    let preview = database.agent_store().preview_instruction(
        input.conversation_id,
        &input.root_id,
        &input.path,
    )?;
    if (
        preview.key().context_revision,
        preview.key().authorization_revision,
        preview.key().mode_revision,
    ) != (
        input.context_revision,
        input.authorization_revision,
        input.mode_revision,
    ) {
        return Err(AgentStoreError::StaleContext.into());
    }
    let source = preview.source();
    if source.identity_key != input.identity_key
        || source.version_key != input.version_key
        || source.content_fingerprint != input.content_fingerprint
    {
        return Err(AgentStoreError::Conflict.into());
    }
    Ok(preview)
}

pub(crate) fn instruction_preview_body(
    preview: &InstructionPreview,
    language: Option<&str>,
) -> AgentCommandResult<(String, String, String)> {
    if !preview.source().preview_available {
        return Err(AgentCommandError::PreviewUnavailable);
    }
    let (title, notice, allow, cancel) = match language {
        Some("pt-BR") => (
            "Aprovar orientação da pasta",
            "Esta aprovação vale apenas como orientação da tarefa nesta conversa e nesta versão. Não concede acesso a arquivos, rede, comandos ou ações destrutivas. A solicitação atual prevalece; conflitos restantes devem ser apresentados ao usuário.",
            "Aprovar esta versão",
            "Cancelar",
        ),
        Some("es") => (
            "Aprobar orientación de la carpeta",
            "Esta aprobación solo sirve como orientación de la tarea en esta conversación y versión. No concede acceso a archivos, red, comandos ni acciones destructivas. La solicitud actual prevalece; los conflictos restantes deben mostrarse al usuario.",
            "Aprobar esta versión",
            "Cancelar",
        ),
        _ => (
            "Approve folder guidance",
            "Approval applies only as task guidance in this conversation and version. It grants no file, network, command or destructive-action permission. The current request takes precedence; remaining conflicts must be shown to the user.",
            "Approve this version",
            "Cancel",
        ),
    };
    let source = preview.source();
    let body = format!(
        "{notice}\n\n#{}\n{}\n{}\nSHA-256: {}\n\n{}",
        preview.key().conversation_id,
        single_line(&source.root_id),
        single_line(&source.path),
        source.content_fingerprint,
        source.content
    );
    if body.len() > 8192 {
        return Err(AgentCommandError::PreviewUnavailable);
    }
    Ok((format!("{title}\n\n{body}"), allow.into(), cancel.into()))
}

#[tauri::command]
pub fn instructions_discover(
    state: State<'_, AppState>,
    conversation_id: i64,
    targets: Option<Vec<InstructionTarget>>,
) -> AgentCommandResult<InstructionDiscovery> {
    let database = state.database().map_err(|_| AgentCommandError::NotReady)?;
    Ok(database
        .agent_store()
        .discover_instructions(conversation_id, &targets.unwrap_or_default())?)
}

#[tauri::command]
pub async fn instructions_decide(
    app: AppHandle,
    state: State<'_, AppState>,
    input: InstructionDecisionInput,
    language: Option<String>,
) -> AgentCommandResult<Option<InstructionSource>> {
    let database = state.database().map_err(|_| AgentCommandError::NotReady)?;
    let preview = expected_preview(&database, &input)?;
    if input.approve {
        let (body, allow, cancel) = instruction_preview_body(&preview, language.as_deref())?;
        let answer = native_choice(
            &app,
            "FoundryUI",
            body,
            MessageDialogButtons::OkCancelCustom(allow.clone(), cancel),
        )
        .await?;
        if !matches!(answer, MessageDialogResult::Custom(label) if label == allow) {
            return Ok(None);
        }
    }
    commit_instruction_preview(&state, &preview, input.approve).map(Some)
}

fn commit_instruction_preview(
    state: &AppState,
    preview: &InstructionPreview,
    approve: bool,
) -> AgentCommandResult<InstructionSource> {
    state
        .with_database(|database| database.agent_store().decide_instruction(preview, approve))
        .map_err(|_| AgentCommandError::NotReady)?
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foundry::db::agent::InstructionStatus;
    use crate::foundry::resources::RootHandle;
    use std::sync::Arc;

    fn fixture() -> (tempfile::TempDir, AppState, Arc<Database>, i64, String) {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(
            folder.path().join("AGENTS.md"),
            "Exact first line.\nExact final line.",
        )
        .unwrap();
        let root = RootHandle::enroll(folder.path()).unwrap();
        let root_id = root.identity().unwrap().storage_key();
        let database = Arc::new(Database::open_in_memory().unwrap());
        let project = database
            .agent_store()
            .create_project(&root, Some("Fixture"))
            .unwrap();
        let conversation = database
            .agent_store()
            .create_project_conversation(Some(project.id), "fixture")
            .unwrap()
            .conversation
            .id;
        let state = AppState::new();
        state.install_database(database.clone());
        (folder, state, database, conversation, root_id)
    }

    #[test]
    fn the_native_body_contains_the_complete_source_and_a_no_permission_notice() {
        let (_folder, _state, database, conversation, root_id) = fixture();
        let preview = database
            .agent_store()
            .preview_instruction(conversation, &root_id, "AGENTS.md")
            .unwrap();
        for language in ["en", "pt-BR", "es"] {
            let (body, allow, cancel) = instruction_preview_body(&preview, Some(language)).unwrap();
            assert!(body.ends_with(&preview.source().content));
            assert!(body.contains(&preview.source().content_fingerprint));
            assert!(body.contains(&format!("#{conversation}")));
            assert!(!allow.is_empty());
            assert!(!cancel.is_empty());
        }
        assert!(
            instruction_preview_body(&preview, Some("en"))
                .unwrap()
                .0
                .contains("grants no file, network, command or destructive-action permission")
        );
    }

    #[test]
    fn frontend_content_cannot_replace_the_source_shown_for_approval() {
        let (_folder, _state, database, conversation, root_id) = fixture();
        let discovery = database
            .agent_store()
            .discover_instructions(conversation, &[])
            .unwrap();
        let source = &discovery.sources[0];
        let input: InstructionDecisionInput = serde_json::from_value(serde_json::json!({
            "conversationId": conversation, "contextRevision": discovery.context_revision,
            "authorizationRevision": discovery.authorization_revision, "modeRevision": discovery.mode_revision,
            "rootId": root_id, "path": source.path, "identityKey": source.identity_key,
            "versionKey": source.version_key, "contentFingerprint": source.content_fingerprint,
            "approve": true, "content": "FORGED FRONTEND TEXT"
        })).unwrap();
        let preview = expected_preview(&database, &input).unwrap();
        let (body, _, _) = instruction_preview_body(&preview, Some("en")).unwrap();
        assert!(!body.contains("FORGED FRONTEND TEXT"));
        assert!(body.ends_with("Exact first line.\nExact final line."));
    }

    #[test]
    fn a_change_during_native_confirmation_rejects_the_decision() {
        let (folder, state, database, conversation, root_id) = fixture();
        let preview = database
            .agent_store()
            .preview_instruction(conversation, &root_id, "AGENTS.md")
            .unwrap();
        std::fs::write(
            folder.path().join("AGENTS.md"),
            "Changed while the native dialog was open.",
        )
        .unwrap();
        assert!(matches!(
            commit_instruction_preview(&state, &preview, true),
            Err(AgentCommandError::Store(AgentStoreError::Conflict))
        ));
        assert_eq!(
            database
                .agent_store()
                .discover_instructions(conversation, &[])
                .unwrap()
                .sources[0]
                .status,
            InstructionStatus::Pending
        );
    }

    #[test]
    fn a_replaced_database_cannot_consume_a_native_preview_from_the_old_instance() {
        let (folder, state, database, conversation, root_id) = fixture();
        let preview = database
            .agent_store()
            .preview_instruction(conversation, &root_id, "AGENTS.md")
            .unwrap();
        let replacement = Arc::new(Database::open_in_memory().unwrap());
        let root = RootHandle::enroll(folder.path()).unwrap();
        let project = replacement
            .agent_store()
            .create_project(&root, Some("Fixture"))
            .unwrap();
        assert_eq!(
            replacement
                .agent_store()
                .create_project_conversation(Some(project.id), "fixture")
                .unwrap()
                .conversation
                .id,
            conversation
        );
        state.install_database(replacement.clone());
        assert!(matches!(
            commit_instruction_preview(&state, &preview, true),
            Err(AgentCommandError::Store(AgentStoreError::Conflict))
        ));
        assert_eq!(
            replacement
                .agent_store()
                .discover_instructions(conversation, &[])
                .unwrap()
                .sources[0]
                .status,
            InstructionStatus::Pending
        );
    }
}
