use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::json;

use super::{ModelRequest, ModelTurn, ToolCallStream, ToolStreamError, TurnProvider};
use crate::foundry::chat::ChatParams;
use crate::foundry::http::{StreamTimeouts, await_between_tokens};
use crate::foundry::local_inference::VerifiedLocalEndpoint;

pub struct EndpointTurnProvider {
    pub endpoint: VerifiedLocalEndpoint,
    pub model_id: String,
    pub params: Option<ChatParams>,
    pub timeouts: StreamTimeouts,
}

#[async_trait]
impl TurnProvider for EndpointTurnProvider {
    async fn next_turn(&mut self, request: ModelRequest) -> Result<ModelTurn, String> {
        let had_tools = !request.tools.is_empty();
        let result = self.send_turn(&request).await;

        if had_tools
            && matches!(&result, Err(error) if error.to_lowercase().contains("tool"))
        {
            let mut plain = request;
            plain.tools = Vec::new();

            return self.send_turn(&plain).await;
        }

        result
    }
}

impl EndpointTurnProvider {
    async fn send_turn(&self, request: &ModelRequest) -> Result<ModelTurn, String> {
        let mut payload = json!({
            "model": self.model_id,
            "messages": request.messages.clone(),
            "tools": request.tools.clone(),
            "stream": true,
            "stream_options": { "include_usage": true },
        });
        if let Some(params) = &self.params {
            params.apply(&mut payload);
        }
        let response = self
            .endpoint
            .post_json_stream("/v1/chat/completions", &payload, self.timeouts.first_token)
            .await
            .map_err(|error| format!("the local endpoint request failed: {error}"))?;
        let status = response.status();
        if !status.is_success() {
            let detail = response.text().await.unwrap_or_else(|_| status.to_string());
            return Err(format!(
                "the local endpoint returned HTTP {status}: {detail}"
            ));
        }
        let mut stream = response.bytes_stream();
        let mut parser = ToolCallStream::new();
        while let Some(chunk) = await_between_tokens(stream.next(), self.timeouts)
            .await
            .map_err(|error| error.to_string())?
        {
            let chunk = chunk.map_err(|error| error.to_string())?;
            parser
                .push(&chunk)
                .map_err(|error: ToolStreamError| error.to_string())?;
        }
        let parsed = parser.finish().map_err(|error| error.to_string())?;
        Ok(ModelTurn {
            content: parsed.content,
            reasoning: parsed.reasoning,
            calls: parsed.calls,
        })
    }
}
