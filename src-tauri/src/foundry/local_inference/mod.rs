mod native;
mod transport;
mod types;

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use types::{InferenceRoute, LocalInferenceError, RuntimeObservation};

pub fn verify_foundry_endpoint(
    cli_binary: &std::path::Path,
    pid: u32,
    endpoint: &str,
    connect: Duration,
) -> Result<VerifiedLocalEndpoint, LocalInferenceError> {
    let runtime = foundry_runtime_path(cli_binary)?;
    let binding = RuntimeBinding::default();
    binding.replace(Some(RuntimeObservation::new(endpoint, pid, runtime, 0)?));
    let mut verified = VerifiedLocalEndpoint::verify(&binding)?;
    verified.connect_timeout = connect;
    Ok(verified)
}

#[cfg(target_os = "macos")]
pub fn foundry_runtime_path(
    cli_executable: impl AsRef<std::path::Path>,
) -> Result<std::path::PathBuf, LocalInferenceError> {
    use std::os::unix::fs::PermissionsExt;
    let invalid = || LocalInferenceError::RuntimeUnverified;
    if !cli_executable.as_ref().is_absolute() {
        return Err(invalid());
    }
    let cli = cli_executable
        .as_ref()
        .canonicalize()
        .map_err(|_| invalid())?;
    let cli_metadata = std::fs::metadata(&cli).map_err(|_| invalid())?;
    if cli.file_name().and_then(|name| name.to_str()) != Some("foundry")
        || !cli_metadata.is_file()
        || cli_metadata.permissions().mode() & 0o111 == 0
    {
        return Err(invalid());
    }
    let directory = cli.parent().ok_or_else(invalid)?;
    let daemon = directory.join("foundrylocald");
    match std::fs::symlink_metadata(&daemon) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(cli),
        Err(_) => Err(invalid()),
        Ok(_) => {
            let daemon = daemon.canonicalize().map_err(|_| invalid())?;
            let metadata = std::fs::metadata(&daemon).map_err(|_| invalid())?;
            if daemon.parent() != Some(directory)
                || !metadata.is_file()
                || metadata.permissions().mode() & 0o111 == 0
            {
                return Err(invalid());
            }
            Ok(daemon)
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn foundry_runtime_path(
    _: impl AsRef<std::path::Path>,
) -> Result<std::path::PathBuf, LocalInferenceError> {
    Err(LocalInferenceError::UnsupportedPlatform)
}

#[derive(Debug, Clone, Default)]
pub struct RuntimeBinding {
    state: Arc<Mutex<BindingState>>,
}

#[derive(Debug, Default)]
struct BindingState {
    observation: Option<RuntimeObservation>,
    revision: Arc<()>,
}

impl RuntimeBinding {
    pub fn replace(&self, observation: Option<RuntimeObservation>) {
        if let Ok(mut state) = self.state.lock()
            && state.observation != observation
        {
            state.observation = observation;
            state.revision = Arc::new(());
        }
    }
}

#[derive(Debug, Clone)]
pub struct VerifiedLocalEndpoint {
    binding: RuntimeBinding,
    revision: Arc<()>,
    observation: RuntimeObservation,
    identity: native::RuntimeIdentity,
    session_revision: Option<(Arc<AtomicU64>, u64)>,
    connect_timeout: Duration,
}

impl VerifiedLocalEndpoint {
    pub fn verify(binding: &RuntimeBinding) -> Result<Self, LocalInferenceError> {
        let (observation, revision) = {
            let state = binding
                .state
                .lock()
                .map_err(|_| LocalInferenceError::RuntimeUnverified)?;
            let observation = state
                .observation
                .clone()
                .ok_or(LocalInferenceError::RuntimeUnverified)?;
            (observation, state.revision.clone())
        };
        let identity = native::verify(&observation)?;
        let verified = Self {
            binding: binding.clone(),
            revision,
            observation,
            identity,
            session_revision: None,
            connect_timeout: Duration::from_secs(5),
        };
        verified.check_binding()?;
        Ok(verified)
    }

    pub fn bind_session(mut self, revision: Arc<AtomicU64>, expected: u64) -> Self {
        if self.session_revision.is_none() {
            self.session_revision = Some((revision, expected));
        }
        self
    }

    pub fn revalidate(&self) -> Result<(), LocalInferenceError> {
        self.check_binding()?;
        if native::verify(&self.observation)? != self.identity {
            return Err(LocalInferenceError::SessionChanged);
        }
        self.check_binding()
    }

    pub async fn post_json<T: serde::Serialize + ?Sized>(
        &self,
        route: impl TryInto<InferenceRoute>,
        body: &T,
        timeout: Duration,
    ) -> Result<reqwest::Response, LocalInferenceError> {
        let request = self.post_request(route)?;
        let request = request
            .json(body)
            .timeout(timeout)
            .build()
            .map_err(|_| LocalInferenceError::RequestFailed)?;
        let deadline = tokio::time::Instant::now() + timeout;
        tokio::time::timeout_at(deadline, self.dispatch(request, deadline, Some(deadline)))
            .await
            .map_err(|_| LocalInferenceError::RequestTimedOut)?
    }

    pub async fn post_json_stream<T: serde::Serialize + ?Sized>(
        &self,
        route: impl TryInto<InferenceRoute>,
        body: &T,
        header_timeout: Duration,
    ) -> Result<reqwest::Response, LocalInferenceError> {
        let request = self.post_request(route)?;
        let request = request
            .json(body)
            .build()
            .map_err(|_| LocalInferenceError::RequestFailed)?;
        let deadline = tokio::time::Instant::now() + header_timeout;
        tokio::time::timeout_at(deadline, self.dispatch(request, deadline, None))
            .await
            .map_err(|_| LocalInferenceError::RequestTimedOut)?
    }

    pub async fn post_multipart(
        &self,
        route: impl TryInto<InferenceRoute>,
        form: reqwest::multipart::Form,
        timeout: Duration,
    ) -> Result<reqwest::Response, LocalInferenceError> {
        let request = self.post_request(route)?;
        let request = request
            .multipart(form)
            .timeout(timeout)
            .build()
            .map_err(|_| LocalInferenceError::RequestFailed)?;
        let deadline = tokio::time::Instant::now() + timeout;
        tokio::time::timeout_at(deadline, self.dispatch(request, deadline, Some(deadline)))
            .await
            .map_err(|_| LocalInferenceError::RequestTimedOut)?
    }

    fn check_session_revision(&self) -> Result<(), LocalInferenceError> {
        if self
            .session_revision
            .as_ref()
            .is_some_and(|(revision, expected)| revision.load(Ordering::SeqCst) != *expected)
        {
            return Err(LocalInferenceError::SessionChanged);
        }
        Ok(())
    }

    fn post_request(
        &self,
        route: impl TryInto<InferenceRoute>,
    ) -> Result<reqwest::RequestBuilder, LocalInferenceError> {
        let route = route
            .try_into()
            .map_err(|_| LocalInferenceError::InvalidEndpoint)?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .http1_only()
            .pool_max_idle_per_host(0)
            .connect_timeout(self.connect_timeout)
            .build()
            .map_err(|_| LocalInferenceError::TransportUnavailable)?;
        let url = self
            .observation
            .endpoint
            .join(route.path())
            .map_err(|_| LocalInferenceError::InvalidEndpoint)?;
        Ok(client.post(url))
    }

    fn check_binding(&self) -> Result<(), LocalInferenceError> {
        self.check_session_revision()?;
        let state = self
            .binding
            .state
            .lock()
            .map_err(|_| LocalInferenceError::RuntimeUnverified)?;
        if !Arc::ptr_eq(&state.revision, &self.revision)
            || state.observation.as_ref() != Some(&self.observation)
        {
            return Err(LocalInferenceError::SessionChanged);
        }
        Ok(())
    }
}
