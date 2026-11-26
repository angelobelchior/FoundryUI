use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CliInfo {
    pub path: String,
    pub version: String,
    pub minimum_version: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    #[serde(default)]
    pub running: bool,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default, alias = "webUrls")]
    pub web_urls: Vec<String>,
    #[serde(default)]
    pub port: Option<u16>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub uptime: Option<String>,
    #[serde(default)]
    pub log_file: Option<String>,
}

impl ServerStatus {
    pub fn is_ready(&self) -> bool {
        self.running && self.endpoint().is_some()
    }

    pub fn endpoint(&self) -> Option<&str> {
        self.web_urls
            .iter()
            .map(|url| url.trim())
            .find(|url| !url.is_empty())
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct DaemonFile {
    #[serde(default)]
    pub pid: Option<u32>,
    #[serde(default, alias = "webUrls")]
    pub web_urls: Vec<String>,
    #[serde(default)]
    pub daemon_version: Option<String>,
    #[serde(default)]
    pub started_at: Option<String>,
    #[serde(default)]
    pub log_file: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelSummary {
    #[serde(default)]
    pub alias: String,
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default, rename = "type")]
    pub model_type: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub file_size_mb: Option<f64>,
    #[serde(default)]
    pub cached: bool,
    #[serde(default)]
    pub loaded: bool,
    #[serde(default)]
    pub license: Option<String>,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub supports_tool_calling: bool,
    #[serde(default)]
    pub supports_reasoning: bool,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub input_modalities: Vec<String>,
    #[serde(default)]
    pub variants: Vec<ModelVariant>,
}

pub type ModelDetails = ModelSummary;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelVariant {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub execution_provider: Option<String>,
    #[serde(default)]
    pub file_size_mb: Option<f64>,
    #[serde(default)]
    pub cached: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ModelListResponse {
    #[serde(default)]
    pub models: Vec<ModelSummary>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ModelInfoResponse {
    #[serde(default)]
    pub model: ModelDetails,
    #[serde(default, alias = "inputModalities")]
    pub input_modalities: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheLocation {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub user_set: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedModel {
    pub alias: String,
    pub id: String,
    pub display_name: Option<String>,
    pub model_type: Option<String>,
    pub device: Option<String>,
    pub file_size_mb: Option<f64>,
    pub supports_tool_calling: bool,
    pub loaded_since: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedSummary {
    pub models: Vec<LoadedModel>,
    pub total_bytes: u64,
    pub available_memory_bytes: u64,
    pub total_memory_bytes: u64,
    pub server_running: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheInfo {
    pub location: String,
    pub user_set: bool,
    pub models: Vec<ModelSummary>,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FreedSpace {
    pub alias: Option<String>,
    pub freed_bytes: u64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterModelInput {
    pub path: String,
    pub model_id: String,
    pub task: String,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub device: Option<String>,
    #[serde(default)]
    pub execution_provider: Option<String>,
    #[serde(default)]
    pub context_length: Option<u32>,
    #[serde(default)]
    pub supports_tool_calling: Option<bool>,
    #[serde(default)]
    pub supports_reasoning: Option<bool>,
}

pub fn megabytes_to_bytes(megabytes: Option<f64>) -> u64 {
    megabytes
        .filter(|value| *value > 0.0)
        .map(|value| (value * 1024.0 * 1024.0).round() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigSetting {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub value: Value,
    #[serde(default)]
    pub user_set: bool,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct ConfigShow {
    #[serde(default)]
    pub settings: Vec<ConfigSetting>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCommandResult {
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CliProblem {
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CliErrorPayload {
    #[serde(default)]
    pub error: Option<CliProblem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSnapshot {
    pub endpoint: Option<String>,
    pub running: bool,
    pub state: Option<String>,
    pub pid: Option<u32>,
    pub started_by_app: bool,
    pub log_file: Option<String>,
    pub port: Option<u16>,
    pub uptime: Option<String>,
}

pub fn parse_json<T: DeserializeOwned>(text: &str) -> Result<T, String> {
    let trimmed = text.trim();

    if trimmed.is_empty() {
        return Err("the command returned no output".to_string());
    }

    if let Ok(value) = serde_json::from_str(trimmed) {
        return Ok(value);
    }

    for offset in json_offsets(trimmed).into_iter().rev() {
        if let Ok(value) = serde_json::from_str(&trimmed[offset..]) {
            return Ok(value);
        }
    }

    Err(format!("could not parse JSON from: {}", preview(trimmed)))
}

fn json_offsets(input: &str) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut line_start = 0;

    for line in input.split_inclusive('\n') {
        let content = line.trim_start();
        let leading = line.len() - content.len();

        if content.starts_with('{') || content.starts_with('[') {
            offsets.push(line_start + leading);
        }

        line_start += line.len();
    }

    offsets
}

fn preview(text: &str) -> String {
    let single_line = text.split_whitespace().collect::<Vec<_>>().join(" ");

    if single_line.chars().count() > 240 {
        let shortened = single_line.chars().take(240).collect::<String>();
        format!("{shortened}…")
    } else {
        single_line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_server_status_from_clean_output() {
        let status: ServerStatus = parse_json(
            r#"{"running":true,"state":"ready","pid":77130,"webUrls":["http://127.0.0.1:63549"]}"#,
        )
        .expect("status parses");

        assert!(status.is_ready());
        assert_eq!(status.endpoint(), Some("http://127.0.0.1:63549"));
        assert_eq!(status.pid, Some(77130));
        assert_eq!(status.state.as_deref(), Some("ready"));
    }

    #[test]
    fn skips_leading_log_lines_before_json() {
        let status: ServerStatus = parse_json(
            "foundrylocald 0.13.0 starting (log-level=info)\n{\"running\":true,\"webUrls\":[\"http://127.0.0.1:54182\"],\"port\":54182}",
        )
        .expect("status parses after logs");

        assert!(status.is_ready());
        assert_eq!(status.port, Some(54182));
    }

    #[test]
    fn tolerates_missing_and_unknown_fields() {
        let status: ServerStatus =
            parse_json(r#"{"running":false,"state":"not_running","futureField":{"a":1}}"#)
                .expect("status parses");

        assert!(!status.is_ready());
        assert_eq!(status.endpoint(), None);
        assert_eq!(status.state.as_deref(), Some("not_running"));

        let model: ModelSummary = parse_json(
            r#"{"alias":"qwen3-0.6b","id":"qwen3-0.6b-generic-gpu:2","newCapability":true}"#,
        )
        .expect("model parses");

        assert_eq!(model.alias, "qwen3-0.6b");
        assert!(!model.cached);
        assert!(model.variants.is_empty());
    }

    #[test]
    fn parses_model_list_and_details() {
        let list: ModelListResponse = parse_json(
            r#"{"models":[{"alias":"qwen3-0.6b","id":"qwen3-0.6b-generic-gpu:2","type":"Chat","device":"Gpu","fileSizeMb":529,"cached":false,"license":"apache-2.0"},{"alias":"whisper-tiny","id":"whisper-tiny-generic-cpu:1","type":"Speech"}]}"#,
        )
        .expect("list parses");

        assert_eq!(list.models.len(), 2);
        assert_eq!(list.models[0].file_size_mb, Some(529.0));
        assert_eq!(list.models[1].model_type.as_deref(), Some("Speech"));

        let info: ModelInfoResponse = parse_json(
            r#"{"model":{"alias":"qwen3-0.6b","id":"qwen3-0.6b-generic-gpu:2","publisher":"Microsoft","variants":[{"id":"qwen3-0.6b-generic-cpu:4","device":"Cpu","executionProvider":"CPUExecutionProvider","fileSizeMb":593,"cached":false}]}}"#,
        )
        .expect("details parse");

        assert_eq!(info.model.publisher.as_deref(), Some("Microsoft"));
        assert_eq!(info.model.variants.len(), 1);
        assert_eq!(
            info.model.variants[0].execution_provider.as_deref(),
            Some("CPUExecutionProvider")
        );
    }

    #[test]
    fn parses_daemon_file() {
        let daemon: DaemonFile = parse_json(
            r#"{"pid":77130,"web_urls":["http://127.0.0.1:63549"],"daemon_version":"0.13.0","protocol_version":3}"#,
        )
        .expect("daemon file parses");

        assert_eq!(daemon.pid, Some(77130));
        assert_eq!(daemon.web_urls, vec!["http://127.0.0.1:63549"]);
        assert_eq!(daemon.daemon_version.as_deref(), Some("0.13.0"));
    }

    #[test]
    fn parses_cli_error_payload() {
        let payload: CliErrorPayload = parse_json(
            r#"{"error":{"code":"INVALID_ARGUMENT","message":"Unrecognized command","hint":"Run foundry --help"}}"#,
        )
        .expect("error payload parses");

        let problem = payload.error.expect("error is present");
        assert_eq!(problem.code, "INVALID_ARGUMENT");
        assert_eq!(problem.hint.as_deref(), Some("Run foundry --help"));
    }

    #[test]
    fn parses_config_show() {
        let config: ConfigShow = parse_json(
            r#"{"settings":[{"key":"port","value":"auto","userSet":false},{"key":"idle-timeout-minutes","value":"disabled","userSet":true}]}"#,
        )
        .expect("config parses");

        assert_eq!(config.settings.len(), 2);
        assert_eq!(config.settings[0].key, "port");
        assert!(config.settings[1].user_set);
    }

    #[test]
    fn reports_unparseable_output() {
        let error = parse_json::<ServerStatus>("no json here").expect_err("parse fails");

        assert!(error.contains("could not parse JSON"));
    }
}
