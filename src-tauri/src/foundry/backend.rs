use async_trait::async_trait;
use serde_json::Value;

use super::error::FoundryResult;
use super::json::{CacheLocation, CliInfo, ConfigShow, ModelDetails, ModelSummary, ServerStatus};

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
