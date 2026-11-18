use std::io;

use futures_util::StreamExt;
use http_body_util::{BodyDataStream, BodyExt};
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use reqwest::ResponseBuilderExt;
use tokio::net::TcpStream;
use tokio::task::JoinHandle;
use tokio::time::{Duration, Instant};

use super::{LocalInferenceError, VerifiedLocalEndpoint, native};

struct ConnectionTask(JoinHandle<()>);

impl Drop for ConnectionTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct ResponseState {
    body: BodyDataStream<Incoming>,
    _connection: ConnectionTask,
    deadline: Option<Instant>,
}

impl VerifiedLocalEndpoint {
    pub(super) async fn dispatch(
        &self,
        request: reqwest::Request,
        header_deadline: Instant,
        body_deadline: Option<Instant>,
    ) -> Result<reqwest::Response, LocalInferenceError> {
        self.check_binding()?;
        let proof = self.clone();
        tokio::task::spawn_blocking(move || proof.revalidate())
            .await
            .map_err(|_| LocalInferenceError::RuntimeUnverified)??;
        self.check_binding()?;
        if Instant::now() >= header_deadline {
            return Err(LocalInferenceError::RequestTimedOut);
        }
        let stream = tokio::time::timeout(
            self.connect_timeout,
            TcpStream::connect(self.observation.address),
        )
        .await
        .map_err(|_| LocalInferenceError::RequestTimedOut)?
        .map_err(|_| LocalInferenceError::RequestFailed)?;
        let client_address = stream
            .local_addr()
            .map_err(|_| LocalInferenceError::RequestFailed)?;
        if stream
            .peer_addr()
            .map_err(|_| LocalInferenceError::RequestFailed)?
            != self.observation.address
        {
            return Err(LocalInferenceError::ListenerUnverified);
        }
        let proof_deadline = header_deadline.min(Instant::now() + self.connect_timeout);
        loop {
            self.check_binding()?;
            let observation = self.observation.clone();
            let identity = tokio::task::spawn_blocking(move || {
                native::verify_connection(&observation, client_address)
            })
            .await
            .map_err(|_| LocalInferenceError::RuntimeUnverified)??;
            if let Some(identity) = identity {
                if identity != self.identity {
                    return Err(LocalInferenceError::SessionChanged);
                }
                break;
            }
            if Instant::now() >= proof_deadline {
                return Err(LocalInferenceError::ListenerUnverified);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        self.check_binding()?;
        if Instant::now() >= header_deadline {
            return Err(LocalInferenceError::RequestTimedOut);
        }
        let url = request.url().clone();
        let route = request
            .url()
            .path()
            .parse::<hyper::Uri>()
            .map_err(|_| LocalInferenceError::InvalidEndpoint)?;
        let mut request: hyper::Request<reqwest::Body> = request
            .try_into()
            .map_err(|_| LocalInferenceError::RequestFailed)?;
        *request.uri_mut() = route;
        request.headers_mut().insert(
            hyper::header::HOST,
            self.observation
                .address
                .to_string()
                .parse()
                .map_err(|_| LocalInferenceError::InvalidEndpoint)?,
        );
        request.headers_mut().insert(
            hyper::header::CONNECTION,
            hyper::http::HeaderValue::from_static("close"),
        );
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|_| LocalInferenceError::RequestFailed)?;
        let connection = ConnectionTask(tokio::spawn(async move {
            let _ = connection.await;
        }));
        self.check_binding()?;
        let response = sender
            .send_request(request)
            .await
            .map_err(|_| LocalInferenceError::RequestFailed)?;
        if response.status().is_redirection() {
            return Err(LocalInferenceError::RedirectBlocked);
        }
        let (parts, body) = response.into_parts();
        let state = ResponseState {
            body: body.into_data_stream(),
            _connection: connection,
            deadline: body_deadline,
        };
        let body = futures_util::stream::unfold(Some(state), |state| async move {
            let mut state = state?;
            let next = if let Some(deadline) = state.deadline {
                match tokio::time::timeout_at(deadline, state.body.next()).await {
                    Ok(next) => next,
                    Err(_) => {
                        return Some((
                            Err(io::Error::new(
                                io::ErrorKind::TimedOut,
                                "Verified local response timed out.",
                            )),
                            None,
                        ));
                    }
                }
            } else {
                state.body.next().await
            };
            match next {
                Some(Ok(bytes)) => Some((Ok(bytes), Some(state))),
                Some(Err(_)) => Some((
                    Err(io::Error::other("Verified local response failed.")),
                    None,
                )),
                None => None,
            }
        });
        let mut response = hyper::Response::builder()
            .status(parts.status)
            .version(parts.version)
            .url(url);
        *response
            .headers_mut()
            .ok_or(LocalInferenceError::RequestFailed)? = parts.headers;
        let response = response
            .body(reqwest::Body::wrap_stream(body))
            .map_err(|_| LocalInferenceError::RequestFailed)?;
        Ok(reqwest::Response::from(response))
    }
}
