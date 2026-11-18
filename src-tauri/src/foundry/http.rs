use std::future::Future;
use std::time::Duration;

use reqwest::Client;

use super::error::{FoundryError, FoundryResult};

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
pub const FIRST_TOKEN_TIMEOUT: Duration = Duration::from_secs(120);
pub const TOKEN_IDLE_TIMEOUT: Duration = Duration::from_secs(60);

const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

pub fn local_inference_error(error: super::local_inference::LocalInferenceError) -> FoundryError {
    FoundryError::CommandFailed {
        command: "local_inference".to_string(),
        message: error.to_string(),
        hint: Some("Context was not sent to an unverified destination. Restart the local Foundry runtime and retry; tool network permissions do not authorize remote inference.".to_string()),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct StreamTimeouts {
    pub connect: Duration,
    pub first_token: Duration,
    pub between_tokens: Duration,
}

impl StreamTimeouts {
    pub fn from_seconds(first_token: u64, between_tokens: u64) -> Self {
        Self {
            connect: CONNECT_TIMEOUT,
            first_token: Duration::from_secs(first_token.max(1)),
            between_tokens: Duration::from_secs(between_tokens.max(1)),
        }
    }

    pub fn with_connect_seconds(mut self, seconds: u64) -> Self {
        self.connect = Duration::from_secs(seconds.max(1));
        self
    }
}

pub fn build_client() -> Client {
    build_client_with(CONNECT_TIMEOUT)
}

pub fn build_client_with(connect: Duration) -> Client {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(connect)
        .pool_idle_timeout(Duration::from_secs(90))
        .build()
        .expect("fixed local HTTP client options")
}

pub async fn health_check(client: &Client, base_url: &str) -> bool {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));

    match client.get(url).timeout(HEALTH_TIMEOUT).send().await {
        Ok(response) => response.status().is_success(),
        Err(_) => false,
    }
}

pub async fn await_first_token<T>(
    future: impl Future<Output = T>,
    timeouts: StreamTimeouts,
) -> FoundryResult<T> {
    await_stage("first token", timeouts.first_token, future).await
}

pub async fn await_between_tokens<T>(
    future: impl Future<Output = T>,
    timeouts: StreamTimeouts,
) -> FoundryResult<T> {
    await_stage("token stream", timeouts.between_tokens, future).await
}

async fn await_stage<T>(
    stage: &str,
    limit: Duration,
    future: impl Future<Output = T>,
) -> FoundryResult<T> {
    match tokio::time::timeout(limit, future).await {
        Ok(value) => Ok(value),
        Err(_) => Err(FoundryError::Timeout {
            stage: stage.to_string(),
            seconds: limit.as_secs(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_a_timeout_with_its_stage() {
        let error = await_stage("first token", Duration::from_millis(10), async {
            tokio::time::sleep(Duration::from_millis(80)).await;
        })
        .await
        .expect_err("the future times out");

        assert_eq!(error.code(), "Timeout");
        assert!(error.to_string().contains("first token"));
    }

    #[test]
    fn builds_timeouts_from_seconds() {
        let timeouts = StreamTimeouts::from_seconds(0, 0);

        assert_eq!(timeouts.connect, CONNECT_TIMEOUT);
        assert_eq!(timeouts.first_token, Duration::from_secs(1));
        assert_eq!(timeouts.between_tokens, Duration::from_secs(1));
        assert_eq!(
            timeouts.with_connect_seconds(0).connect,
            Duration::from_secs(1)
        );
    }
}
