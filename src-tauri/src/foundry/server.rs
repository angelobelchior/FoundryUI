use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use reqwest::Client;
use serde::{Deserialize, Serialize};

use super::backend::FoundryBackend;
use super::error::{FoundryError, FoundryResult};
use super::http::{
    CONNECT_TIMEOUT, FIRST_TOKEN_TIMEOUT, StreamTimeouts, TOKEN_IDLE_TIMEOUT, build_client,
    health_check,
};
use super::json::{ServerSnapshot, ServerStatus};
use super::local_inference::VerifiedLocalEndpoint;
use super::sdk_backend::SdkBackend;

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

#[derive(Debug, Default)]
struct ServerState {
    endpoint: Option<String>,
    status: Option<ServerStatus>,
    last_observed_at: Option<Instant>,
}

pub struct ServerManager {
    state: Mutex<ServerState>,
    preferences: Mutex<ServerPreferences>,
    client: Client,
}

impl ServerManager {
    pub fn new(preferences: ServerPreferences) -> Self {
        Self {
            state: Mutex::new(ServerState::default()),
            preferences: Mutex::new(preferences.normalized()),
            client: build_client(),
        }
    }

    pub fn preferences(&self) -> ServerPreferences {
        lock(&self.preferences).clone()
    }

    pub fn set_preferences(&self, preferences: ServerPreferences) {
        *lock(&self.preferences) = preferences.normalized();
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

    pub async fn verified_endpoint(&self, cli: &SdkBackend) -> FoundryResult<VerifiedLocalEndpoint> {
        let snapshot = self.ensure_running(cli).await?;
        let endpoint = snapshot
            .endpoint
            .ok_or_else(|| FoundryError::ServerUnreachable {
                detail: "start the local server and try again".to_string(),
            })?;
        let connect = self.stream_timeouts().connect;

        VerifiedLocalEndpoint::new(&endpoint, connect)
            .map_err(super::http::local_inference_error)
    }

    pub fn snapshot(&self) -> ServerSnapshot {
        snapshot_from(&lock(&self.state))
    }

    pub fn should_stop_on_exit(&self) -> bool {
        false
    }

    pub fn mark_stopped(&self) {
        *lock(&self.state) = ServerState::default();
    }

    pub async fn observe_cached(
        &self,
        cli: &SdkBackend,
        max_age: Duration,
    ) -> FoundryResult<ServerSnapshot> {
        let cached_is_fresh = lock(&self.state)
            .last_observed_at
            .is_some_and(|at| at.elapsed() <= max_age);

        if cached_is_fresh {
            return Ok(self.snapshot());
        }

        self.observe(cli).await
    }

    pub async fn observe(&self, cli: &SdkBackend) -> FoundryResult<ServerSnapshot> {
        let cached_endpoint = lock(&self.state).endpoint.clone();

        if let Some(endpoint) = cached_endpoint
            && health_check(&self.client, &endpoint).await
        {
            self.store_session(Some(endpoint), None);

            return Ok(self.snapshot());
        }

        let status = cli.server_status().await?;
        let endpoint = status.endpoint().map(str::to_string);

        self.store_session(endpoint, Some(status));

        Ok(self.snapshot())
    }

    pub async fn ensure_running(&self, cli: &SdkBackend) -> FoundryResult<ServerSnapshot> {
        if let Ok(snapshot) = self.observe(cli).await
            && snapshot.running
        {
            return Ok(snapshot);
        }

        let started = cli.server_start(None).await?;

        self.adopt_after_command(cli, started).await
    }

    pub async fn stop(&self, cli: &SdkBackend) -> FoundryResult<ServerSnapshot> {
        cli.server_stop().await?;
        self.mark_stopped();

        Ok(self.snapshot())
    }

    pub async fn restart(&self, cli: &SdkBackend) -> FoundryResult<ServerSnapshot> {
        let restarted = cli.server_restart(None).await?;

        self.adopt_after_command(cli, restarted).await
    }

    pub async fn logs(&self, cli: &SdkBackend, lines: u32) -> FoundryResult<String> {
        cli.server_logs(lines).await
    }

    async fn adopt_after_command(
        &self,
        cli: &SdkBackend,
        status: ServerStatus,
    ) -> FoundryResult<ServerSnapshot> {
        let endpoint = status.endpoint().map(str::to_string);

        if let Some(url) = &endpoint
            && self.wait_for_health(url).await
        {
            self.store_session(Some(url.clone()), Some(status));

            return Ok(self.snapshot());
        }

        let fresh = cli.server_status().await?;
        let endpoint = fresh.endpoint().map(str::to_string);

        if let Some(url) = endpoint
            && self.wait_for_health(&url).await
        {
            self.store_session(Some(url), Some(fresh));

            return Ok(self.snapshot());
        }

        Err(FoundryError::ServerUnreachable {
            detail: "the local server did not answer on its advertised URL".to_string(),
        })
    }

    fn store_session(&self, endpoint: Option<String>, status: Option<ServerStatus>) {
        let mut state = lock(&self.state);

        state.endpoint = endpoint;
        if let Some(status) = status {
            state.status = Some(status);
        }
        state.last_observed_at = Some(Instant::now());
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

    ServerSnapshot {
        running: endpoint.is_some(),
        endpoint,
        state: status.and_then(|status| status.state.clone()),
        pid: None,
        started_by_app: false,
        log_file: status.and_then(|status| status.log_file.clone()),
        port: status.and_then(|status| status.port).or_else(|| {
            status
                .and_then(|status| status.endpoint())
                .and_then(port_from_url)
        }),
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
            status: Some(ServerStatus {
                running: true,
                state: Some("ready".to_string()),
                web_urls: vec!["http://127.0.0.1:54182".to_string()],
                ..ServerStatus::default()
            }),
            last_observed_at: None,
        };

        let snapshot = snapshot_from(&state);

        assert!(snapshot.running);
        assert_eq!(snapshot.port, Some(54182));
        assert!(!snapshot.started_by_app);
    }

    #[test]
    fn stopped_snapshot_has_no_endpoint() {
        let snapshot = snapshot_from(&ServerState::default());

        assert!(!snapshot.running);
        assert_eq!(snapshot.endpoint, None);
        assert_eq!(snapshot.port, None);
    }

    #[test]
    fn never_stops_on_exit() {
        let manager = ServerManager::new(ServerPreferences::default());

        assert!(!manager.should_stop_on_exit());
    }
}
