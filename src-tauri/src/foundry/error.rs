use serde::ser::{Serialize, SerializeStruct, Serializer};

pub type FoundryResult<T> = Result<T, FoundryError>;

#[derive(Debug, Clone, thiserror::Error)]
pub enum FoundryError {
    #[error("foundry CLI is not installed")]
    CliMissing,
    #[error("foundry CLI {found} is older than {minimum}")]
    CliOutdated { found: String, minimum: String },
    #[error("server unreachable: {detail}")]
    ServerUnreachable { detail: String },
    #[error("model {alias} is not downloaded")]
    ModelNotCached { alias: String },
    #[error("model {alias} is not loaded")]
    ModelNotLoaded { alias: String },
    #[error("not enough memory: {detail}")]
    OutOfMemory { detail: String },
    #[error("{stage} timed out after {seconds}s")]
    Timeout { stage: String, seconds: u64 },
    #[error("operation cancelled")]
    Cancelled,
    #[error("unexpected response: {detail}")]
    BadResponse { detail: String },
    #[error("{command} failed: {message}")]
    CommandFailed {
        command: String,
        message: String,
        hint: Option<String>,
    },
    #[error("local database error: {detail}")]
    Storage { detail: String },
    #[error("model {alias} does not accept images")]
    UnsupportedImage { alias: String },
    #[error("attachment rejected: {reason}")]
    AttachmentRejected { reason: String },
    #[error("audio rejected: {reason}")]
    AudioRejected { reason: String },
    #[error("microphone access was denied")]
    MicrophoneDenied,
    #[error("could not record audio: {detail}")]
    RecordingFailed { detail: String },
    #[error("could not export the conversation: {detail}")]
    ExportFailed { detail: String },
}

impl FoundryError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::CliMissing => "CliMissing",
            Self::CliOutdated { .. } => "CliOutdated",
            Self::ServerUnreachable { .. } => "ServerUnreachable",
            Self::ModelNotCached { .. } => "ModelNotCached",
            Self::ModelNotLoaded { .. } => "ModelNotLoaded",
            Self::OutOfMemory { .. } => "OutOfMemory",
            Self::Timeout { .. } => "Timeout",
            Self::Cancelled => "Cancelled",
            Self::BadResponse { .. } => "BadResponse",
            Self::CommandFailed { .. } => "CommandFailed",
            Self::Storage { .. } => "Storage",
            Self::UnsupportedImage { .. } => "UnsupportedImage",
            Self::AttachmentRejected { .. } => "AttachmentRejected",
            Self::AudioRejected { .. } => "AudioRejected",
            Self::MicrophoneDenied => "MicrophoneDenied",
            Self::RecordingFailed { .. } => "RecordingFailed",
            Self::ExportFailed { .. } => "ExportFailed",
        }
    }

    pub fn message_key(&self) -> String {
        format!("errors.{}", self.code())
    }

    pub fn detail(&self) -> Option<String> {
        match self {
            Self::CliOutdated { found, minimum } => {
                Some(format!("found {found}, minimum {minimum}"))
            }
            Self::ServerUnreachable { detail }
            | Self::OutOfMemory { detail }
            | Self::BadResponse { detail } => Some(detail.clone()),
            Self::Storage { detail } => Some(detail.clone()),
            Self::UnsupportedImage { alias } => Some(alias.clone()),
            Self::AttachmentRejected { reason } => Some(reason.clone()),
            Self::AudioRejected { reason } => Some(reason.clone()),
            Self::RecordingFailed { detail } => Some(detail.clone()),
            Self::ExportFailed { detail } => Some(detail.clone()),
            Self::ModelNotCached { alias } | Self::ModelNotLoaded { alias } => Some(alias.clone()),
            Self::Timeout { stage, seconds } => Some(format!("{stage}, {seconds}s")),
            Self::CommandFailed { hint, .. } => hint.clone(),
            Self::CliMissing | Self::Cancelled | Self::MicrophoneDenied => None,
        }
    }
}

impl Serialize for FoundryError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("FoundryError", 4)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("messageKey", &self.message_key())?;
        state.serialize_field("message", &self.to_string())?;
        state.serialize_field("detail", &self.detail())?;
        state.end()
    }
}
