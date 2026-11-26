#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceRoute {
    ChatCompletions,
    Responses,
    Embeddings,
    AudioTranscriptions,
}

impl InferenceRoute {
    pub(super) fn path(self) -> &'static str {
        match self {
            Self::ChatCompletions => "v1/chat/completions",
            Self::Responses => "v1/responses",
            Self::Embeddings => "v1/embeddings",
            Self::AudioTranscriptions => "v1/audio/transcriptions",
        }
    }
}

impl TryFrom<&str> for InferenceRoute {
    type Error = LocalInferenceError;

    fn try_from(route: &str) -> Result<Self, Self::Error> {
        match route {
            "/v1/chat/completions" => Ok(Self::ChatCompletions),
            "/v1/responses" => Ok(Self::Responses),
            "/v1/embeddings" => Ok(Self::Embeddings),
            "/v1/audio/transcriptions" => Ok(Self::AudioTranscriptions),
            _ => Err(LocalInferenceError::InvalidEndpoint),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error, serde::Serialize)]
pub enum LocalInferenceError {
    #[error("Local inference requires an explicit numeric loopback HTTP endpoint and port.")]
    InvalidEndpoint,
    #[error("The local inference transport could not be initialized.")]
    TransportUnavailable,
    #[error("The local inference request failed.")]
    RequestFailed,
    #[error("The local inference request timed out.")]
    RequestTimedOut,
}
