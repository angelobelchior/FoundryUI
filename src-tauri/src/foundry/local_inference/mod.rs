mod types;

use std::time::Duration;

pub use types::{InferenceRoute, LocalInferenceError};

#[derive(Clone)]
pub struct VerifiedLocalEndpoint {
    base: reqwest::Url,
    client: reqwest::Client,
}

impl std::fmt::Debug for VerifiedLocalEndpoint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VerifiedLocalEndpoint")
            .field("base", &self.base)
            .finish()
    }
}

impl VerifiedLocalEndpoint {
    pub fn new(endpoint: &str, connect_timeout: Duration) -> Result<Self, LocalInferenceError> {
        let base = reqwest::Url::parse(endpoint).map_err(|_| LocalInferenceError::InvalidEndpoint)?;
        ensure_loopback(&base)?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(connect_timeout)
            .build()
            .map_err(|_| LocalInferenceError::TransportUnavailable)?;
        Ok(Self { base, client })
    }

    pub async fn post_json<T: serde::Serialize + ?Sized>(
        &self,
        route: impl TryInto<InferenceRoute>,
        body: &T,
        timeout: Duration,
    ) -> Result<reqwest::Response, LocalInferenceError> {
        let url = self.url(route)?;
        self.client
            .post(url)
            .json(body)
            .timeout(timeout)
            .send()
            .await
            .map_err(|_| LocalInferenceError::RequestFailed)
    }

    pub async fn post_json_stream<T: serde::Serialize + ?Sized>(
        &self,
        route: impl TryInto<InferenceRoute>,
        body: &T,
        header_timeout: Duration,
    ) -> Result<reqwest::Response, LocalInferenceError> {
        let url = self.url(route)?;
        let request = self
            .client
            .post(url)
            .json(body)
            .build()
            .map_err(|_| LocalInferenceError::RequestFailed)?;
        tokio::time::timeout(header_timeout, self.client.execute(request))
            .await
            .map_err(|_| LocalInferenceError::RequestTimedOut)?
            .map_err(|_| LocalInferenceError::RequestFailed)
    }

    pub async fn post_multipart(
        &self,
        route: impl TryInto<InferenceRoute>,
        form: reqwest::multipart::Form,
        timeout: Duration,
    ) -> Result<reqwest::Response, LocalInferenceError> {
        let url = self.url(route)?;
        self.client
            .post(url)
            .multipart(form)
            .timeout(timeout)
            .send()
            .await
            .map_err(|_| LocalInferenceError::RequestFailed)
    }

    fn url(&self, route: impl TryInto<InferenceRoute>) -> Result<reqwest::Url, LocalInferenceError> {
        let route = route
            .try_into()
            .map_err(|_| LocalInferenceError::InvalidEndpoint)?;
        self.base
            .join(route.path())
            .map_err(|_| LocalInferenceError::InvalidEndpoint)
    }
}

fn ensure_loopback(url: &reqwest::Url) -> Result<(), LocalInferenceError> {
    let host = url.host_str().ok_or(LocalInferenceError::InvalidEndpoint)?;
    let ip: std::net::IpAddr = host
        .parse()
        .map_err(|_| LocalInferenceError::InvalidEndpoint)?;
    if !ip.is_loopback() || url.port().is_none() {
        return Err(LocalInferenceError::InvalidEndpoint);
    }
    Ok(())
}
