use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use foundry_local_sdk::{
    DeviceType, FoundryLocalConfig, FoundryLocalError, FoundryLocalManager, Model, ModelInfo,
    NativeErrorCode,
};
use serde_json::{json, Value};

use super::backend::FoundryBackend;
use super::error::{FoundryError, FoundryResult};
use super::json::{
    CacheLocation, CliInfo, ConfigSetting, ConfigShow, ModelDetails, ModelSummary, ModelVariant,
    ServerStatus,
};

pub const SDK_VERSION: &str = "2.3.1";
const MINIMUM_RUNTIME_VERSION: &str = "2.3.0";

#[derive(Clone)]
pub struct SdkBackend {
    manager: Arc<FoundryLocalManager>,
    library_path: String,
    cache_dir: String,
    logs_dir: String,
}

impl std::fmt::Debug for SdkBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SdkBackend")
            .field("library_path", &self.library_path)
            .field("cache_dir", &self.cache_dir)
            .field("logs_dir", &self.logs_dir)
            .finish()
    }
}

impl SdkBackend {
    pub fn create(library_path: &str, cache_dir: &str, logs_dir: &str) -> FoundryResult<Self> {
        let config = FoundryLocalConfig::new("foundryui")
            .library_path(library_path)
            .model_cache_dir(cache_dir)
            .logs_dir(logs_dir);
        let manager =
            FoundryLocalManager::create(config).map_err(|error| map_error("create", error))?;

        Ok(Self {
            manager,
            library_path: library_path.to_string(),
            cache_dir: cache_dir.to_string(),
            logs_dir: logs_dir.to_string(),
        })
    }

    pub fn detect() -> FoundryResult<Self> {
        let runtime = discover_runtime_dir().ok_or(FoundryError::CliMissing)?;
        let cache = discover_cache_dir();
        let logs = discover_logs_dir();

        Self::create(&runtime, &cache, &logs)
    }

    pub async fn model(&self, alias: &str) -> FoundryResult<Arc<Model>> {
        match self.manager.catalog().get_model(alias).await {
            Ok(model) => Ok(model),
            Err(_) => self
                .manager
                .catalog()
                .get_model_variant(alias)
                .await
                .map_err(|error| map_error("model info", error)),
        }
    }

    pub async fn list_local_models(&self) -> FoundryResult<Vec<ModelSummary>> {
        let local = self
            .manager
            .local_catalog()
            .map_err(|error| map_error("local models", error))?;
        let models = local
            .get_models()
            .await
            .map_err(|error| map_error("local models", error))?;

        models
            .iter()
            .map(|model| summary_from_model(model, false, None))
            .collect()
    }

    pub async fn register_model(
        &self,
        input: &super::json::RegisterModelInput,
    ) -> FoundryResult<()> {
        let local = self
            .manager
            .local_catalog()
            .map_err(|error| map_error("model register", error))?;
        let mut metadata = local
            .create_model_info()
            .map_err(|error| map_error("model register", error))?;

        metadata
            .set_string_property("task", input.task.trim())
            .map_err(|error| map_error("model register", error))?;

        if let Some(display_name) = input.display_name.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            metadata
                .set_string_property("display_name", display_name)
                .map_err(|error| map_error("model register", error))?;
        }

        if let Some(device) = input.device.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            metadata
                .set_string_property("device_type", device)
                .map_err(|error| map_error("model register", error))?;
        }

        if let Some(provider) = input.execution_provider.as_deref().map(str::trim).filter(|v| !v.is_empty()) {
            metadata
                .set_string_property("execution_provider", provider)
                .map_err(|error| map_error("model register", error))?;
        }

        if let Some(length) = input.context_length.filter(|length| *length > 0) {
            metadata
                .set_int_property("context_length", length as i64)
                .map_err(|error| map_error("model register", error))?;
        }

        if input.supports_tool_calling == Some(true) {
            metadata
                .set_int_property("supports_tool_calling", 1)
                .map_err(|error| map_error("model register", error))?;
        }

        if input.supports_reasoning == Some(true) {
            metadata
                .set_int_property("supports_reasoning", 1)
                .map_err(|error| map_error("model register", error))?;
        }

        local
            .register_model(input.path.trim(), input.model_id.trim(), metadata)
            .await
            .map_err(|error| map_error("model register", error))?;

        Ok(())
    }

    pub async fn unregister_model(&self, target: &str) -> FoundryResult<()> {
        let local = self
            .manager
            .local_catalog()
            .map_err(|error| map_error("model unregister", error))?;

        local
            .unregister_model(target)
            .await
            .map_err(|error| map_error("model unregister", error))
    }

    async fn loaded_keys(&self) -> FoundryResult<HashSet<String>> {
        let loaded = self
            .manager
            .catalog()
            .get_loaded_models()
            .await
            .map_err(|error| map_error("model list --loaded", error))?;

        Ok(loaded
            .iter()
            .flat_map(|model| [model.alias().to_string(), model.id().to_string()])
            .collect())
    }
}

#[async_trait]
impl FoundryBackend for SdkBackend {
    fn cli_info(&self) -> CliInfo {
        CliInfo {
            path: self.library_path.clone(),
            version: SDK_VERSION.to_string(),
            minimum_version: MINIMUM_RUNTIME_VERSION.to_string(),
        }
    }

    async fn status(&self) -> FoundryResult<Value> {
        let eps = self
            .manager
            .discover_eps()
            .map_err(|error| map_error("status", error))?;
        let urls = self
            .manager
            .urls()
            .map_err(|error| map_error("status", error))?;

        Ok(json!({
            "version": SDK_VERSION,
            "runtimeMinimum": MINIMUM_RUNTIME_VERSION,
            "webUrls": urls,
            "executionProviders": eps
                .iter()
                .map(|ep| json!({ "name": ep.name.clone(), "isRegistered": ep.is_registered }))
                .collect::<Vec<_>>(),
        }))
    }

    async fn server_status(&self) -> FoundryResult<ServerStatus> {
        let urls = self
            .manager
            .urls()
            .map_err(|error| map_error("server status", error))?;
        let running = !urls.is_empty();

        Ok(ServerStatus {
            running,
            state: Some(if running {
                "ready".to_string()
            } else {
                "not_running".to_string()
            }),
            web_urls: urls,
            ..ServerStatus::default()
        })
    }

    async fn server_start(&self, _port: Option<u16>) -> FoundryResult<ServerStatus> {
        self.manager
            .start_web_service()
            .await
            .map_err(|error| map_error("server start", error))?;
        self.server_status().await
    }

    async fn server_stop(&self) -> FoundryResult<()> {
        self.manager
            .stop_web_service()
            .await
            .map_err(|error| map_error("server stop", error))
    }

    async fn server_restart(&self, _port: Option<u16>) -> FoundryResult<ServerStatus> {
        self.manager
            .stop_web_service()
            .await
            .map_err(|error| map_error("server restart", error))?;
        self.manager
            .start_web_service()
            .await
            .map_err(|error| map_error("server restart", error))?;
        self.server_status().await
    }

    async fn server_logs(&self, lines: u32) -> FoundryResult<String> {
        read_tail(&self.logs_dir, lines as usize)
    }

    async fn list_models(&self) -> FoundryResult<Vec<ModelSummary>> {
        let keys = self.loaded_keys().await?;
        let models = self
            .manager
            .catalog()
            .get_models()
            .await
            .map_err(|error| map_error("model list", error))?;

        models
            .iter()
            .map(|model| summary_from_model(model, loaded(model, &keys), Some("Public".to_string())))
            .collect()
    }

    async fn list_loaded_models(&self) -> FoundryResult<Vec<ModelSummary>> {
        let models = self
            .manager
            .catalog()
            .get_loaded_models()
            .await
            .map_err(|error| map_error("model list --loaded", error))?;

        models
            .iter()
            .map(|model| summary_from_model(model, true, Some("Public".to_string())))
            .collect()
    }

    async fn model_info(&self, alias: &str) -> FoundryResult<ModelDetails> {
        let model = self.model(alias).await?;
        summary_from_model(&model, false, Some("Public".to_string()))
    }

    async fn download(&self, alias: &str) -> FoundryResult<String> {
        let model = self.model(alias).await?;
        model
            .download_builder()
            .run()
            .await
            .map_err(|error| map_error("model download", error))?;
        Ok(alias.to_string())
    }

    async fn load(&self, alias: &str) -> FoundryResult<String> {
        let model = self.model(alias).await?;
        model
            .load()
            .await
            .map_err(|error| map_error("model load", error))?;
        Ok(alias.to_string())
    }

    async fn unload(&self, alias: &str) -> FoundryResult<String> {
        let model = self.model(alias).await?;
        model
            .unload()
            .await
            .map_err(|error| map_error("model unload", error))?;
        Ok(alias.to_string())
    }

    async fn remove(&self, alias: &str) -> FoundryResult<String> {
        let model = self.model(alias).await?;
        model
            .remove_from_cache()
            .await
            .map_err(|error| map_error("cache clear", error))?;
        Ok(alias.to_string())
    }

    async fn cache_list(&self) -> FoundryResult<Vec<ModelSummary>> {
        let models = self
            .manager
            .catalog()
            .get_cached_models()
            .await
            .map_err(|error| map_error("cache list", error))?;

        models
            .iter()
            .map(|model| summary_from_model(model, false, Some("Public".to_string())))
            .collect()
    }

    async fn cache_location(&self) -> FoundryResult<CacheLocation> {
        Ok(CacheLocation {
            path: self.cache_dir.clone(),
            user_set: true,
        })
    }

    async fn cache_clear(&self, alias: Option<&str>) -> FoundryResult<String> {
        match alias {
            Some(alias) => {
                let model = self.model(alias).await?;
                model
                    .remove_from_cache()
                    .await
                    .map_err(|error| map_error("cache clear", error))?;
                Ok(alias.to_string())
            }
            None => {
                let models = self
                    .manager
                    .catalog()
                    .get_cached_models()
                    .await
                    .map_err(|error| map_error("cache clear", error))?;

                for model in models {
                    model
                        .remove_from_cache()
                        .await
                        .map_err(|error| map_error("cache clear", error))?;
                }

                Ok(String::new())
            }
        }
    }

    async fn cache_set_directory(&self, _path: &str) -> FoundryResult<String> {
        Err(FoundryError::CommandFailed {
            command: "foundry cache cd".to_string(),
            message: "the cache directory is fixed at startup in SDK mode".to_string(),
            hint: Some("restart the application with a new cache directory".to_string()),
        })
    }

    async fn config_show(&self) -> FoundryResult<ConfigShow> {
        Ok(ConfigShow {
            settings: vec![
                ConfigSetting {
                    key: "cache_directory".to_string(),
                    value: json!(self.cache_dir),
                    user_set: true,
                },
                ConfigSetting {
                    key: "library_path".to_string(),
                    value: json!(self.library_path),
                    user_set: true,
                },
            ],
        })
    }

    async fn config_set(&self, _key: &str, _value: &str) -> FoundryResult<()> {
        Err(FoundryError::CommandFailed {
            command: "foundry config set".to_string(),
            message: "configuration is fixed at startup in SDK mode".to_string(),
            hint: None,
        })
    }

    async fn config_reset(&self, _key: Option<&str>) -> FoundryResult<()> {
        Err(FoundryError::CommandFailed {
            command: "foundry config reset".to_string(),
            message: "configuration is fixed at startup in SDK mode".to_string(),
            hint: None,
        })
    }
}

pub(crate) fn map_error(command: &str, error: FoundryLocalError) -> FoundryError {
    if error.native_code() == Some(NativeErrorCode::OperationCancelled) {
        return FoundryError::Cancelled;
    }

    FoundryError::CommandFailed {
        command: format!("foundry-local-sdk {command}"),
        message: error.to_string(),
        hint: None,
    }
}

fn loaded(model: &Model, keys: &HashSet<String>) -> bool {
    keys.contains(model.alias()) || keys.contains(model.id())
}

fn summary_from_model(
    model: &Model,
    loaded: bool,
    source: Option<String>,
) -> FoundryResult<ModelSummary> {
    let info = model.info().map_err(|error| map_error("model info", error))?;
    let variants = model
        .variants()
        .iter()
        .map(|variant| variant.info().map(|info| variant_summary(&info)))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| map_error("variant info", error))?;
    let supports_reasoning = model
        .get_int_property("supports_reasoning", -1)
        .ok()
        .and_then(|value| match value {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        })
        .unwrap_or(false);

    Ok(model_summary(
        &info,
        loaded,
        supports_reasoning,
        source,
        variants,
    ))
}

pub(crate) fn model_summary(
    info: &ModelInfo,
    loaded: bool,
    supports_reasoning: bool,
    source: Option<String>,
    variants: Vec<ModelVariant>,
) -> ModelSummary {
    ModelSummary {
        alias: info.alias.clone(),
        id: info.id.clone(),
        display_name: info.display_name.clone(),
        model_type: task_to_model_type(info.task.as_deref()),
        device: info
            .runtime
            .as_ref()
            .and_then(|runtime| device_string(&runtime.device_type)),
        file_size_mb: info.file_size_mb.map(|value| value as f64),
        cached: info.cached,
        loaded,
        license: info.license.clone(),
        publisher: info.publisher.clone(),
        supports_tool_calling: info.supports_tool_calling.unwrap_or(false),
        supports_reasoning,
        source,
        input_modalities: split_modalities(info.input_modalities.as_deref()),
        variants,
    }
}

fn variant_summary(info: &ModelInfo) -> ModelVariant {
    ModelVariant {
        id: info.id.clone(),
        display_name: info.display_name.clone(),
        device: info
            .runtime
            .as_ref()
            .and_then(|runtime| device_string(&runtime.device_type)),
        execution_provider: info
            .runtime
            .as_ref()
            .map(|runtime| runtime.execution_provider.clone()),
        file_size_mb: info.file_size_mb.map(|value| value as f64),
        cached: info.cached,
    }
}

fn device_string(device: &DeviceType) -> Option<String> {
    match device {
        DeviceType::CPU => Some("Cpu".to_string()),
        DeviceType::GPU => Some("Gpu".to_string()),
        DeviceType::NPU => Some("Npu".to_string()),
        DeviceType::Invalid => None,
    }
}

fn task_to_model_type(task: Option<&str>) -> Option<String> {
    match task {
        Some("chat-completion") => Some("Chat".to_string()),
        Some("vision-language-chat") => Some("Multimodal".to_string()),
        Some("automatic-speech-recognition") => Some("Speech".to_string()),
        Some("embeddings") => Some("Embedding".to_string()),
        _ => None,
    }
}

fn split_modalities(value: Option<&str>) -> Vec<String> {
    value
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn discover_runtime_dir() -> Option<String> {
    if let Ok(dir) = std::env::var("FOUNDRY_LOCAL_LIB_DIR")
        && dir_has_runtime(&dir)
    {
        return Some(dir);
    }

    for pattern in [
        "/opt/homebrew/Cellar/foundrylocal/*/libexec",
        "/usr/local/Cellar/foundrylocal/*/libexec",
    ] {
        if let Some(dir) = glob_runtime_dir(pattern) {
            return Some(dir);
        }
    }

    for dir in [
        "/opt/homebrew/opt/foundrylocal/libexec",
        "/usr/local/opt/foundrylocal/libexec",
    ] {
        if dir_has_runtime(dir) {
            return Some(dir.to_string());
        }
    }

    None
}

fn glob_runtime_dir(pattern: &str) -> Option<String> {
    let (prefix, suffix) = pattern.split_once('*')?;
    let base = std::path::Path::new(prefix);
    let suffix = suffix.trim_start_matches('/');

    for entry in std::fs::read_dir(base).ok()?.flatten() {
        let candidate = entry.path().join(suffix);

        if dir_has_runtime(&candidate.to_string_lossy()) {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }

    None
}

fn dir_has_runtime(dir: &str) -> bool {
    let runtime = if cfg!(windows) {
        "foundry_local.dll"
    } else if cfg!(target_os = "macos") {
        "libfoundry_local.dylib"
    } else {
        "libfoundry_local.so"
    };

    std::path::Path::new(dir).join(runtime).is_file()
}

fn discover_cache_dir() -> String {
    let home = dirs::home_dir();
    let default = home
        .as_ref()
        .map(|home| {
            home.join(".foundry")
                .join("cache")
                .join("models")
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_default();

    let Some(config_path) = home.map(|home| home.join(".foundry").join("config.json")) else {
        return default;
    };

    let Ok(content) = std::fs::read_to_string(config_path) else {
        return default;
    };

    let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) else {
        return default;
    };

    json.get("cache_directory")
        .and_then(|value| value.as_str())
        .filter(|dir| !dir.is_empty())
        .map(str::to_string)
        .unwrap_or(default)
}

fn discover_logs_dir() -> String {
    dirs::home_dir()
        .map(|home| {
            home.join(".foundry")
                .join("logs")
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or_default()
}

fn read_tail(logs_dir: &str, lines: usize) -> FoundryResult<String> {
    let read_error = |error: std::io::Error| FoundryError::CommandFailed {
        command: "foundry server logs".to_string(),
        message: error.to_string(),
        hint: None,
    };
    let mut entries: Vec<_> = std::fs::read_dir(logs_dir)
        .map_err(read_error)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    entries.sort();

    let Some(latest) = entries.last() else {
        return Ok(String::new());
    };

    let content = std::fs::read_to_string(latest).map_err(read_error)?;

    Ok(content
        .lines()
        .rev()
        .take(lines)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use foundry_local_sdk::Runtime;

    fn sample_info() -> ModelInfo {
        ModelInfo {
            id: "qwen3-0.6b-generic-gpu:2".to_string(),
            name: "qwen3-0.6b".to_string(),
            version: 2,
            alias: "qwen3-0.6b".to_string(),
            display_name: Some("qwen3-0.6b-generic-gpu".to_string()),
            provider_type: "FoundryLocal".to_string(),
            uri: String::new(),
            model_type: "ONNX".to_string(),
            prompt_template: None,
            publisher: Some("Microsoft".to_string()),
            model_settings: None,
            license: Some("apache-2.0".to_string()),
            license_description: None,
            cached: false,
            task: Some("chat-completion".to_string()),
            runtime: Some(Runtime {
                device_type: DeviceType::GPU,
                execution_provider: "WebGPUExecutionProvider".to_string(),
            }),
            file_size_mb: Some(529),
            supports_tool_calling: Some(true),
            max_output_tokens: None,
            min_fl_version: None,
            created_at_unix: 0,
            context_length: None,
            input_modalities: Some("text".to_string()),
            output_modalities: Some("text".to_string()),
            capabilities: None,
        }
    }

    #[test]
    fn maps_known_tasks_to_model_types() {
        assert_eq!(task_to_model_type(Some("chat-completion")), Some("Chat".to_string()));
        assert_eq!(task_to_model_type(Some("vision-language-chat")), Some("Multimodal".to_string()));
        assert_eq!(task_to_model_type(Some("automatic-speech-recognition")), Some("Speech".to_string()));
        assert_eq!(task_to_model_type(Some("embeddings")), Some("Embedding".to_string()));
        assert_eq!(task_to_model_type(Some("unknown-task")), None);
        assert_eq!(task_to_model_type(None), None);
    }

    #[test]
    fn splits_and_trims_modalities() {
        assert_eq!(split_modalities(Some("text,image,audio")), vec!["text", "image", "audio"]);
        assert_eq!(split_modalities(Some("text")), vec!["text"]);
        assert_eq!(split_modalities(Some(" text , image ")), vec!["text", "image"]);
        assert_eq!(split_modalities(None), Vec::<String>::new());
    }

    #[test]
    fn maps_device_types() {
        assert_eq!(device_string(&DeviceType::CPU), Some("Cpu".to_string()));
        assert_eq!(device_string(&DeviceType::GPU), Some("Gpu".to_string()));
        assert_eq!(device_string(&DeviceType::NPU), Some("Npu".to_string()));
        assert_eq!(device_string(&DeviceType::Invalid), None);
    }

    #[test]
    fn builds_model_summary_from_sdk_info() {
        let variants = vec![variant_summary(&sample_info())];
        let summary = model_summary(&sample_info(), true, true, Some("Public".to_string()), variants);

        assert_eq!(summary.alias, "qwen3-0.6b");
        assert_eq!(summary.id, "qwen3-0.6b-generic-gpu:2");
        assert_eq!(summary.model_type.as_deref(), Some("Chat"));
        assert_eq!(summary.device.as_deref(), Some("Gpu"));
        assert_eq!(summary.file_size_mb, Some(529.0));
        assert!(summary.loaded);
        assert!(summary.supports_tool_calling);
        assert!(summary.supports_reasoning);
        assert_eq!(summary.publisher.as_deref(), Some("Microsoft"));
        assert_eq!(summary.license.as_deref(), Some("apache-2.0"));
        assert_eq!(summary.input_modalities, vec!["text"]);
        assert_eq!(summary.source.as_deref(), Some("Public"));
        assert_eq!(summary.variants.len(), 1);
        assert_eq!(summary.variants[0].execution_provider.as_deref(), Some("WebGPUExecutionProvider"));
    }

    #[test]
    fn maps_cancellation_to_the_typed_error() {
        let error = FoundryLocalError::Native {
            code: NativeErrorCode::OperationCancelled,
            message: "cancelled".to_string(),
        };

        assert_eq!(map_error("model download", error).code(), "Cancelled");
    }

    #[test]
    fn maps_other_sdk_errors_to_command_failed() {
        let error = FoundryLocalError::ModelOperation {
            reason: "not cached".to_string(),
        };
        let mapped = map_error("model load", error);

        assert_eq!(mapped.code(), "CommandFailed");
        assert!(mapped.to_string().contains("not cached"));
    }

    #[tokio::test]
    #[ignore = "requires the installed Foundry Local runtime"]
    async fn detects_and_lists_the_catalog() {
        let backend = SdkBackend::detect().expect("runtime is installed");
        let models = backend.list_models().await.expect("models are listed");

        assert!(!models.is_empty());
        assert!(models.iter().any(|model| model.alias == "qwen3-0.6b"));

        let variant = models
            .iter()
            .find(|model| model.alias == "qwen3-0.6b")
            .expect("qwen3-0.6b");
        backend
            .model(&variant.id)
            .await
            .expect("variant id resolves through the model lookup");
    }

    #[tokio::test]
    #[ignore = "requires the installed Foundry Local runtime and the cached whisper-large-v3-turbo model"]
    async fn loads_and_transcribes_via_the_web_service() {
        use std::time::Duration;

        use super::super::backend::FoundryBackend;
        use super::super::local_inference::VerifiedLocalEndpoint;
        use super::super::speech::transcribe;

        let backend = SdkBackend::detect().expect("runtime is installed");

        backend
            .load("whisper-large-v3-turbo")
            .await
            .expect("model loads");

        let loaded = backend.list_loaded_models().await.expect("loaded models");
        let model_id = loaded
            .iter()
            .find(|model| model.alias == "whisper-large-v3-turbo")
            .map(|model| model.id.clone())
            .expect("whisper id");

        let status = backend
            .server_start(None)
            .await
            .expect("web service starts");
        let endpoint = status.endpoint().expect("running endpoint").to_string();

        let directory = tempfile::tempdir().expect("temp dir");
        let file = directory.path().join("tone.wav");
        std::fs::write(&file, sine_wav(16_000, 1.0, 440.0)).expect("wav write");

        let endpoint = VerifiedLocalEndpoint::new(&endpoint, Duration::from_secs(5))
            .expect("verified endpoint");

        let result = transcribe(&endpoint, &model_id, &file, None, 0.0).await;

        backend
            .unload("whisper-large-v3-turbo")
            .await
            .expect("model unloads");
        backend.server_stop().await.expect("web service stops");

        let transcription = result.expect("transcription succeeds");
        assert!(transcription.text.len() < 1000, "a transcription payload is produced");
    }

    #[tokio::test]
    #[ignore = "requires the installed Foundry Local runtime and the cached gemma-4-e2b-it model"]
    async fn loads_and_chats_via_the_web_service() {
        use super::super::backend::FoundryBackend;

        let backend = SdkBackend::detect().expect("runtime is installed");

        backend.load("gemma-4-e2b-it").await.expect("model loads");

        let loaded = backend.list_loaded_models().await.expect("loaded models");
        let model = loaded
            .iter()
            .find(|model| model.alias == "gemma-4-e2b-it")
            .expect("gemma loaded");
        eprintln!("loaded alias={} id={}", model.alias, model.id);

        let status = backend.server_start(None).await.expect("web service");
        let endpoint = status.endpoint().expect("endpoint").to_string();

        let payload = serde_json::json!({
            "model": model.id,
            "messages": [{"role": "user", "content": "Say the word hello."}],
            "stream": true,
            "stream_options": {"include_usage": true},
            "max_tokens": 32,
        });

        let response = reqwest::Client::new()
            .post(format!("{endpoint}/v1/chat/completions"))
            .json(&payload)
            .send()
            .await
            .expect("chat request");

        eprintln!("chat status: {}", response.status());
        let body = response.text().await.unwrap_or_default();
        eprintln!("chat body: {body}");

        backend.unload("gemma-4-e2b-it").await.expect("unload");
        backend.server_stop().await.expect("stop");
    }

    #[tokio::test]
    #[ignore = "requires the installed Foundry Local runtime and the cached gemma-4-e2b-it model"]
    async fn sends_an_image_via_the_web_service() {
        use super::super::backend::FoundryBackend;

        let backend = SdkBackend::detect().expect("runtime is installed");

        backend.load("gemma-4-e2b-it").await.expect("model loads");

        let loaded = backend.list_loaded_models().await.expect("loaded models");
        let model = loaded
            .iter()
            .find(|model| model.alias == "gemma-4-e2b-it")
            .expect("gemma loaded");
        eprintln!("loaded alias={} id={}", model.alias, model.id);

        let status = backend.server_start(None).await.expect("web service");
        let endpoint = status.endpoint().expect("endpoint").to_string();
        let client = reqwest::Client::new();
        let image = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAEAAAABACAIAAAAlC+aJAAAAeUlEQVR4nO3PQQkAMAzAwAqrfxUTMxF7HINABFzm7H7dcEEDWtCAFjSgBQ1oQQNa0IAWNKAFDWhBA1rQgBY0oAUNaEEDWtCAFjSgBQ1oQQNa0IAWNKAFDWhBA1rQgBY0oAUNaEEDWtCAFjSgBQ1oQQNa0IAWNKAFj108cEE8uoIF1wAAAABJRU5ErkJggg==";

        let chat_payload = serde_json::json!({
            "model": model.id,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What color is the image? Answer in one word."},
                    {"type": "image_url", "image_url": {"url": image}}
                ]
            }],
            "stream": true,
            "stream_options": {"include_usage": true},
            "max_tokens": 32,
        });
        let response = client
            .post(format!("{endpoint}/v1/chat/completions"))
            .json(&chat_payload)
            .send()
            .await
            .expect("chat request");
        eprintln!("[chat/completions object] status: {}", response.status());
        let body = response.text().await.unwrap_or_default();
        eprintln!("[chat/completions object] body: {body}");

        let chat_string_payload = serde_json::json!({
            "model": model.id,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": "What color is the image? Answer in one word."},
                    {"type": "image_url", "image_url": image}
                ]
            }],
            "stream": true,
            "stream_options": {"include_usage": true},
            "max_tokens": 32,
        });
        let response = client
            .post(format!("{endpoint}/v1/chat/completions"))
            .json(&chat_string_payload)
            .send()
            .await
            .expect("chat request");
        eprintln!("[chat/completions string] status: {}", response.status());
        let body = response.text().await.unwrap_or_default();
        eprintln!("[chat/completions string] body: {body}");

        let responses_payload = serde_json::json!({
            "model": model.id,
            "input": [{
                "role": "user",
                "content": [
                    {"type": "input_text", "text": "What color is the image? Answer in one word."},
                    {"type": "input_image", "image_url": image}
                ]
            }],
            "stream": true,
            "max_output_tokens": 32,
        });
        let response = client
            .post(format!("{endpoint}/v1/responses"))
            .json(&responses_payload)
            .send()
            .await
            .expect("responses request");
        eprintln!("[responses] status: {}", response.status());
        let body = response.text().await.unwrap_or_default();
        eprintln!("[responses] body: {body}");

        backend.unload("gemma-4-e2b-it").await.expect("unload");
        backend.server_stop().await.expect("stop");
    }

    #[tokio::test]
    #[ignore = "requires the installed Foundry Local runtime and the cached gemma-4-e2b-it model"]
    async fn gemma_agent_turn_falls_back_to_chat_without_tools() {
        use std::time::Duration;

        use super::super::agent_runtime::{EndpointTurnProvider, ModelRequest, TurnProvider};
        use super::super::backend::FoundryBackend;
        use super::super::http::StreamTimeouts;
        use super::super::local_inference::VerifiedLocalEndpoint;

        let backend = SdkBackend::detect().expect("runtime is installed");
        backend.load("gemma-4-e2b-it").await.expect("model loads");
        let loaded = backend.list_loaded_models().await.expect("loaded models");
        let model = loaded
            .iter()
            .find(|model| model.alias == "gemma-4-e2b-it")
            .expect("gemma loaded");

        let status = backend.server_start(None).await.expect("web service");
        let endpoint = status.endpoint().expect("endpoint").to_string();
        let endpoint =
            VerifiedLocalEndpoint::new(&endpoint, Duration::from_secs(5)).expect("endpoint");

        let mut provider = EndpointTurnProvider {
            endpoint,
            model_id: model.id.clone(),
            params: None,
            timeouts: StreamTimeouts::from_seconds(60, 60),
        };

        let tools = vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a file from the workspace.",
                "parameters": {
                    "type": "object",
                    "properties": { "path": { "type": "string" } },
                    "required": ["path"]
                }
            }
        })];

        let turn = provider
            .next_turn(ModelRequest {
                messages: vec![serde_json::json!({"role": "user", "content": "Say the word hello."})],
                tools,
            })
            .await
            .expect("turn succeeds via the no-tools fallback");

        assert!(
            !turn.content.trim().is_empty(),
            "a reply is produced: {:?}",
            turn.content
        );

        backend.unload("gemma-4-e2b-it").await.expect("unload");
        backend.server_stop().await.expect("stop");
    }

    fn sine_wav(sample_rate: u32, seconds: f32, frequency: f32) -> Vec<u8> {
        let count = (sample_rate as f32 * seconds) as usize;
        let samples: Vec<i16> = (0..count)
            .map(|index| {
                let time = index as f32 / sample_rate as f32;
                let wave = (time * frequency * std::f32::consts::TAU).sin();
                (wave * 0.3 * i16::MAX as f32) as i16
            })
            .collect();
        let data_len = (count * 2) as u32;

        let mut wav = Vec::with_capacity(44 + data_len as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            wav.extend_from_slice(&sample.to_le_bytes());
        }

        wav
    }
}
