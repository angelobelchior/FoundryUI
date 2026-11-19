use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use tauri::AppHandle;
use tauri_plugin_dialog::{
    DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};

use super::db::agent::AgentStoreError;

pub type AgentCommandResult<T> = Result<T, AgentCommandError>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentCommandError {
    #[error("{0}")]
    Store(#[from] AgentStoreError),
    #[error("the conversation database is not ready")]
    NotReady,
    #[error("the native confirmation did not finish")]
    DialogUnavailable,
    #[error("the selected folder could not be validated")]
    InvalidSelection,
    #[error("a concrete native preview is unavailable; this request remains pending")]
    PreviewUnavailable,
}

impl AgentCommandError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Store(AgentStoreError::NotFound) => "AgentNotFound",
            Self::Store(AgentStoreError::InvalidInput) => "AgentInvalidInput",
            Self::Store(AgentStoreError::Conflict) => "AgentConflict",
            Self::Store(AgentStoreError::StaleContext) => "AgentStaleContext",
            Self::Store(AgentStoreError::InvalidTransition) => "AgentInvalidTransition",
            Self::Store(AgentStoreError::AlreadyClaimed) => "AgentAlreadyClaimed",
            Self::Store(AgentStoreError::ConversationBusy) => "AgentBusy",
            Self::Store(AgentStoreError::Storage | AgentStoreError::Database { .. }) => {
                "AgentStorage"
            }
            Self::NotReady => "AgentNotReady",
            Self::DialogUnavailable => "AgentDialogUnavailable",
            Self::InvalidSelection => "AgentInvalidSelection",
            Self::PreviewUnavailable => "AgentPreviewUnavailable",
        }
    }
}

impl Serialize for AgentCommandError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let detail = if let Self::Store(AgentStoreError::Database { kind, sqlite_code }) = self {
            Some(format!("kind:{kind:?};sqlite:{sqlite_code:?}"))
        } else {
            None
        };
        let mut state = serializer.serialize_struct("AgentCommandError", 4)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("messageKey", &format!("errors.{}", self.code()))?;
        state.serialize_field("message", &self.to_string())?;
        state.serialize_field("detail", &detail)?;
        state.end()
    }
}

pub(crate) fn single_line(value: &str) -> String {
    value.chars().map(|character| {
        if character.is_control() || matches!(character, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}') {
            ' '
        } else {
            character
        }
    }).collect()
}

pub(crate) async fn native_choice(
    app: &AppHandle,
    title: &str,
    body: String,
    buttons: MessageDialogButtons,
) -> AgentCommandResult<MessageDialogResult> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(body)
        .title(title)
        .kind(MessageDialogKind::Warning)
        .buttons(buttons)
        .show_with_result(move |answer| {
            let _ = sender.send(answer);
        });
    receiver
        .await
        .map_err(|_| AgentCommandError::DialogUnavailable)
}
