use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceRoute {
    ChatCompletions,
    Embeddings,
    AudioTranscriptions,
}

impl InferenceRoute {
    pub(super) fn path(self) -> &'static str {
        match self {
            Self::ChatCompletions => "v1/chat/completions",
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
    #[error("The local runtime identity could not be verified. Rediscover the Foundry runtime.")]
    RuntimeUnverified,
    #[error("The inference listener is not owned exclusively by the expected local runtime.")]
    ListenerUnverified,
    #[error("The local inference endpoint or runtime changed. Prepare the context again.")]
    SessionChanged,
    #[error("Native local inference verification is unavailable on this platform.")]
    UnsupportedPlatform,
    #[error("The verified local inference transport could not be initialized.")]
    TransportUnavailable,
    #[error("The verified local inference request failed.")]
    RequestFailed,
    #[error("The verified local inference request timed out.")]
    RequestTimedOut,
    #[error("The local inference endpoint attempted a redirect; no redirect was followed.")]
    RedirectBlocked,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeObservation {
    pub(super) endpoint: reqwest::Url,
    pub(super) address: SocketAddr,
    pub(super) pid: u32,
    pub(super) expected_executable: PathBuf,
    pub(super) generation: u64,
}

impl RuntimeObservation {
    pub fn new(
        endpoint: &str,
        pid: u32,
        expected_executable: impl AsRef<Path>,
        generation: u64,
    ) -> Result<Self, LocalInferenceError> {
        let invalid = || LocalInferenceError::InvalidEndpoint;
        let authority = endpoint.strip_prefix("http://").ok_or_else(invalid)?;
        let authority = authority.strip_suffix('/').unwrap_or(authority);
        let (host, port) = authority.rsplit_once(':').ok_or_else(invalid)?;
        let host = host
            .strip_prefix('[')
            .and_then(|host| host.strip_suffix(']'))
            .unwrap_or(host);
        let ip: IpAddr = host.parse().map_err(|_| invalid())?;
        let port: u16 = port.parse().map_err(|_| invalid())?;
        if !ip.is_loopback() || port == 0 {
            return Err(invalid());
        }
        let address = SocketAddr::new(ip, port);
        let canonical = format!("http://{address}");
        if endpoint != canonical && endpoint != format!("{canonical}/") {
            return Err(invalid());
        }
        Ok(Self {
            endpoint: reqwest::Url::parse(&canonical).map_err(|_| invalid())?,
            address,
            pid,
            expected_executable: expected_executable.as_ref().to_path_buf(),
            generation,
        })
    }
}
