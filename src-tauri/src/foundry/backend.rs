use async_trait::async_trait;
use serde_json::Value;

use super::cli::CliBackend;
use super::error::{FoundryError, FoundryResult};
use super::json::{
    CacheLocation, CliInfo, ConfigShow, ModelDetails, ModelInfoResponse, ModelListResponse,
    ModelSummary, ServerCommandResult, ServerStatus,
};

#[async_trait]
pub trait FoundryBackend: Send + Sync {
    fn cli_info(&self) -> CliInfo;
    async fn status(&self) -> FoundryResult<Value>;
    async fn server_status(&self) -> FoundryResult<ServerStatus>;
    async fn server_start(&self, port: Option<u16>) -> FoundryResult<ServerStatus>;
    async fn server_stop(&self) -> FoundryResult<()>;
    async fn server_restart(&self, port: Option<u16>) -> FoundryResult<ServerStatus>;
    async fn server_logs(&self, lines: u32) -> FoundryResult<String>;
    async fn list_models(&self) -> FoundryResult<Vec<ModelSummary>>;
    async fn list_loaded_models(&self) -> FoundryResult<Vec<ModelSummary>>;
    async fn model_info(&self, alias: &str) -> FoundryResult<ModelDetails>;
    async fn download(&self, alias: &str) -> FoundryResult<String>;
    async fn load(&self, alias: &str) -> FoundryResult<String>;
    async fn unload(&self, alias: &str) -> FoundryResult<String>;
    async fn remove(&self, alias: &str) -> FoundryResult<String>;
    async fn cache_list(&self) -> FoundryResult<Vec<ModelSummary>>;
    async fn cache_location(&self) -> FoundryResult<CacheLocation>;
    async fn cache_clear(&self, alias: Option<&str>) -> FoundryResult<String>;
    async fn cache_set_directory(&self, path: &str) -> FoundryResult<String>;
    async fn config_show(&self) -> FoundryResult<ConfigShow>;
    async fn config_set(&self, key: &str, value: &str) -> FoundryResult<()>;
    async fn config_reset(&self, key: Option<&str>) -> FoundryResult<()>;
}

#[async_trait]
impl FoundryBackend for CliBackend {
    fn cli_info(&self) -> CliInfo {
        self.info()
    }

    async fn status(&self) -> FoundryResult<Value> {
        self.run_json(&["status"]).await
    }

    async fn server_status(&self) -> FoundryResult<ServerStatus> {
        self.run_json(&["server", "status"]).await
    }

    async fn server_start(&self, port: Option<u16>) -> FoundryResult<ServerStatus> {
        self.run_server_command("start", port).await
    }

    async fn server_stop(&self) -> FoundryResult<()> {
        let result: ServerCommandResult = self.run_json(&["server", "stop"]).await?;

        if result.success {
            return Ok(());
        }

        Err(FoundryError::CommandFailed {
            command: "foundry server stop".to_string(),
            message: result
                .message
                .unwrap_or_else(|| "the server did not stop".to_string()),
            hint: None,
        })
    }

    async fn server_restart(&self, port: Option<u16>) -> FoundryResult<ServerStatus> {
        self.run_server_command("restart", port).await
    }

    async fn server_logs(&self, lines: u32) -> FoundryResult<String> {
        let count = lines.clamp(1, 500).to_string();

        self.run_text(&["server", "logs", "-n", count.as_str()])
            .await
    }

    async fn list_models(&self) -> FoundryResult<Vec<ModelSummary>> {
        let response: ModelListResponse = self.run_json(&["model", "list"]).await?;

        Ok(response.models)
    }

    async fn list_loaded_models(&self) -> FoundryResult<Vec<ModelSummary>> {
        let response: ModelListResponse = self.run_json(&["model", "list", "--loaded"]).await?;

        Ok(response.models)
    }

    async fn model_info(&self, alias: &str) -> FoundryResult<ModelDetails> {
        let response: ModelInfoResponse = self.run_json(&["model", "info", alias, "-v"]).await?;
        let mut model = response.model;

        if model.input_modalities.is_empty() {
            model.input_modalities = response.input_modalities;
        }

        Ok(model)
    }

    async fn download(&self, alias: &str) -> FoundryResult<String> {
        self.run_text(&["model", "download", alias, "--force"])
            .await
    }

    async fn load(&self, alias: &str) -> FoundryResult<String> {
        self.run_text(&["model", "load", alias]).await
    }

    async fn unload(&self, alias: &str) -> FoundryResult<String> {
        self.run_text(&["model", "unload", alias]).await
    }

    async fn remove(&self, alias: &str) -> FoundryResult<String> {
        self.run_text(&["cache", "clear", alias, "--force"]).await
    }

    async fn cache_list(&self) -> FoundryResult<Vec<ModelSummary>> {
        let response: ModelListResponse = self.run_json(&["cache", "list"]).await?;

        Ok(response.models)
    }

    async fn cache_location(&self) -> FoundryResult<CacheLocation> {
        self.run_json(&["cache", "location"]).await
    }

    async fn cache_clear(&self, alias: Option<&str>) -> FoundryResult<String> {
        let mut args = vec!["cache", "clear"];

        if let Some(alias) = alias {
            args.push(alias);
        }

        args.push("--force");

        self.run_text(&args).await
    }

    async fn cache_set_directory(&self, path: &str) -> FoundryResult<String> {
        self.run_text(&["cache", "cd", path, "--force"]).await
    }

    async fn config_show(&self) -> FoundryResult<ConfigShow> {
        self.run_json(&["config", "show"]).await
    }

    async fn config_set(&self, key: &str, value: &str) -> FoundryResult<()> {
        self.run_text(&["config", "set", key, value, "--force"])
            .await?;

        Ok(())
    }

    async fn config_reset(&self, key: Option<&str>) -> FoundryResult<()> {
        let mut args = vec!["config", "reset"];

        if let Some(key) = key {
            args.push(key);
        }

        self.run_text(&args).await?;

        Ok(())
    }
}

impl CliBackend {
    async fn run_server_command(
        &self,
        verb: &str,
        port: Option<u16>,
    ) -> FoundryResult<ServerStatus> {
        let mut args = vec!["server".to_string(), verb.to_string()];

        if let Some(port) = port {
            args.push("--port".to_string());
            args.push(port.to_string());
        }

        args.push("--idle-timeout".to_string());
        args.push("0".to_string());

        let args: Vec<&str> = args.iter().map(String::as_str).collect();

        self.run_json_env(&args, &[("ORT_DISABLE_TELEMETRY", "1")])
            .await
    }
}
