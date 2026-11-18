use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::backend::FoundryBackend;
use super::cli::CliBackend;
use super::error::{FoundryError, FoundryResult};
use super::http::{
    CONNECT_TIMEOUT, FIRST_TOKEN_TIMEOUT, StreamTimeouts, TOKEN_IDLE_TIMEOUT, build_client,
    health_check,
};
use super::json::{DaemonFile, ServerSnapshot, ServerStatus, parse_json};
use super::local_inference::{VerifiedLocalEndpoint, verify_foundry_endpoint};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerPreferences {
    pub port: Option<u16>,
    pub keep_running: bool,
    pub connect_timeout_secs: u64,
    pub first_token_timeout_secs: u64,
    pub token_idle_timeout_secs: u64,
}

impl Default for ServerPreferences {
    fn default() -> Self {
        Self {
            port: None,
            keep_running: false,
            connect_timeout_secs: CONNECT_TIMEOUT.as_secs(),
            first_token_timeout_secs: FIRST_TOKEN_TIMEOUT.as_secs(),
            token_idle_timeout_secs: TOKEN_IDLE_TIMEOUT.as_secs(),
        }
    }
}

impl ServerPreferences {
    pub fn normalized(mut self) -> Self {
        self.port = self.port.filter(|port| *port != 0);
        self.connect_timeout_secs = self.connect_timeout_secs.max(1);
        self.first_token_timeout_secs = self.first_token_timeout_secs.max(1);
        self.token_idle_timeout_secs = self.token_idle_timeout_secs.max(1);
        self
    }
}

#[derive(Debug, Clone, Copy)]
enum Ownership {
    Keep,
    App,
    External,
}

impl Ownership {
    fn started_by_app(self) -> Option<bool> {
        match self {
            Self::Keep => None,
            Self::App => Some(true),
            Self::External => Some(false),
        }
    }
}

#[derive(Debug, Default)]
struct ServerState {
    endpoint: Option<String>,
    started_by_app: bool,
    status: Option<ServerStatus>,
    last_observed_at: Option<Instant>,
}

pub struct ServerManager {
    state: Mutex<ServerState>,
    preferences: Mutex<ServerPreferences>,
    client: Client,
    session_revision: Arc<AtomicU64>,
}

impl ServerManager {
    pub fn new(preferences: ServerPreferences) -> Self {
        Self {
            state: Mutex::new(ServerState::default()),
            preferences: Mutex::new(preferences.normalized()),
            client: build_client(),
            session_revision: Arc::new(AtomicU64::new(1)),
        }
    }

    pub fn preferences(&self) -> ServerPreferences {
        lock(&self.preferences).clone()
    }

    pub fn set_preferences(&self, preferences: ServerPreferences) {
        let changed = {
            let mut current = lock(&self.preferences);
            let changed = current.port != preferences.port;
            *current = preferences.normalized();
            changed
        };
        if changed {
            self.invalidate_prepared_context();
        }
    }

    pub(crate) fn invalidate_prepared_context(&self) {
        let _state = lock(&self.state);
        self.session_revision.fetch_add(1, Ordering::SeqCst);
    }

    pub fn endpoint(&self) -> Option<String> {
        lock(&self.state).endpoint.clone()
    }

    pub fn stream_timeouts(&self) -> StreamTimeouts {
        let preferences = lock(&self.preferences);

        StreamTimeouts::from_seconds(
            preferences.first_token_timeout_secs,
            preferences.token_idle_timeout_secs,
        )
        .with_connect_seconds(preferences.connect_timeout_secs)
    }

    pub async fn verified_endpoint(
        &self,
        cli: &CliBackend,
    ) -> FoundryResult<VerifiedLocalEndpoint> {
        let expected_revision = {
            let _state = lock(&self.state);
            self.session_revision.load(Ordering::SeqCst)
        };
        let status = match cli.server_status().await {
            Ok(status) => status,
            Err(_) => {
                self.invalidate_prepared_context();
                return Err(FoundryError::ServerUnreachable {
                    detail:
                        "the local runtime identity could not be refreshed; no context was sent"
                            .to_string(),
                });
            }
        };
        let revision = {
            let mut state = lock(&self.state);
            if self.session_revision.load(Ordering::SeqCst) != expected_revision {
                return Err(super::http::local_inference_error(
                    super::local_inference::LocalInferenceError::SessionChanged,
                ));
            }
            self.update_session(
                &mut state,
                Some(status.clone()),
                status
                    .endpoint()
                    .filter(|_| status.running)
                    .map(str::to_string),
                Ownership::Keep,
            )
        };
        let endpoint = status
            .endpoint()
            .filter(|_| status.running)
            .ok_or_else(|| FoundryError::ServerUnreachable {
                detail: "start the verified local server and try again".to_string(),
            })?
            .to_string();
        let pid = status.pid.ok_or_else(|| FoundryError::ServerUnreachable {
            detail: "the local runtime did not provide a process identity; no context was sent"
                .to_string(),
        })?;
        let cli_path = std::path::PathBuf::from(cli.info().path);
        let connect = self.stream_timeouts().connect;
        let proof = tokio::time::timeout(
            connect,
            tokio::task::spawn_blocking(move || {
                verify_foundry_endpoint(&cli_path, pid, &endpoint, connect)
            }),
        )
        .await
        .map_err(|_| {
            super::http::local_inference_error(
                super::local_inference::LocalInferenceError::RequestTimedOut,
            )
        })?
        .map_err(|_| {
            super::http::local_inference_error(
                super::local_inference::LocalInferenceError::RuntimeUnverified,
            )
        })?
        .map_err(super::http::local_inference_error)?;
        if self.session_revision.load(Ordering::SeqCst) != revision {
            return Err(super::http::local_inference_error(
                super::local_inference::LocalInferenceError::SessionChanged,
            ));
        }
        Ok(proof.bind_session(self.session_revision.clone(), revision))
    }

    pub fn snapshot(&self) -> ServerSnapshot {
        snapshot_from(&lock(&self.state))
    }

    pub fn should_stop_on_exit(&self) -> bool {
        let keep_running = lock(&self.preferences).keep_running;

        lock(&self.state).started_by_app && !keep_running
    }

    pub fn mark_stopped(&self) {
        let mut state = lock(&self.state);
        self.session_revision.fetch_add(1, Ordering::SeqCst);
        *state = ServerState::default();
    }

    pub async fn observe_cached(
        &self,
        cli: &CliBackend,
        max_age: Duration,
    ) -> FoundryResult<ServerSnapshot> {
        let cached_is_fresh = {
            let state = lock(&self.state);

            state
                .last_observed_at
                .is_some_and(|at| at.elapsed() <= max_age)
        };

        if cached_is_fresh {
            return Ok(self.snapshot());
        }

        self.observe(cli).await
    }

    pub async fn observe(&self, cli: &CliBackend) -> FoundryResult<ServerSnapshot> {
        let cached_endpoint = lock(&self.state).endpoint.clone();

        if let Some(endpoint) = cached_endpoint
            && health_check(&self.client, &endpoint).await
        {
            self.store_session(None, Some(endpoint), Ownership::Keep);

            return Ok(self.snapshot());
        }

        if let Some(status) = self.daemon_status().await {
            let endpoint = status.endpoint().map(str::to_string);

            self.store_session(Some(status), endpoint, Ownership::Keep);

            return Ok(self.snapshot());
        }

        let status_result = cli.server_status().await;

        if let Ok(status) = &status_result
            && status.is_ready()
            && let Some(endpoint) = status.endpoint()
            && health_check(&self.client, endpoint).await
        {
            self.store_session(
                Some(status.clone()),
                Some(endpoint.to_string()),
                Ownership::Keep,
            );

            return Ok(self.snapshot());
        }

        match status_result {
            Ok(status) => {
                let ownership = if status.running {
                    Ownership::Keep
                } else {
                    Ownership::External
                };

                self.store_session(Some(status), None, ownership);

                Ok(self.snapshot())
            }
            Err(error) => {
                self.invalidate_prepared_context();
                Err(error)
            }
        }
    }

    pub async fn ensure_running(&self, cli: &CliBackend) -> FoundryResult<ServerSnapshot> {
        if let Ok(snapshot) = self.observe(cli).await
            && snapshot.running
        {
            return Ok(snapshot);
        }

        let port = lock(&self.preferences).port;
        let started = cli.server_start(port).await?;

        self.adopt_after_command(
            cli,
            started,
            Ownership::App,
            "the daemon started but did not answer on its advertised URL",
        )
        .await
    }

    pub async fn stop(&self, cli: &CliBackend) -> FoundryResult<ServerSnapshot> {
        self.invalidate_prepared_context();
        cli.server_stop().await?;
        self.mark_stopped();

        Ok(self.snapshot())
    }

    pub async fn restart(&self, cli: &CliBackend) -> FoundryResult<ServerSnapshot> {
        self.invalidate_prepared_context();
        let (was_running, started_by_app) = {
            let state = lock(&self.state);

            (state.endpoint.is_some(), state.started_by_app)
        };
        let ownership = if started_by_app || !was_running {
            Ownership::App
        } else {
            Ownership::External
        };
        let port = lock(&self.preferences).port;
        let restarted = cli.server_restart(port).await?;

        self.adopt_after_command(
            cli,
            restarted,
            ownership,
            "the daemon restarted but did not answer on its advertised URL",
        )
        .await
    }

    pub async fn logs(&self, cli: &CliBackend, lines: u32) -> FoundryResult<String> {
        cli.server_logs(lines).await
    }

    async fn adopt_after_command(
        &self,
        cli: &CliBackend,
        status: ServerStatus,
        ownership: Ownership,
        failure_detail: &str,
    ) -> FoundryResult<ServerSnapshot> {
        let endpoint = status.endpoint().map(str::to_string);

        if let Some(url) = &endpoint
            && self.wait_for_health(url).await
        {
            self.store_session(Some(status), Some(url.clone()), ownership);

            return Ok(self.snapshot());
        }

        let fresh = cli.server_status().await?;
        let endpoint = fresh.endpoint().map(str::to_string);

        if let Some(url) = endpoint
            && self.wait_for_health(&url).await
        {
            self.store_session(Some(fresh), Some(url), ownership);

            return Ok(self.snapshot());
        }

        Err(FoundryError::ServerUnreachable {
            detail: failure_detail.to_string(),
        })
    }

    fn store_session(
        &self,
        status: Option<ServerStatus>,
        endpoint: Option<String>,
        ownership: Ownership,
    ) -> u64 {
        let mut state = lock(&self.state);
        self.update_session(&mut state, status, endpoint, ownership)
    }

    fn update_session(
        &self,
        state: &mut ServerState,
        status: Option<ServerStatus>,
        endpoint: Option<String>,
        ownership: Ownership,
    ) -> u64 {
        let binding_changed = state.endpoint != endpoint
            || status.as_ref().is_some_and(|next| {
                state.status.as_ref().is_none_or(|current| {
                    current.pid != next.pid
                        || current.started_at != next.started_at
                        || current.running != next.running
                })
            });
        if binding_changed {
            self.session_revision.fetch_add(1, Ordering::SeqCst);
        }

        if let Some(status) = status {
            state.status = Some(status);
        }

        state.endpoint = endpoint;
        state.last_observed_at = Some(Instant::now());

        if let Some(started) = ownership.started_by_app() {
            state.started_by_app = started;
        }
        self.session_revision.load(Ordering::SeqCst)
    }

    async fn daemon_status(&self) -> Option<ServerStatus> {
        let path = dirs::home_dir()?.join(".foundry").join("daemon.json");
        let content = tokio::fs::read_to_string(path).await.ok()?;
        let daemon: DaemonFile = parse_json(&content).ok()?;

        for url in &daemon.web_urls {
            if health_check(&self.client, url).await {
                return Some(ServerStatus {
                    running: true,
                    state: Some("ready".to_string()),
                    pid: daemon.pid,
                    web_urls: vec![url.clone()],
                    started_at: daemon.started_at.clone(),
                    log_file: daemon.log_file.clone(),
                    ..ServerStatus::default()
                });
            }
        }

        None
    }

    async fn wait_for_health(&self, base_url: &str) -> bool {
        for attempt in 0..50 {
            if health_check(&self.client, base_url).await {
                return true;
            }

            if attempt < 49 {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        false
    }
}

fn snapshot_from(state: &ServerState) -> ServerSnapshot {
    let status = state.status.as_ref();
    let endpoint = state.endpoint.clone();
    let port = status
        .and_then(|status| status.port)
        .or_else(|| endpoint.as_deref().and_then(port_from_url));

    ServerSnapshot {
        running: endpoint.is_some(),
        endpoint,
        state: status.and_then(|status| status.state.clone()),
        pid: status.and_then(|status| status.pid),
        started_by_app: state.started_by_app,
        log_file: status.and_then(|status| status.log_file.clone()),
        port,
        uptime: status.and_then(|status| status.uptime.clone()),
    }
}

fn port_from_url(url: &str) -> Option<u16> {
    reqwest::Url::parse(url).ok()?.port()
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_preferences() {
        let preferences = ServerPreferences {
            port: Some(0),
            keep_running: true,
            connect_timeout_secs: 0,
            first_token_timeout_secs: 0,
            token_idle_timeout_secs: 0,
        }
        .normalized();

        assert_eq!(preferences.port, None);
        assert!(preferences.keep_running);
        assert_eq!(preferences.connect_timeout_secs, 1);
        assert_eq!(preferences.first_token_timeout_secs, 1);
        assert_eq!(preferences.token_idle_timeout_secs, 1);
    }

    #[test]
    fn builds_snapshot_from_status() {
        let state = ServerState {
            endpoint: Some("http://127.0.0.1:54182".to_string()),
            started_by_app: true,
            status: Some(ServerStatus {
                running: true,
                state: Some("ready".to_string()),
                pid: Some(123),
                web_urls: vec!["http://127.0.0.1:54182".to_string()],
                ..ServerStatus::default()
            }),
            ..ServerState::default()
        };

        let snapshot = snapshot_from(&state);

        assert!(snapshot.running);
        assert_eq!(snapshot.port, Some(54182));
        assert_eq!(snapshot.pid, Some(123));
        assert!(snapshot.started_by_app);
    }

    #[test]
    fn stopped_snapshot_has_no_endpoint() {
        let snapshot = snapshot_from(&ServerState::default());

        assert!(!snapshot.running);
        assert_eq!(snapshot.endpoint, None);
        assert_eq!(snapshot.port, None);
    }

    #[test]
    fn stops_only_daemons_it_started() {
        let manager = ServerManager::new(ServerPreferences::default());

        assert!(!manager.should_stop_on_exit());

        lock(&manager.state).started_by_app = true;

        assert!(manager.should_stop_on_exit());

        manager.set_preferences(ServerPreferences {
            keep_running: true,
            ..ServerPreferences::default()
        });

        assert!(!manager.should_stop_on_exit());
    }

    #[cfg(target_os = "macos")]
    struct Runtime(std::process::Child);

    #[cfg(target_os = "macos")]
    impl Drop for Runtime {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[cfg(target_os = "macos")]
    struct NativeServerFixture {
        _runtime: Runtime,
        directory: tempfile::TempDir,
        cli: CliBackend,
        status: ServerStatus,
    }

    #[cfg(target_os = "macos")]
    impl NativeServerFixture {
        fn new() -> Self {
            use std::os::unix::fs::PermissionsExt;

            let directory = tempfile::tempdir().expect("fixture installation");
            let folder = directory.path().canonicalize().expect("canonical folder");
            let executable = folder.join("foundrylocald");
            let source = folder.join("runtime.c");
            std::fs::write(&source, "#include <arpa/inet.h>\n#include <stdlib.h>\n#include <sys/socket.h>\n#include <unistd.h>\nint main(int argc, char **argv) { if (argc != 2) return 1; int fd = socket(AF_INET, SOCK_STREAM, 0); struct sockaddr_in a = {0}; a.sin_family = AF_INET; a.sin_addr.s_addr = htonl(INADDR_LOOPBACK); a.sin_port = htons(atoi(argv[1])); if (fd < 0 || bind(fd, (struct sockaddr *)&a, sizeof(a)) || listen(fd, 4)) return 2; sleep(30); close(fd); return 0; }\n").expect("fixture source");
            assert!(
                std::process::Command::new("/usr/bin/cc")
                    .arg(&source)
                    .arg("-o")
                    .arg(&executable)
                    .status()
                    .expect("fixture compiler")
                    .success()
            );
            let reserve = std::net::TcpListener::bind("127.0.0.1:0").expect("reserve port");
            let address = reserve.local_addr().expect("address");
            drop(reserve);
            let runtime = Runtime(
                std::process::Command::new(&executable)
                    .arg(address.port().to_string())
                    .stdin(std::process::Stdio::null())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .expect("controlled runtime"),
            );
            let cli_path = folder.join("foundry");
            std::fs::write(
                &cli_path,
                format!(
                    r#"#!/bin/sh
if [ "$1" = "--version" ]; then
    echo 'foundry 0.13.0'
    exit 0
fi
status=$(/bin/cat '{folder}/status.json') || exit 1
if [ -f '{folder}/gate' ]; then
    printf '%s' "$status" > '{folder}/captured.json'
    /usr/bin/touch '{folder}/ready'
    attempts=0
    while [ ! -f '{folder}/release' ]; do
        [ "$attempts" -lt 250 ] || exit 1
        /bin/sleep 0.02
        attempts=$((attempts + 1))
    done
fi
printf '%s' "$status"
"#,
                    folder = folder.display()
                ),
            )
            .expect("fixture CLI");
            std::fs::set_permissions(&cli_path, std::fs::Permissions::from_mode(0o700))
                .expect("CLI permissions");
            let cli = CliBackend::detect_from(&[cli_path]).expect("fixture CLI detection");
            let status = ServerStatus {
                running: true,
                pid: Some(runtime.0.id()),
                web_urls: vec![format!("http://{address}")],
                ..ServerStatus::default()
            };
            let fixture = Self {
                _runtime: runtime,
                directory,
                cli,
                status,
            };
            fixture.write_status();
            fixture
        }

        fn write_status(&self) {
            std::fs::write(
                self.directory.path().join("status.json"),
                serde_json::to_vec(&self.status).expect("status JSON"),
            )
            .expect("fixture status");
        }

        async fn prepare(&self, manager: &ServerManager) -> VerifiedLocalEndpoint {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match manager.verified_endpoint(&self.cli).await {
                    Ok(prepared) => {
                        prepared.revalidate().expect("native fixture proof");
                        return prepared;
                    }
                    Err(_) if Instant::now() < deadline => {
                        tokio::time::sleep(Duration::from_millis(20)).await
                    }
                    Err(error) => panic!("native fixture did not become ready: {error}"),
                }
            }
        }

        fn gate_status(&self) {
            std::fs::write(self.directory.path().join("gate"), "").expect("enable status gate");
        }

        async fn wait_for_captured_status(&self) {
            tokio::time::timeout(Duration::from_secs(5), async {
                while !self.directory.path().join("ready").exists() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .expect("CLI captured status before deadline");
            let captured: ServerStatus = serde_json::from_slice(
                &std::fs::read(self.directory.path().join("captured.json"))
                    .expect("captured status"),
            )
            .expect("captured status JSON");
            assert!(captured.running);
            assert_eq!(captured.pid, self.status.pid);
            assert_eq!(captured.endpoint(), self.status.endpoint());
        }

        fn release_status(&self) {
            std::fs::write(self.directory.path().join("release"), "").expect("release status gate");
        }
    }

    #[cfg(target_os = "macos")]
    impl Drop for NativeServerFixture {
        fn drop(&mut self) {
            let _ = std::fs::write(self.directory.path().join("release"), "");
        }
    }

    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn an_observed_stop_invalidates_prepared_context_even_while_the_process_still_listens() {
        let mut fixture = NativeServerFixture::new();
        let manager = ServerManager::new(ServerPreferences::default());
        let prepared = fixture.prepare(&manager).await;
        fixture.status.running = false;
        fixture.write_status();
        assert!(manager.verified_endpoint(&fixture.cli).await.is_err());
        assert!(!manager.snapshot().running);
        assert_eq!(
            prepared.revalidate(),
            Err(super::super::local_inference::LocalInferenceError::SessionChanged)
        );
    }

    #[tokio::test]
    #[cfg(target_os = "macos")]
    async fn an_in_flight_discovery_cannot_adopt_a_session_invalidated_by_a_stop() {
        let fixture = NativeServerFixture::new();
        let manager = Arc::new(ServerManager::new(ServerPreferences::default()));
        let prepared = fixture.prepare(&manager).await;
        fixture.gate_status();
        let discovery = tokio::spawn({
            let manager = manager.clone();
            let cli = fixture.cli.clone();
            async move { manager.verified_endpoint(&cli).await }
        });
        fixture.wait_for_captured_status().await;
        manager.mark_stopped();
        fixture.release_status();

        let error = tokio::time::timeout(Duration::from_secs(5), discovery)
            .await
            .expect("discovery completed before deadline")
            .expect("discovery task completed")
            .expect_err("stale discovery must not produce a verified endpoint");
        assert_eq!(error.code(), "CommandFailed");
        assert!(
            matches!(error, FoundryError::CommandFailed { message, .. } if message.contains("changed"))
        );
        assert!(!manager.snapshot().running);
        assert_eq!(manager.endpoint(), None);
        assert_eq!(
            prepared.revalidate(),
            Err(super::super::local_inference::LocalInferenceError::SessionChanged)
        );
    }

    #[tokio::test]
    #[ignore = "requires an installed foundry CLI"]
    async fn live_daemon_discovery_restart_and_lifecycle() {
        let cli = CliBackend::detect().expect("cli is installed");
        let manager = ServerManager::new(ServerPreferences::default());
        let initial = manager.observe(&cli).await.expect("daemon is observed");

        if initial.running {
            assert!(!initial.started_by_app);

            let restarted = manager.restart(&cli).await.expect("daemon restarts");
            let status = cli.server_status().await.expect("status is readable");

            assert_eq!(restarted.endpoint.as_deref(), status.endpoint());
            assert!(!restarted.started_by_app);
        } else {
            let started = manager.ensure_running(&cli).await.expect("daemon starts");

            assert!(started.running);
            assert!(started.started_by_app);

            let restarted = manager.restart(&cli).await.expect("daemon restarts");
            let status = cli.server_status().await.expect("status is readable");

            assert_eq!(restarted.endpoint.as_deref(), status.endpoint());
            assert!(restarted.started_by_app);

            manager.stop(&cli).await.expect("daemon stops");

            assert!(!manager.snapshot().running);
        }
    }

    #[tokio::test]
    #[ignore = "requires an installed foundry CLI and a running daemon"]
    async fn live_catalog_cache_and_config() {
        let cli = CliBackend::detect().expect("cli is installed");
        let status = cli.server_status().await.expect("status is readable");

        if !status.running {
            return;
        }

        let models = cli.list_models().await.expect("models are listed");
        let location = cli.cache_location().await.expect("cache path is readable");
        let config = cli.config_show().await.expect("config is readable");

        assert!(!models.is_empty());
        assert!(location.path.contains(".foundry"));
        assert!(!config.settings.is_empty());
    }
}
