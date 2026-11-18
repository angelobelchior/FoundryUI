use super::{LocalInferenceError, RuntimeObservation};

#[cfg(target_os = "macos")]
mod macos;

#[cfg(target_os = "macos")]
pub(super) use macos::{RuntimeIdentity, verify, verify_connection};

#[cfg(not(target_os = "macos"))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RuntimeIdentity;

#[cfg(not(target_os = "macos"))]
pub(super) fn verify(_: &RuntimeObservation) -> Result<RuntimeIdentity, LocalInferenceError> {
    Err(LocalInferenceError::UnsupportedPlatform)
}

#[cfg(not(target_os = "macos"))]
pub(super) fn verify_connection(
    _: &RuntimeObservation,
    _: std::net::SocketAddr,
) -> Result<Option<RuntimeIdentity>, LocalInferenceError> {
    Err(LocalInferenceError::UnsupportedPlatform)
}
