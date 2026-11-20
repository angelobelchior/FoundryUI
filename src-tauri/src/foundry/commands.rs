use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

use super::app::AppState;
use super::attachments::StagedAttachment;
use super::backend::FoundryBackend;
use super::catalog::{self, CatalogModel, ModelKind, Origin};
use super::chat::{ChatParams, GenerationContext, GenerationStart, SendReceipt, StreamEvent};
use super::cli::CliBackend;
use super::db::{
    ChatMessage, Conversation, CorpusDocument, CorpusDocumentInput, Database, EmbeddingCorpus,
    MessagePage, MessageRole, MessageStatus, ModelChange, SearchHit, TokenUsage, Transcription,
    TranscriptionParams,
};
use super::download;
use super::embeddings::{self, EmbeddingUsage};
use super::error::{FoundryError, FoundryResult};
use super::export::{self, ExportFormat, ExportLabels, ExportReceipt};
use super::jobs::{JobStatus, MODEL_JOB_EVENT, ModelJob, now_millis};
use super::json::{
    CacheInfo, CliInfo, ConfigShow, FreedSpace, LoadedModel, LoadedSummary, ModelSummary,
    RegisterModelInput, ServerSnapshot, megabytes_to_bytes,
};
use super::model_cache::{
    directory_size, model_directory_size, model_slug, prune_empty_directories,
    remove_model_directory,
};
use super::params::{self, ParamScope, ParamsSnapshot, ResolvedParams};
use super::server::ServerPreferences;
use super::speech::{self, AudioInfo, InputDevice, MicrophonePermission};

const SERVER_OBSERVE_CACHE_WINDOW: Duration = Duration::from_millis(1200);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSnapshot {
    pub server: ServerSnapshot,
    pub loaded: LoadedSummary,
    pub catalog: Vec<CatalogModel>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticsSnapshot {
    pub cli: CliInfo,
    pub server: ServerSnapshot,
    pub cache: CacheInfo,
    pub logs: String,
    pub config: ConfigShow,
    pub status: Value,
    pub loaded: LoadedSummary,
}

#[tauri::command]
pub fn cli_detect(state: State<'_, AppState>) -> FoundryResult<CliInfo> {
    let cli = state.refresh_backend()?;

    Ok(cli.cli_info())
}

#[tauri::command]
pub async fn server_status(state: State<'_, AppState>) -> FoundryResult<ServerSnapshot> {
    let cli = state.backend()?;

    state.server.observe(&cli).await
}

#[tauri::command]
pub async fn server_start(state: State<'_, AppState>) -> FoundryResult<ServerSnapshot> {
    let cli = state.backend()?;

    state.server.ensure_running(&cli).await
}

#[tauri::command]
pub async fn server_stop(state: State<'_, AppState>) -> FoundryResult<ServerSnapshot> {
    let cli = state.backend()?;

    state.server.stop(&cli).await
}

#[tauri::command]
pub async fn server_restart(state: State<'_, AppState>) -> FoundryResult<ServerSnapshot> {
    let cli = state.backend()?;

    state.server.restart(&cli).await
}

#[tauri::command]
pub async fn server_logs(state: State<'_, AppState>, lines: u32) -> FoundryResult<String> {
    let cli = state.backend()?;

    state.server.logs(&cli, lines).await
}

#[tauri::command]
pub async fn cli_status(state: State<'_, AppState>) -> FoundryResult<Value> {
    let cli = state.backend()?;

    cli.status().await
}

#[tauri::command]
pub fn server_get_preferences(state: State<'_, AppState>) -> ServerPreferences {
    state.server.preferences()
}

#[tauri::command]
pub fn server_set_preferences(
    state: State<'_, AppState>,
    preferences: ServerPreferences,
) -> ServerPreferences {
    state.server.set_preferences(preferences);

    state.server.preferences()
}

#[tauri::command]
pub async fn config_show(state: State<'_, AppState>) -> FoundryResult<ConfigShow> {
    let cli = state.backend()?;

    cli.config_show().await
}

#[tauri::command]
pub async fn config_set(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> FoundryResult<ConfigShow> {
    let cli = state.backend()?;
    let key = key.trim();

    if key.is_empty() {
        return Err(FoundryError::CommandFailed {
            command: "foundry config set".to_string(),
            message: "the config key is empty".to_string(),
            hint: None,
        });
    }

    cli.config_set(key, value.trim()).await?;

    cli.config_show().await
}

#[tauri::command]
pub async fn diagnostics_report(state: State<'_, AppState>) -> FoundryResult<String> {
    let cli = state.backend()?;

    cli.run_text(&["report"]).await
}

#[tauri::command]
pub async fn workspace_snapshot(state: State<'_, AppState>) -> FoundryResult<WorkspaceSnapshot> {
    let overlay = catalog::overlay()?;
    let cli = state.backend()?;
    let runtime = runtime_state(state.inner(), &cli).await;
    let models = cli.list_models().await?;
    let loaded_aliases = loaded_aliases_from_models(&runtime.loaded_models);

    Ok(WorkspaceSnapshot {
        server: runtime.snapshot.clone(),
        loaded: loaded_summary_from_models(
            state.inner(),
            runtime.loaded_models,
            runtime.snapshot.running,
        ),
        catalog: catalog::merge_all(models, &loaded_aliases, overlay),
    })
}

#[tauri::command]
pub async fn diagnostics_snapshot(
    state: State<'_, AppState>,
) -> FoundryResult<DiagnosticsSnapshot> {
    let cli = state.backend()?;
    let runtime = runtime_state(state.inner(), &cli).await;
    let cache = read_cache_info(&cli).await?;
    let logs = state.server.logs(&cli, 200).await?;
    let config = cli.config_show().await?;
    let status = cli.status().await?;

    Ok(DiagnosticsSnapshot {
        cli: cli.cli_info(),
        server: runtime.snapshot.clone(),
        cache,
        logs,
        config,
        status,
        loaded: loaded_summary_from_models(
            state.inner(),
            runtime.loaded_models,
            runtime.snapshot.running,
        ),
    })
}

#[tauri::command]
pub fn chat_list_conversations(
    state: State<'_, AppState>,
    archived: bool,
) -> FoundryResult<Vec<Conversation>> {
    state.database()?.list_conversations(archived)
}

#[tauri::command]
pub fn chat_create_conversation(
    state: State<'_, AppState>,
    model_alias: String,
) -> FoundryResult<Conversation> {
    require_non_empty("chat_create_conversation", "model alias", &model_alias)?;

    state.database()?.create_conversation(model_alias.trim())
}

#[tauri::command]
pub fn chat_rename_conversation(
    state: State<'_, AppState>,
    id: i64,
    title: String,
) -> FoundryResult<Conversation> {
    state.database()?.rename_conversation(id, &title)
}

#[tauri::command]
pub async fn chat_delete_conversation(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    language: Option<String>,
) -> FoundryResult<()> {
    super::lifecycle_commands::delete_conversation_with_confirmation(
        &app,
        &state,
        id,
        language.as_deref(),
    )
    .await
}

#[tauri::command]
pub fn chat_set_conversation_archived(
    state: State<'_, AppState>,
    id: i64,
    archived: bool,
) -> FoundryResult<Conversation> {
    state.database()?.set_conversation_archived(id, archived)
}

#[tauri::command]
pub fn chat_set_conversation_favorite(
    state: State<'_, AppState>,
    id: i64,
    favorite: bool,
) -> FoundryResult<Conversation> {
    state.database()?.set_conversation_favorite(id, favorite)
}

#[tauri::command]
pub fn chat_set_model(
    state: State<'_, AppState>,
    id: i64,
    model_alias: String,
) -> FoundryResult<ModelChange> {
    require_non_empty("chat_set_model", "model alias", &model_alias)?;

    state
        .database()?
        .set_conversation_model(id, model_alias.trim())
}

#[tauri::command]
pub fn chat_set_variant(
    state: State<'_, AppState>,
    id: i64,
    model_variant_id: Option<String>,
) -> FoundryResult<ModelChange> {
    state
        .database()?
        .set_conversation_variant(id, model_variant_id.as_deref())
}

#[tauri::command]
pub fn chat_list_messages(
    state: State<'_, AppState>,
    conversation_id: i64,
    before_seq: Option<i64>,
    limit: Option<u32>,
) -> FoundryResult<MessagePage> {
    state
        .database()?
        .list_messages(conversation_id, before_seq, limit)
}

#[tauri::command]
pub fn chat_conversation_usage(
    state: State<'_, AppState>,
    conversation_id: i64,
) -> FoundryResult<TokenUsage> {
    state.database()?.conversation_usage(conversation_id)
}

#[tauri::command]
pub async fn chat_send_message(
    state: State<'_, AppState>,
    conversation_id: i64,
    content: String,
    attachments: Option<Vec<String>>,
    channel: Channel<StreamEvent>,
) -> FoundryResult<SendReceipt> {
    let text = content.trim();
    let tokens = attachments.unwrap_or_default();

    if text.is_empty() && tokens.is_empty() {
        return Err(FoundryError::CommandFailed {
            command: "chat_send_message".to_string(),
            message: "the message is empty".to_string(),
            hint: None,
        });
    }

    let prepared = prepare_generation(state.inner(), conversation_id).await?;

    if !tokens.is_empty() {
        ensure_image_input(&prepared.cli, &prepared.conversation.model_alias).await?;
    }

    let staged = state.attachments()?.take(&tokens)?;
    let start = begin_generation(state.inner(), conversation_id, "chat_send_message")?;
    let user_message = prepared.database.append_message(
        conversation_id,
        MessageRole::User,
        text,
        "",
        Some(&prepared.conversation.model_alias),
        MessageStatus::Complete,
    )?;

    for attachment in &staged {
        prepared
            .database
            .add_attachment(user_message.id, "image", attachment)?;
    }

    let user_message = prepared.database.message(user_message.id)?;
    let (assistant_message, conversation) = open_reply(&prepared, conversation_id)?;

    Ok(launch_generation(
        state.inner(),
        prepared,
        start,
        user_message,
        assistant_message,
        conversation,
        Vec::new(),
        channel,
    ))
}

#[tauri::command]
pub async fn chat_regenerate(
    state: State<'_, AppState>,
    conversation_id: i64,
    channel: Channel<StreamEvent>,
) -> FoundryResult<SendReceipt> {
    let prepared = prepare_generation(state.inner(), conversation_id).await?;
    let user_message = prepared.database.latest_user_message(conversation_id)?;
    let start = begin_generation(state.inner(), conversation_id, "chat_regenerate")?;
    let removed = prepared
        .database
        .truncate_after(conversation_id, user_message.seq)?;
    let (assistant_message, conversation) = open_reply(&prepared, conversation_id)?;

    Ok(launch_generation(
        state.inner(),
        prepared,
        start,
        user_message,
        assistant_message,
        conversation,
        removed,
        channel,
    ))
}

#[tauri::command]
pub async fn chat_edit_message(
    state: State<'_, AppState>,
    message_id: i64,
    content: String,
    channel: Channel<StreamEvent>,
) -> FoundryResult<SendReceipt> {
    let text = content.trim();

    if text.is_empty() {
        return Err(FoundryError::CommandFailed {
            command: "chat_edit_message".to_string(),
            message: "the message is empty".to_string(),
            hint: None,
        });
    }

    let original = state.database()?.message(message_id)?;

    if original.role != MessageRole::User {
        return Err(FoundryError::CommandFailed {
            command: "chat_edit_message".to_string(),
            message: "only user messages can be edited".to_string(),
            hint: None,
        });
    }

    let conversation_id = original.conversation_id;
    let prepared = prepare_generation(state.inner(), conversation_id).await?;

    if !original.attachments.is_empty() {
        ensure_image_input(&prepared.cli, &prepared.conversation.model_alias).await?;
    }

    let start = begin_generation(state.inner(), conversation_id, "chat_edit_message")?;
    let (removed, user_message) = prepared.database.replace_from(
        conversation_id,
        original.seq,
        text,
        &prepared.conversation.model_alias,
        &original.attachments,
    )?;
    let (assistant_message, conversation) = open_reply(&prepared, conversation_id)?;

    Ok(launch_generation(
        state.inner(),
        prepared,
        start,
        user_message,
        assistant_message,
        conversation,
        removed,
        channel,
    ))
}

#[tauri::command]
pub fn chat_search(
    state: State<'_, AppState>,
    query: String,
    conversation_id: Option<i64>,
    limit: Option<u32>,
) -> FoundryResult<Vec<SearchHit>> {
    state
        .database()?
        .search_messages(&query, conversation_id, limit)
}

#[tauri::command]
pub fn chat_export(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: i64,
    format: ExportFormat,
    labels: ExportLabels,
) -> FoundryResult<ExportReceipt> {
    let database = state.database()?;
    let conversation = database.conversation(conversation_id)?;
    let messages = database.all_messages(conversation_id)?;
    let title = if conversation.title.trim().is_empty() {
        &labels.untitled
    } else {
        &conversation.title
    };
    let stem = export::file_stem(title, conversation_id);
    let contents = match format {
        ExportFormat::Markdown => export::markdown(&conversation, &messages, &labels),
        ExportFormat::Json => export::json(
            &conversation,
            &database.conversation_params(conversation_id)?,
            &messages,
        )?,
    };
    let directory = export_directory(&app)?;
    let path = export::write_export(&directory, &stem, format, &contents)?;

    let _ = app.opener().reveal_item_in_dir(&path);

    Ok(ExportReceipt {
        path: path.to_string_lossy().to_string(),
        bytes: contents.len() as u64,
    })
}

struct PreparedGeneration {
    database: Arc<Database>,
    cli: CliBackend,
    conversation: Conversation,
    resolved: ResolvedParams,
    chat_params: ChatParams,
    endpoint: super::local_inference::VerifiedLocalEndpoint,
    model_id: String,
    supports_reasoning: bool,
}

async fn prepare_generation(
    state: &AppState,
    conversation_id: i64,
) -> FoundryResult<PreparedGeneration> {
    let database = state.database()?;
    let conversation = database.conversation(conversation_id)?;
    let global = database.generation_defaults()?;
    let model_params = database.model_params(&conversation.model_alias)?;
    let conversation_params = database.conversation_params(conversation_id)?;
    let resolved = params::resolve(&conversation_params, &model_params, &global);
    let cli = state.backend()?;
    let endpoint = state.server.verified_endpoint(&cli).await?;
    let loaded = cli.list_loaded_models().await?;
    let requested = conversation
        .model_variant_id
        .as_deref()
        .unwrap_or(&conversation.model_alias);
    let loaded_model = loaded
        .iter()
        .find(|model| model.alias == requested || model.id == requested)
        .ok_or_else(|| FoundryError::ModelNotLoaded {
            alias: requested.to_string(),
        })?;
    let overlay = catalog::overlay()?;
    let supported_generation_params = catalog::supported_generation_params(
        loaded_model,
        Some(&conversation.model_alias),
        overlay,
    );
    let resolved = params::filter_supported(resolved, &supported_generation_params);
    let chat_params = resolved.params();
    let model_id = loaded_model.id.clone();
    let supports_reasoning = loaded_model.supports_reasoning;

    Ok(PreparedGeneration {
        database,
        cli,
        conversation,
        resolved,
        chat_params,
        endpoint,
        model_id,
        supports_reasoning,
    })
}

fn begin_generation(
    state: &AppState,
    conversation_id: i64,
    command: &str,
) -> FoundryResult<GenerationStart> {
    state
        .generations
        .begin(conversation_id)
        .ok_or_else(|| FoundryError::CommandFailed {
            command: command.to_string(),
            message: "this conversation is already generating a reply".to_string(),
            hint: None,
        })
}

fn open_reply(
    prepared: &PreparedGeneration,
    conversation_id: i64,
) -> FoundryResult<(ChatMessage, Conversation)> {
    let assistant_message = prepared.database.append_message(
        conversation_id,
        MessageRole::Assistant,
        "",
        "",
        Some(&prepared.conversation.model_alias),
        MessageStatus::Streaming,
    )?;

    prepared
        .database
        .set_message_params(assistant_message.id, &prepared.resolved)?;

    let assistant_message = prepared.database.message(assistant_message.id)?;
    let conversation = prepared.database.conversation(conversation_id)?;

    Ok((assistant_message, conversation))
}

#[allow(clippy::too_many_arguments)]
fn launch_generation(
    state: &AppState,
    prepared: PreparedGeneration,
    start: GenerationStart,
    user_message: ChatMessage,
    assistant_message: ChatMessage,
    conversation: Conversation,
    removed: Vec<i64>,
    channel: Channel<StreamEvent>,
) -> SendReceipt {
    let GenerationStart {
        request_id,
        cancellation,
        guard,
    } = start;
    let context = GenerationContext {
        request_id: request_id.clone(),
        database: prepared.database,
        endpoint: prepared.endpoint,
        model_id: prepared.model_id,
        conversation: conversation.clone(),
        user_message: user_message.clone(),
        assistant_message: assistant_message.clone(),
        params: Some(prepared.chat_params),
        supports_reasoning: prepared.supports_reasoning,
        timeouts: state.server.stream_timeouts(),
        cancellation,
        _guard: guard,
    };

    tauri::async_runtime::spawn(super::chat::run_generation(context, channel));

    SendReceipt {
        request_id,
        conversation,
        user_message,
        assistant_message,
        removed,
    }
}

fn export_directory(app: &AppHandle) -> FoundryResult<PathBuf> {
    let directory = app
        .path()
        .download_dir()
        .or_else(|_| app.path().document_dir())
        .or_else(|_| app.path().home_dir())
        .map_err(|error| FoundryError::ExportFailed {
            detail: error.to_string(),
        })?;

    std::fs::create_dir_all(&directory).map_err(|error| FoundryError::ExportFailed {
        detail: error.to_string(),
    })?;

    Ok(directory)
}

async fn ensure_image_input(cli: &CliBackend, alias: &str) -> FoundryResult<()> {
    let overlay = catalog::overlay()?;
    let model = cli.model_info(alias).await?;
    let merged = catalog::merge(model, &HashSet::new(), overlay);

    if accepts_images(&merged) {
        return Ok(());
    }

    Err(FoundryError::UnsupportedImage {
        alias: alias.to_string(),
    })
}

fn accepts_images(model: &CatalogModel) -> bool {
    if model.input_modalities.origin == Origin::NotDeclared {
        return true;
    }

    model
        .input_modalities
        .value
        .as_deref()
        .is_some_and(|modalities| modalities.iter().any(|modality| modality == "image"))
}

#[tauri::command]
pub fn params_snapshot(
    state: State<'_, AppState>,
    conversation_id: Option<i64>,
    model_alias: Option<String>,
) -> FoundryResult<ParamsSnapshot> {
    snapshot_for(&state, conversation_id, model_alias.as_deref())
}

#[tauri::command]
pub fn params_set_layer(
    state: State<'_, AppState>,
    scope: ParamScope,
    params: ChatParams,
    conversation_id: Option<i64>,
    model_alias: Option<String>,
) -> FoundryResult<ParamsSnapshot> {
    let params = params.validate()?;
    let database = state.database()?;
    let alias = clean_alias(model_alias.as_deref());

    match scope {
        ParamScope::Global => {
            database.set_generation_defaults(&params)?;
        }
        ParamScope::Model => {
            let alias = alias
                .as_deref()
                .ok_or_else(|| FoundryError::CommandFailed {
                    command: "params_set_layer".to_string(),
                    message: "a model is required to save model parameters".to_string(),
                    hint: None,
                })?;

            database.set_model_params(alias, &params)?;
        }
        ParamScope::Conversation => {
            let conversation_id = conversation_id.ok_or_else(|| FoundryError::CommandFailed {
                command: "params_set_layer".to_string(),
                message: "a conversation is required to save conversation parameters".to_string(),
                hint: None,
            })?;

            database.set_conversation_params(conversation_id, &params)?;
        }
    }

    snapshot_for(&state, conversation_id, alias.as_deref())
}

fn clean_alias(model_alias: Option<&str>) -> Option<String> {
    model_alias
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn snapshot_for(
    state: &AppState,
    conversation_id: Option<i64>,
    model_alias: Option<&str>,
) -> FoundryResult<ParamsSnapshot> {
    let database = state.database()?;
    let global = database.generation_defaults()?;
    let model = match clean_alias(model_alias) {
        Some(alias) => database.model_params(&alias)?,
        None => ChatParams::default(),
    };
    let conversation = match conversation_id {
        Some(id) => database.conversation_params(id)?,
        None => ChatParams::default(),
    };
    let resolved = params::resolve(&conversation, &model, &global);

    Ok(ParamsSnapshot {
        global,
        model,
        conversation,
        resolved,
    })
}

#[tauri::command]
pub fn attachment_store(
    state: State<'_, AppState>,
    data_url: String,
) -> FoundryResult<StagedAttachment> {
    state.attachments()?.store(&data_url)
}

#[tauri::command]
pub fn attachment_discard(state: State<'_, AppState>, tokens: Vec<String>) {
    if let Ok(store) = state.attachments() {
        store.discard(&tokens);
    }
}

#[tauri::command]
pub fn attachment_data(state: State<'_, AppState>, id: i64) -> FoundryResult<String> {
    state.database()?.attachment_data(id)
}

#[tauri::command]
pub fn chat_cancel_generation(state: State<'_, AppState>, request_id: String) -> bool {
    state.generations.cancel(&request_id)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionResult {
    pub transcription: Transcription,
    pub auto_loaded: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechStorageSettings {
    pub directory: String,
    pub default_directory: String,
    pub custom: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TranscriptionDeleteMode {
    All,
    Transcription,
    Audio,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionDeleteResult {
    pub removed: bool,
    pub transcription: Option<Transcription>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingCorpusDetails {
    pub corpus: EmbeddingCorpus,
    pub documents: Vec<CorpusDocument>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum VectorizeEvent {
    Started {
        request_id: String,
        corpus_id: i64,
        total_documents: u32,
    },
    Progress {
        request_id: String,
        corpus_id: i64,
        completed: u32,
        total_documents: u32,
        document_id: i64,
        usage: EmbeddingUsage,
    },
}

#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingUsageTotals {
    pub prompt_tokens: i64,
    pub total_tokens: i64,
}

impl EmbeddingUsageTotals {
    fn add(&mut self, usage: EmbeddingUsage) {
        self.prompt_tokens += usage.prompt_tokens.max(0);
        self.total_tokens += usage.total_tokens.max(0);
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorizeReceipt {
    pub request_id: String,
    pub corpus: EmbeddingCorpus,
    pub usage: EmbeddingUsageTotals,
    pub completed: u32,
    pub total_documents: u32,
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingSearchHit {
    pub document_id: i64,
    pub position: u32,
    pub text: String,
    pub source_path: Option<String>,
    pub source_exists: bool,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingSearchResult {
    pub usage: EmbeddingUsageTotals,
    pub hits: Vec<EmbeddingSearchHit>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingCompareResult {
    pub usage: EmbeddingUsageTotals,
    pub score: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingAnswerResult {
    pub answer: String,
    pub model_alias: String,
    pub model_id: String,
    pub auto_loaded_model: bool,
    pub search: EmbeddingSearchResult,
    pub citations: Vec<EmbeddingSearchHit>,
    pub usage: TokenUsage,
}

#[derive(Debug, Clone)]
struct AnswerModel {
    alias: String,
    model_id: String,
    auto_loaded: bool,
}

#[derive(Debug, Default, Deserialize)]
struct AnswerCompletionUsage {
    #[serde(default)]
    prompt_tokens: Option<i64>,
    #[serde(default)]
    completion_tokens: Option<i64>,
    #[serde(default)]
    total_tokens: Option<i64>,
}

#[derive(Debug, Default, Deserialize)]
struct AnswerCompletionChoice {
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    message: Option<AnswerCompletionMessage>,
}

#[derive(Debug, Default, Deserialize)]
struct AnswerCompletionMessage {
    #[serde(default)]
    content: Value,
}

#[derive(Debug, Default, Deserialize)]
struct AnswerCompletionResponse {
    #[serde(default)]
    choices: Vec<AnswerCompletionChoice>,
    #[serde(default)]
    usage: Option<AnswerCompletionUsage>,
}

#[tauri::command]
pub fn speech_get_storage_settings(
    app: AppHandle,
    state: State<'_, AppState>,
) -> FoundryResult<SpeechStorageSettings> {
    let database = state.database()?;
    let configured = database.speech_storage_directory()?;
    let custom = configured.is_some();
    let default_directory = default_speech_storage_root(&app)?;
    std::fs::create_dir_all(&default_directory).map_err(|error| FoundryError::Storage {
        detail: error.to_string(),
    })?;
    let default_directory =
        default_directory
            .canonicalize()
            .map_err(|error| FoundryError::Storage {
                detail: error.to_string(),
            })?;
    let directory = configured
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(|| default_directory.clone());

    std::fs::create_dir_all(&directory).map_err(|error| FoundryError::Storage {
        detail: error.to_string(),
    })?;

    Ok(SpeechStorageSettings {
        directory: directory.to_string_lossy().to_string(),
        default_directory: default_directory.to_string_lossy().to_string(),
        custom,
    })
}

#[tauri::command]
pub fn speech_set_storage_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    directory: Option<String>,
) -> FoundryResult<SpeechStorageSettings> {
    let candidate = directory
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    let default_directory = default_speech_storage_root(&app)?;
    std::fs::create_dir_all(&default_directory).map_err(|error| FoundryError::Storage {
        detail: error.to_string(),
    })?;
    let default_resolved =
        default_directory
            .canonicalize()
            .map_err(|error| FoundryError::Storage {
                detail: error.to_string(),
            })?;
    let target = candidate.unwrap_or_else(|| default_directory.clone());

    std::fs::create_dir_all(&target).map_err(|error| FoundryError::Storage {
        detail: error.to_string(),
    })?;

    let resolved = target
        .canonicalize()
        .map_err(|error| FoundryError::Storage {
            detail: error.to_string(),
        })?;
    let custom = resolved != default_resolved;

    let resolved_path = resolved.to_string_lossy().to_string();

    state.database()?.set_speech_storage_directory(if custom {
        Some(resolved_path.as_str())
    } else {
        None
    })?;

    Ok(SpeechStorageSettings {
        directory: resolved_path,
        default_directory: default_resolved.to_string_lossy().to_string(),
        custom,
    })
}

#[tauri::command]
pub fn speech_open_storage_folder(app: AppHandle, state: State<'_, AppState>) -> FoundryResult<()> {
    let directory = speech_storage_root(&app, &state)?;

    std::fs::create_dir_all(&directory).map_err(|error| FoundryError::Storage {
        detail: error.to_string(),
    })?;

    app.opener()
        .reveal_item_in_dir(&directory)
        .map_err(|error| FoundryError::CommandFailed {
            command: "open speech storage folder".to_string(),
            message: error.to_string(),
            hint: None,
        })
}

#[tauri::command]
pub fn speech_import(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> FoundryResult<AudioInfo> {
    let source = Path::new(path.trim());

    speech::import_audio(source, &speech_import_root(&app, &state)?)
}

#[tauri::command]
pub fn speech_list_input_devices() -> FoundryResult<Vec<InputDevice>> {
    speech::input_devices()
}

#[tauri::command]
pub fn speech_record_start(
    state: State<'_, AppState>,
    device: Option<String>,
) -> FoundryResult<i64> {
    state.recorder.start(device.as_deref())
}

#[tauri::command]
pub fn speech_record_level(state: State<'_, AppState>) -> f32 {
    state.recorder.level()
}

#[tauri::command]
pub fn speech_record_stop(app: AppHandle, state: State<'_, AppState>) -> FoundryResult<AudioInfo> {
    let recorded = state.recorder.stop()?;

    speech::write_recording(&speech_recordings_root(&app, &state)?, &recorded)
}

#[tauri::command]
pub fn speech_record_cancel(state: State<'_, AppState>) {
    state.recorder.cancel();
}

#[tauri::command]
pub fn speech_discard_audio(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> FoundryResult<()> {
    let database = state.database()?;
    let target = Path::new(path.trim());

    if database
        .transcription_paths()?
        .contains(&target.to_string_lossy().into_owned())
    {
        return Ok(());
    }

    let roots = speech_audio_roots(&app, &state)?;

    ensure_path_in_roots(target, &roots)?;

    speech::remove_audio(&audio_operation_root(target, &roots), target)
}

#[tauri::command]
pub fn speech_audio_data(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> FoundryResult<String> {
    let target = Path::new(path.trim());
    let roots = speech_audio_roots(&app, &state)?;

    ensure_path_in_roots(target, &roots)?;

    speech::read_audio_data_url(&audio_operation_root(target, &roots), target)
}

#[tauri::command]
pub fn speech_microphone_permission() -> MicrophonePermission {
    speech::microphone_permission()
}

#[tauri::command]
pub fn speech_open_microphone_settings(app: AppHandle) -> FoundryResult<()> {
    let url = if cfg!(target_os = "macos") {
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
    } else {
        "ms-settings:privacy-microphone"
    };

    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|error| FoundryError::CommandFailed {
            command: "open microphone settings".to_string(),
            message: error.to_string(),
            hint: None,
        })
}

#[tauri::command]
pub async fn speech_transcribe(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    model_alias: String,
    language: Option<String>,
    temperature: Option<f32>,
) -> FoundryResult<TranscriptionResult> {
    let alias = model_alias.trim();

    require_non_empty("speech_transcribe", "model alias", alias)?;

    let file = Path::new(path.trim());
    let roots = speech_audio_roots(&app, &state)?;

    ensure_path_in_roots(file, &roots)?;

    let cli = state.backend()?;

    state.server.ensure_running(&cli).await?;

    let endpoint = state.server.verified_endpoint(&cli).await?;
    let mut auto_loaded = false;
    let mut model = loaded_speech_model(&cli, alias).await?;

    if model.is_none() {
        cli.load(alias)
            .await
            .map_err(|error| classify_model_error(error, alias))?;
        state.jobs.mark_loaded(alias);
        auto_loaded = true;
        model = loaded_speech_model(&cli, alias).await?;
    }

    let model_id = model
        .map(|model| model.id)
        .unwrap_or_else(|| alias.to_string());
    let language = language
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let requested_language = language.clone();
    let temperature = temperature.unwrap_or_default().clamp(0.0, 1.0);
    let payload =
        speech::transcribe(&endpoint, &model_id, file, language.as_deref(), temperature).await?;
    let duration_ms = speech::probe_duration_ms(file)
        .map(|value| value.min(i64::MAX as u64) as i64)
        .or(payload.duration_ms)
        .unwrap_or_default();
    let transcription = state.database()?.insert_transcription(
        alias,
        &file.to_string_lossy(),
        payload.language.or(language).as_deref(),
        Some(&TranscriptionParams {
            requested_language,
            temperature,
        }),
        &payload.text,
        duration_ms,
    )?;

    Ok(TranscriptionResult {
        transcription,
        auto_loaded,
    })
}

#[tauri::command]
pub fn speech_list_transcriptions(
    state: State<'_, AppState>,
    archived: Option<bool>,
) -> FoundryResult<Vec<Transcription>> {
    Ok(state
        .database()?
        .list_transcriptions(archived.unwrap_or(false))?
        .into_iter()
        .map(with_audio_status)
        .collect())
}

#[tauri::command]
pub fn speech_set_transcription_favorite(
    state: State<'_, AppState>,
    id: i64,
    favorite: bool,
) -> FoundryResult<Transcription> {
    let updated = state.database()?.set_transcription_favorite(id, favorite)?;

    Ok(with_audio_status(updated))
}

#[tauri::command]
pub fn speech_set_transcription_archived(
    state: State<'_, AppState>,
    id: i64,
    archived: bool,
) -> FoundryResult<Transcription> {
    let updated = state.database()?.set_transcription_archived(id, archived)?;

    Ok(with_audio_status(updated))
}

#[tauri::command]
pub fn speech_update_transcription(
    state: State<'_, AppState>,
    id: i64,
    text: String,
) -> FoundryResult<Transcription> {
    let updated = state.database()?.update_transcription_text(id, &text)?;

    Ok(with_audio_status(updated))
}

#[tauri::command]
pub fn speech_delete_transcription(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    mode: Option<TranscriptionDeleteMode>,
) -> FoundryResult<TranscriptionDeleteResult> {
    let database = state.database()?;
    let mode = mode.unwrap_or(TranscriptionDeleteMode::All);
    let roots = speech_audio_roots(&app, &state)?;

    if matches!(mode, TranscriptionDeleteMode::Audio) {
        let transcription = database.transcription(id)?;

        if ensure_path_in_roots(Path::new(&transcription.source_path), &roots).is_ok() {
            let path = Path::new(&transcription.source_path);

            let _ = speech::remove_audio(&audio_operation_root(path, &roots), path);
        }

        return Ok(TranscriptionDeleteResult {
            removed: false,
            transcription: Some(with_audio_status(transcription)),
        });
    }

    let removed = database.delete_transcription(id)?;

    if matches!(mode, TranscriptionDeleteMode::All)
        && !database
            .transcription_paths()?
            .contains(&removed.source_path)
    {
        let path = Path::new(&removed.source_path);

        if ensure_path_in_roots(path, &roots).is_ok() {
            let _ = speech::remove_audio(&audio_operation_root(path, &roots), path);
        }
    }

    Ok(TranscriptionDeleteResult {
        removed: true,
        transcription: None,
    })
}

#[tauri::command]
pub fn speech_save_text(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> FoundryResult<ExportReceipt> {
    let transcription = state.database()?.transcription(id)?;
    let directory = export_directory(&app)?;
    let stem = speech::file_stem(Path::new(&transcription.source_path));
    let path = speech::write_text_export(&directory, &stem, &transcription.text)?;

    let _ = app.opener().reveal_item_in_dir(&path);

    Ok(ExportReceipt {
        path: path.to_string_lossy().to_string(),
        bytes: transcription.text.len() as u64,
    })
}

#[tauri::command]
pub fn speech_send_to_chat(
    state: State<'_, AppState>,
    conversation_id: Option<i64>,
    model_alias: Option<String>,
    text: String,
) -> FoundryResult<Conversation> {
    let content = text.trim();

    if content.is_empty() {
        return Err(FoundryError::CommandFailed {
            command: "speech_send_to_chat".to_string(),
            message: "the transcription is empty".to_string(),
            hint: None,
        });
    }

    let database = state.database()?;
    let conversation = match conversation_id {
        Some(id) => database.conversation(id)?,
        None => {
            let alias = model_alias
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| FoundryError::CommandFailed {
                    command: "speech_send_to_chat".to_string(),
                    message: "a chat model is required for a new conversation".to_string(),
                    hint: None,
                })?;

            database.create_conversation(alias)?
        }
    };

    database.append_message(
        conversation.id,
        MessageRole::User,
        content,
        "",
        Some(&conversation.model_alias),
        MessageStatus::Complete,
    )?;

    database.conversation(conversation.id)
}

#[tauri::command]
pub fn embeddings_list_corpora(
    state: State<'_, AppState>,
    archived: Option<bool>,
) -> FoundryResult<Vec<EmbeddingCorpus>> {
    state
        .database()?
        .list_embedding_corpora(archived.unwrap_or(false))
}

#[tauri::command]
pub fn embeddings_set_corpus_favorite(
    state: State<'_, AppState>,
    corpus_id: i64,
    favorite: bool,
) -> FoundryResult<EmbeddingCorpus> {
    state
        .database()?
        .set_embedding_corpus_favorite(corpus_id, favorite)
}

#[tauri::command]
pub fn embeddings_set_corpus_archived(
    state: State<'_, AppState>,
    corpus_id: i64,
    archived: bool,
) -> FoundryResult<EmbeddingCorpus> {
    state
        .database()?
        .set_embedding_corpus_archived(corpus_id, archived)
}

#[tauri::command]
pub fn embeddings_get_corpus(
    state: State<'_, AppState>,
    corpus_id: i64,
) -> FoundryResult<EmbeddingCorpusDetails> {
    let database = state.database()?;

    Ok(EmbeddingCorpusDetails {
        corpus: database.embedding_corpus(corpus_id)?,
        documents: database.embedding_documents(corpus_id)?,
    })
}

#[tauri::command]
pub fn embeddings_import_lines(path: String) -> FoundryResult<Vec<String>> {
    embeddings::read_document_lines(Path::new(path.trim()))
}

#[tauri::command]
pub fn embeddings_reveal_source(app: AppHandle, path: String) -> FoundryResult<bool> {
    let value = path.trim();

    if value.is_empty() {
        return Err(FoundryError::CommandFailed {
            command: "embeddings_reveal_source".to_string(),
            message: "the source path is empty".to_string(),
            hint: None,
        });
    }

    let source = Path::new(value);

    if !source.exists() {
        return Ok(false);
    }

    app.opener()
        .reveal_item_in_dir(source)
        .map_err(|error| FoundryError::CommandFailed {
            command: "embeddings_reveal_source".to_string(),
            message: error.to_string(),
            hint: None,
        })?;

    Ok(true)
}

#[tauri::command]
pub fn embeddings_create_corpus(
    state: State<'_, AppState>,
    name: String,
    model_alias: String,
    dimensions: Option<u32>,
    documents: Vec<String>,
    document_sources: Option<Vec<Option<String>>>,
) -> FoundryResult<EmbeddingCorpus> {
    let inputs = embedding_document_inputs(&documents, document_sources.as_deref());

    state.database()?.create_embedding_corpus_with_sources(
        &name,
        &model_alias,
        dimensions.unwrap_or_default(),
        &inputs,
    )
}

#[tauri::command]
pub fn embeddings_rename_corpus(
    state: State<'_, AppState>,
    corpus_id: i64,
    name: String,
) -> FoundryResult<EmbeddingCorpus> {
    state
        .database()?
        .update_embedding_corpus_name(corpus_id, &name)
}

#[tauri::command]
pub fn embeddings_replace_documents(
    state: State<'_, AppState>,
    corpus_id: i64,
    documents: Vec<String>,
    document_sources: Option<Vec<Option<String>>>,
) -> FoundryResult<EmbeddingCorpus> {
    let inputs = embedding_document_inputs(&documents, document_sources.as_deref());

    state
        .database()?
        .replace_embedding_corpus_documents_with_sources(corpus_id, &inputs)
}

#[tauri::command]
pub fn embeddings_set_model(
    state: State<'_, AppState>,
    corpus_id: i64,
    model_alias: String,
    dimensions: Option<u32>,
) -> FoundryResult<EmbeddingCorpus> {
    state.database()?.set_embedding_corpus_model(
        corpus_id,
        &model_alias,
        dimensions.unwrap_or_default(),
    )
}

#[tauri::command]
pub fn embeddings_delete_corpus(state: State<'_, AppState>, corpus_id: i64) -> FoundryResult<()> {
    state.database()?.delete_embedding_corpus(corpus_id)
}

#[tauri::command]
pub fn embeddings_cancel_vectorize(state: State<'_, AppState>, request_id: String) -> bool {
    state.vectorizations.cancel(request_id.trim())
}

#[tauri::command]
pub async fn embeddings_vectorize(
    state: State<'_, AppState>,
    corpus_id: i64,
    channel: Channel<VectorizeEvent>,
) -> FoundryResult<VectorizeReceipt> {
    let database = state.database()?;
    let corpus = database.embedding_corpus(corpus_id)?;
    let documents = database.embedding_documents(corpus.id)?;
    let total_documents = documents.len() as u32;

    if documents.is_empty() {
        return Err(FoundryError::CommandFailed {
            command: "embeddings_vectorize".to_string(),
            message: "the corpus has no documents".to_string(),
            hint: None,
        });
    }

    let cli = state.backend()?;

    state.server.ensure_running(&cli).await?;

    let endpoint = state.server.verified_endpoint(&cli).await?;
    let mut model = loaded_embedding_model(&cli, &corpus.model_alias).await?;

    if model.is_none() {
        cli.load(&corpus.model_alias)
            .await
            .map_err(|error| classify_model_error(error, &corpus.model_alias))?;
        state.jobs.mark_loaded(&corpus.model_alias);
        model = loaded_embedding_model(&cli, &corpus.model_alias).await?;
    }

    let model_id = model
        .map(|value| value.id)
        .unwrap_or_else(|| corpus.model_alias.clone());
    let request_id = format!("emb-{}-{}", corpus.id, now_millis());
    let cancellation = state.vectorizations.begin(&request_id);
    let _ = channel.send(VectorizeEvent::Started {
        request_id: request_id.clone(),
        corpus_id,
        total_documents,
    });

    let result = async {
        let mut usage = EmbeddingUsageTotals::default();
        let mut completed = 0;
        let mut dimensions = corpus.dimensions;

        for document in documents {
            if cancellation.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }

            let batch =
                embeddings::embed_texts(&endpoint, &model_id, std::slice::from_ref(&document.text))
                    .await?;
            let usage_step = batch.usage;
            let vector =
                batch
                    .vectors
                    .into_iter()
                    .next()
                    .ok_or_else(|| FoundryError::BadResponse {
                        detail: "the embeddings response has no vectors".to_string(),
                    })?;
            let normalized = embeddings::normalize_vector(&vector);

            if normalized.is_empty() {
                return Err(FoundryError::BadResponse {
                    detail: "the embeddings response has an empty vector".to_string(),
                });
            }

            let vector_dimensions = normalized.len() as u32;

            if dimensions == 0 {
                database.update_embedding_dimensions(corpus.id, vector_dimensions)?;
                dimensions = vector_dimensions;
            } else if dimensions != vector_dimensions {
                return Err(FoundryError::BadResponse {
                    detail: format!(
                        "model {} returned {} dimensions, expected {}",
                        corpus.model_alias, vector_dimensions, dimensions
                    ),
                });
            }

            let bytes = embeddings::vector_blob(&normalized);

            database.upsert_corpus_vector(corpus.id, document.id, vector_dimensions, &bytes)?;
            usage.add(usage_step);
            completed += 1;
            let _ = channel.send(VectorizeEvent::Progress {
                request_id: request_id.clone(),
                corpus_id,
                completed,
                total_documents,
                document_id: document.id,
                usage: usage_step,
            });
        }

        Ok(VectorizeReceipt {
            request_id: request_id.clone(),
            corpus: database.embedding_corpus(corpus.id)?,
            usage,
            completed,
            total_documents,
            cancelled: cancellation.load(std::sync::atomic::Ordering::SeqCst),
        })
    }
    .await;

    state.vectorizations.finish(&request_id);

    result
}

#[tauri::command]
pub async fn embeddings_search(
    state: State<'_, AppState>,
    corpus_id: i64,
    query: String,
    top_k: Option<u32>,
) -> FoundryResult<EmbeddingSearchResult> {
    let text = query.trim();

    require_non_empty("embeddings_search", "query", text)?;

    Ok(embedding_search_core(&state, corpus_id, text, top_k)
        .await?
        .1)
}

#[tauri::command]
pub async fn embeddings_answer(
    state: State<'_, AppState>,
    corpus_id: i64,
    query: String,
    top_k: Option<u32>,
) -> FoundryResult<EmbeddingAnswerResult> {
    let text = query.trim();

    require_non_empty("embeddings_answer", "query", text)?;

    let (_, search) = embedding_search_core(&state, corpus_id, text, top_k).await?;
    let cli = state.backend()?;

    state.server.ensure_running(&cli).await?;

    let endpoint = state.server.verified_endpoint(&cli).await?;
    let answer_model = resolve_answer_model(&state, &cli).await?;
    let citations = search.hits.iter().take(8).cloned().collect::<Vec<_>>();
    let (answer, usage) = request_semantic_answer(
        &endpoint,
        &answer_model.model_id,
        &answer_model.alias,
        text,
        &citations,
    )
    .await?;

    Ok(EmbeddingAnswerResult {
        answer,
        model_alias: answer_model.alias,
        model_id: answer_model.model_id,
        auto_loaded_model: answer_model.auto_loaded,
        search,
        citations,
        usage,
    })
}

#[tauri::command]
pub async fn embeddings_compare(
    state: State<'_, AppState>,
    model_alias: String,
    left: String,
    right: String,
) -> FoundryResult<EmbeddingCompareResult> {
    let alias = model_alias.trim();
    let left = left.trim();
    let right = right.trim();

    require_non_empty("embeddings_compare", "model alias", alias)?;
    require_non_empty("embeddings_compare", "left text", left)?;
    require_non_empty("embeddings_compare", "right text", right)?;

    let cli = state.backend()?;

    state.server.ensure_running(&cli).await?;

    let endpoint = state.server.verified_endpoint(&cli).await?;
    let mut model = loaded_embedding_model(&cli, alias).await?;

    if model.is_none() {
        cli.load(alias)
            .await
            .map_err(|error| classify_model_error(error, alias))?;
        state.jobs.mark_loaded(alias);
        model = loaded_embedding_model(&cli, alias).await?;
    }

    let model_id = model
        .map(|value| value.id)
        .unwrap_or_else(|| alias.to_string());
    let batch =
        embeddings::embed_texts(&endpoint, &model_id, &[left.to_string(), right.to_string()])
            .await?;

    if batch.vectors.len() < 2 {
        return Err(FoundryError::BadResponse {
            detail: "the embeddings response must contain two vectors".to_string(),
        });
    }

    let a = embeddings::normalize_vector(&batch.vectors[0]);
    let b = embeddings::normalize_vector(&batch.vectors[1]);

    Ok(EmbeddingCompareResult {
        usage: EmbeddingUsageTotals {
            prompt_tokens: batch.usage.prompt_tokens.max(0),
            total_tokens: batch.usage.total_tokens.max(0),
        },
        score: embeddings::similarity_score(&a, &b),
    })
}

async fn embedding_search_core(
    state: &AppState,
    corpus_id: i64,
    query: &str,
    top_k: Option<u32>,
) -> FoundryResult<(EmbeddingCorpus, EmbeddingSearchResult)> {
    let database = state.database()?;
    let corpus = database.embedding_corpus(corpus_id)?;
    let vectors = database.corpus_vector_records(corpus_id)?;

    if vectors.is_empty() {
        return Err(FoundryError::CommandFailed {
            command: "embeddings_search".to_string(),
            message: "vectorize the corpus before searching".to_string(),
            hint: None,
        });
    }

    let cli = state.backend()?;

    state.server.ensure_running(&cli).await?;

    let endpoint = state.server.verified_endpoint(&cli).await?;
    let mut model = loaded_embedding_model(&cli, &corpus.model_alias).await?;

    if model.is_none() {
        cli.load(&corpus.model_alias)
            .await
            .map_err(|error| classify_model_error(error, &corpus.model_alias))?;
        state.jobs.mark_loaded(&corpus.model_alias);
        model = loaded_embedding_model(&cli, &corpus.model_alias).await?;
    }

    let model_id = model
        .map(|value| value.id)
        .unwrap_or_else(|| corpus.model_alias.clone());
    let batch = embeddings::embed_texts(&endpoint, &model_id, &[query.to_string()]).await?;
    let query_vector = embeddings::normalize_vector(batch.vectors.first().ok_or_else(|| {
        FoundryError::BadResponse {
            detail: "the embeddings response has no vectors".to_string(),
        }
    })?);

    if query_vector.is_empty() {
        return Err(FoundryError::BadResponse {
            detail: "the embeddings response has an empty vector".to_string(),
        });
    }

    let mut hits: Vec<EmbeddingSearchHit> = vectors
        .into_iter()
        .filter_map(|record| {
            let vector = embeddings::vector_from_blob(&record.vector)?;

            if !record.dimensions.eq(&(query_vector.len() as u32)) {
                return None;
            }

            let document = record.document;
            let source_exists = document
                .source_path
                .as_deref()
                .map(|value| Path::new(value).exists())
                .unwrap_or(true);
            let semantic_score = embeddings::similarity_score(&query_vector, &vector);
            let lexical_score = embeddings::lexical_overlap_score(&document.text, query);
            let score = (semantic_score * 0.82 + lexical_score * 0.18).clamp(0.0, 1.0);

            Some(EmbeddingSearchHit {
                document_id: document.id,
                position: document.position,
                text: document.text,
                source_exists,
                source_path: document.source_path,
                score,
            })
        })
        .collect();

    hits.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(left.position.cmp(&right.position))
    });

    let limit = top_k.unwrap_or(5).clamp(1, 50) as usize;
    hits.truncate(limit);

    Ok((
        corpus,
        EmbeddingSearchResult {
            usage: EmbeddingUsageTotals {
                prompt_tokens: batch.usage.prompt_tokens.max(0),
                total_tokens: batch.usage.total_tokens.max(0),
            },
            hits,
        },
    ))
}

async fn resolve_answer_model(state: &AppState, cli: &CliBackend) -> FoundryResult<AnswerModel> {
    let overlay = catalog::overlay()?;
    let loaded = loaded_aliases(state, cli).await;
    let models = catalog::merge_all(cli.list_models().await?, &loaded, overlay);

    if let Some(model) = models
        .iter()
        .find(|model| model.loaded && model.kind == ModelKind::Chat)
        .or_else(|| {
            models
                .iter()
                .find(|model| model.loaded && model.kind == ModelKind::Multimodal)
        })
    {
        let model_id = resolved_model_id(model);

        return Ok(AnswerModel {
            alias: model.alias.clone(),
            model_id,
            auto_loaded: false,
        });
    }

    let Some(model) = models
        .iter()
        .find(|model| model.cached && model.kind == ModelKind::Chat)
        .or_else(|| {
            models
                .iter()
                .find(|model| model.cached && model.kind == ModelKind::Multimodal)
        })
    else {
        return Err(FoundryError::CommandFailed {
            command: "embeddings_answer".to_string(),
            message: "no cached chat model is available to synthesize an answer".to_string(),
            hint: Some("open Models, download/load a chat model and try again".to_string()),
        });
    };

    cli.load(&model.alias)
        .await
        .map_err(|error| classify_model_error(error, &model.alias))?;
    state.jobs.mark_loaded(&model.alias);

    Ok(AnswerModel {
        alias: model.alias.clone(),
        model_id: resolved_model_id(model),
        auto_loaded: true,
    })
}

async fn request_semantic_answer(
    endpoint: &super::local_inference::VerifiedLocalEndpoint,
    model_id: &str,
    model_alias: &str,
    query: &str,
    citations: &[EmbeddingSearchHit],
) -> FoundryResult<(String, TokenUsage)> {
    let context = if citations.is_empty() {
        "No relevant chunks were retrieved for this query.".to_string()
    } else {
        citations
            .iter()
            .enumerate()
            .map(|(index, hit)| {
                let source = hit.source_path.as_deref().unwrap_or("unknown source");
                let excerpt = compact_excerpt(&hit.text, 700);

                format!(
                    "[{}] score: {:.1}%\nsource: {}\ncontent: {}",
                    index + 1,
                    hit.score * 100.0,
                    source,
                    excerpt
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    };

    let prompt = format!(
        "Question:\n{}\n\nContext chunks:\n{}\n\nInstructions:\n- Answer only from the context chunks above.\n- Keep the same language as the question.\n- Prefer synthesis over quote dumping.\n- If context is insufficient, state what is missing.\n- Always cite evidence in brackets like [1], [2].\n- When the user asks for recurrence/frequency/trends, aggregate recurring entities relevant to the document domain (for example terms, concepts, requirements, events, technologies, dates or risks), normalize obvious aliases/variants, rank by relevance/frequency, and include a short evidence line per item.\n\nOutput format:\n1) Direct answer (2-4 lines).\n2) Key points (short bullets).\n3) Evidence used (bullets with citations).",
        query, context
    );
    let payload = json!({
        "model": model_id,
        "stream": false,
        "temperature": 0.1,
        "messages": [
            {
                "role": "system",
                "content": "You are a retrieval assistant. Ground every claim in the provided context chunks and avoid unsupported assumptions."
            },
            {
                "role": "user",
                "content": prompt,
            }
        ]
    });
    let response = endpoint
        .post_json("/v1/chat/completions", &payload, Duration::from_secs(120))
        .await
        .map_err(super::http::local_inference_error)?;
    let status = response.status();
    let body = response.text().await.unwrap_or_default();

    if !status.is_success() {
        if status == reqwest::StatusCode::NOT_FOUND || answer_missing_model(&body) {
            return Err(FoundryError::ModelNotLoaded {
                alias: model_alias.to_string(),
            });
        }

        return Err(FoundryError::BadResponse {
            detail: format!("HTTP {status}: {body}"),
        });
    }

    let parsed: AnswerCompletionResponse =
        serde_json::from_str(&body).map_err(|error| FoundryError::BadResponse {
            detail: format!("invalid chat completion payload: {error}"),
        })?;
    let answer = parsed
        .choices
        .iter()
        .find_map(extract_completion_text)
        .unwrap_or_default();

    if answer.trim().is_empty() {
        return Err(FoundryError::BadResponse {
            detail: "the semantic answer response is empty".to_string(),
        });
    }

    let usage = parsed.usage.unwrap_or_default();
    let prompt_tokens = usage.prompt_tokens.unwrap_or_default().max(0);
    let completion_tokens = usage.completion_tokens.unwrap_or_default().max(0);
    let total_tokens = usage
        .total_tokens
        .unwrap_or(prompt_tokens + completion_tokens)
        .max(0);

    Ok((
        answer,
        TokenUsage {
            prompt_tokens,
            completion_tokens,
            total_tokens,
        },
    ))
}

fn extract_completion_text(choice: &AnswerCompletionChoice) -> Option<String> {
    if let Some(message) = &choice.message {
        let mut parts = Vec::new();

        collect_completion_text(&message.content, &mut parts);

        if !parts.is_empty() {
            return Some(parts.join("\n").trim().to_string());
        }
    }

    choice
        .text
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn collect_completion_text(value: &Value, parts: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            let trimmed = text.trim();

            if !trimmed.is_empty() {
                parts.push(trimmed.to_string());
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_completion_text(item, parts);
            }
        }
        Value::Object(map) => {
            if let Some(text) = map.get("text") {
                collect_completion_text(text, parts);
            }

            if let Some(text) = map.get("content") {
                collect_completion_text(text, parts);
            }
        }
        _ => {}
    }
}

fn compact_excerpt(text: &str, max_chars: usize) -> String {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");

    if normalized.chars().count() <= max_chars {
        return normalized;
    }

    let mut clipped = String::new();

    for (index, ch) in normalized.chars().enumerate() {
        if index >= max_chars {
            break;
        }

        clipped.push(ch);
    }

    clipped.push('…');

    clipped
}

fn resolved_model_id(model: &CatalogModel) -> String {
    let id = model.id.trim();

    if id.is_empty() {
        return model.alias.clone();
    }

    id.to_string()
}

fn answer_missing_model(detail: &str) -> bool {
    let lower = detail.to_lowercase();

    lower.contains("not loaded")
        || lower.contains("model not found")
        || lower.contains("no model matching")
        || lower.contains("model is not loaded")
}

fn embedding_document_inputs(
    documents: &[String],
    sources: Option<&[Option<String>]>,
) -> Vec<CorpusDocumentInput> {
    documents
        .iter()
        .enumerate()
        .map(|(index, text)| CorpusDocumentInput {
            text: text.clone(),
            source_path: sources
                .and_then(|items| items.get(index))
                .cloned()
                .and_then(|value| value.map(|inner| inner.trim().to_string()))
                .filter(|value| !value.is_empty()),
        })
        .collect()
}

async fn loaded_speech_model(cli: &CliBackend, alias: &str) -> FoundryResult<Option<ModelSummary>> {
    Ok(cli
        .list_loaded_models()
        .await?
        .into_iter()
        .find(|model| model.alias == alias || model.id == alias))
}

async fn loaded_embedding_model(
    cli: &CliBackend,
    alias: &str,
) -> FoundryResult<Option<ModelSummary>> {
    Ok(cli
        .list_loaded_models()
        .await?
        .into_iter()
        .find(|model| model.alias == alias || model.id == alias))
}

fn app_data_root(app: &AppHandle) -> FoundryResult<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|error| FoundryError::Storage {
            detail: error.to_string(),
        })
}

fn default_speech_storage_root(app: &AppHandle) -> FoundryResult<PathBuf> {
    Ok(app_data_root(app)?.join("speech-audio"))
}

fn speech_storage_root(app: &AppHandle, state: &State<'_, AppState>) -> FoundryResult<PathBuf> {
    let configured = state.database()?.speech_storage_directory()?;

    Ok(configured
        .map(PathBuf::from)
        .unwrap_or(default_speech_storage_root(app)?))
}

fn speech_import_root(app: &AppHandle, state: &State<'_, AppState>) -> FoundryResult<PathBuf> {
    Ok(speech_storage_root(app, state)?.join("imports"))
}

fn speech_recordings_root(app: &AppHandle, state: &State<'_, AppState>) -> FoundryResult<PathBuf> {
    Ok(speech_storage_root(app, state)?.join("recordings"))
}

fn speech_audio_roots(app: &AppHandle, state: &State<'_, AppState>) -> FoundryResult<Vec<PathBuf>> {
    let mut roots = vec![
        speech_import_root(app, state)?,
        speech_recordings_root(app, state)?,
        app_data_root(app)?.join("audio"),
        app_data_root(app)?.join("recordings"),
    ];

    roots.sort();
    roots.dedup();

    Ok(roots)
}

fn ensure_path_in_roots(path: &Path, roots: &[PathBuf]) -> FoundryResult<()> {
    if path.exists() {
        return Ok(());
    }

    if roots
        .iter()
        .any(|root| speech::ensure_within(root, path).is_ok())
    {
        return Ok(());
    }

    Err(FoundryError::AudioRejected {
        reason: "the audio file is outside the configured speech storage".to_string(),
    })
}

fn audio_operation_root(path: &Path, roots: &[PathBuf]) -> PathBuf {
    if let Some(root) = roots
        .iter()
        .find(|root| speech::ensure_within(root, path).is_ok())
    {
        return root.clone();
    }

    path.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| roots[0].clone())
}

fn with_audio_status(mut transcription: Transcription) -> Transcription {
    let path = Path::new(&transcription.source_path);

    transcription.audio_exists = path.exists();

    transcription
}

fn require_non_empty(command: &str, field: &str, value: &str) -> FoundryResult<()> {
    if value.trim().is_empty() {
        return Err(FoundryError::CommandFailed {
            command: command.to_string(),
            message: format!("the {field} is empty"),
            hint: None,
        });
    }

    Ok(())
}

#[tauri::command]
pub async fn catalog_list(state: State<'_, AppState>) -> FoundryResult<Vec<CatalogModel>> {
    let overlay = catalog::overlay()?;
    let cli = state.backend()?;
    let runtime = runtime_state(state.inner(), &cli).await;
    let models = cli.list_models().await?;
    let loaded = loaded_aliases_from_models(&runtime.loaded_models);

    Ok(catalog::merge_all(models, &loaded, overlay))
}

#[tauri::command]
pub async fn catalog_model(
    state: State<'_, AppState>,
    alias: String,
) -> FoundryResult<CatalogModel> {
    let overlay = catalog::overlay()?;
    let cli = state.backend()?;
    let runtime = runtime_state(state.inner(), &cli).await;
    let model = cli.model_info(&alias).await?;
    let loaded = loaded_aliases_from_models(&runtime.loaded_models);

    Ok(catalog::merge(model, &loaded, overlay))
}

#[tauri::command]
pub async fn model_download(
    app: AppHandle,
    state: State<'_, AppState>,
    alias: String,
) -> FoundryResult<ModelJob> {
    if let Some(job) = state.jobs.job(&alias)
        && job.status == JobStatus::Running
    {
        return Ok(job);
    }

    let cli = state.backend()?;
    let model = cli
        .model_info(&alias)
        .await
        .map_err(|error| classify_model_error(error, &alias))?;
    let total_bytes = megabytes_to_bytes(model.file_size_mb);

    if model.cached {
        let job = ModelJob::already_cached(&alias, total_bytes);

        state.jobs.update(job.clone());

        return Ok(job);
    }

    let location = cli.cache_location().await?;
    let Some((job, cancellation)) = state.jobs.begin_download(&alias, total_bytes) else {
        return Ok(state
            .jobs
            .job(&alias)
            .unwrap_or_else(|| ModelJob::starting_download(&alias, total_bytes)));
    };
    let jobs = state.jobs.clone();
    let cache_root = PathBuf::from(location.path);
    let alias_for_task = alias.clone();
    let job_for_task = job.clone();
    let emitter = app.clone();

    tauri::async_runtime::spawn(async move {
        download::run_download(
            download::DownloadRequest {
                cli,
                jobs,
                alias: alias_for_task,
                model,
                cache_root,
                job: job_for_task,
                cancellation,
            },
            move |update| {
                let _ = emitter.emit(MODEL_JOB_EVENT, update);
            },
        )
        .await;
    });

    Ok(job)
}

#[tauri::command]
pub fn model_download_cancel(state: State<'_, AppState>, alias: String) -> bool {
    state.jobs.cancel(&alias)
}

#[tauri::command]
pub fn model_jobs(state: State<'_, AppState>) -> Vec<ModelJob> {
    state.jobs.jobs()
}

#[tauri::command]
pub async fn loaded_models(state: State<'_, AppState>) -> FoundryResult<LoadedSummary> {
    let cli = state.backend()?;

    loaded_summary(&state, &cli).await
}

#[tauri::command]
pub async fn model_load(state: State<'_, AppState>, alias: String) -> FoundryResult<LoadedSummary> {
    let cli = state.backend()?;

    state.server.ensure_running(&cli).await?;
    cli.load(&alias)
        .await
        .map_err(|error| classify_model_error(error, &alias))?;
    state.jobs.mark_loaded(&alias);

    loaded_summary(&state, &cli).await
}

#[tauri::command]
pub async fn model_unload(
    state: State<'_, AppState>,
    alias: String,
) -> FoundryResult<LoadedSummary> {
    let cli = state.backend()?;

    cli.unload(&alias)
        .await
        .map_err(|error| classify_model_error(error, &alias))?;
    state.jobs.clear_loaded(&alias);

    loaded_summary(&state, &cli).await
}

#[tauri::command]
pub async fn model_remove(state: State<'_, AppState>, alias: String) -> FoundryResult<FreedSpace> {
    let cli = state.backend()?;
    let cached = cli.cache_list().await?;
    let target = cached
        .into_iter()
        .find(|model| model.alias == alias || model.id == alias);
    let location = cli.cache_location().await.ok();
    let freed_bytes = target
        .as_ref()
        .map(|model| {
            location
                .as_ref()
                .map(|location| {
                    model_directory_size(Path::new(&location.path), &model_slug(&model.id))
                })
                .filter(|bytes| *bytes > 0)
                .unwrap_or_else(|| megabytes_to_bytes(model.file_size_mb))
        })
        .unwrap_or_default();

    cli.cache_clear(Some(&alias))
        .await
        .map_err(|error| classify_model_error(error, &alias))?;
    state.jobs.clear_loaded(&alias);

    if let Some(target) = target
        && let Some(location) = location
    {
        remove_model_directory(Path::new(&location.path), &model_slug(&target.id));
    }

    Ok(FreedSpace {
        alias: Some(alias),
        freed_bytes,
    })
}

#[tauri::command]
pub async fn cache_info(state: State<'_, AppState>) -> FoundryResult<CacheInfo> {
    let cli = state.backend()?;

    read_cache_info(&cli).await
}

#[tauri::command]
pub async fn cache_clear_all(state: State<'_, AppState>) -> FoundryResult<FreedSpace> {
    let cli = state.backend()?;
    let location = cli.cache_location().await?;
    let measured = directory_size(Path::new(&location.path));
    let freed_bytes = if measured > 0 {
        measured
    } else {
        cli.cache_list()
            .await?
            .iter()
            .map(|model| megabytes_to_bytes(model.file_size_mb))
            .sum()
    };

    cli.cache_clear(None)
        .await
        .map_err(|error| classify_model_error(error, "cache"))?;
    prune_empty_directories(Path::new(&location.path));

    Ok(FreedSpace {
        alias: None,
        freed_bytes,
    })
}

#[tauri::command]
pub async fn cache_set_directory(
    state: State<'_, AppState>,
    path: String,
) -> FoundryResult<CacheInfo> {
    let cli = state.backend()?;
    let path = path.trim();

    if path.is_empty() {
        return Err(FoundryError::CommandFailed {
            command: "foundry cache cd".to_string(),
            message: "the cache directory is empty".to_string(),
            hint: None,
        });
    }

    cli.cache_set_directory(path).await?;

    read_cache_info(&cli).await
}

#[tauri::command]
pub async fn local_models(state: State<'_, AppState>) -> FoundryResult<Vec<ModelSummary>> {
    let cli = state.backend()?;

    local_models_for(&cli).await
}

#[tauri::command]
pub async fn model_register(
    state: State<'_, AppState>,
    input: RegisterModelInput,
) -> FoundryResult<Vec<ModelSummary>> {
    let args = register_args(&input)?;
    let cli = state.backend()?;
    let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();

    cli.run_text(&borrowed)
        .await
        .map_err(|error| classify_model_error(error, &input.model_id))?;

    local_models_for(&cli).await
}

#[tauri::command]
pub async fn model_unregister(
    state: State<'_, AppState>,
    model: String,
) -> FoundryResult<Vec<ModelSummary>> {
    let target = model.trim();

    if target.is_empty() {
        return Err(FoundryError::CommandFailed {
            command: "foundry model unregister".to_string(),
            message: "the model is empty".to_string(),
            hint: None,
        });
    }

    let cli = state.backend()?;

    cli.run_text(&["model", "unregister", target, "--yes"])
        .await
        .map_err(|error| classify_model_error(error, target))?;

    local_models_for(&cli).await
}

async fn loaded_summary(state: &AppState, cli: &CliBackend) -> FoundryResult<LoadedSummary> {
    let runtime = runtime_state(state, cli).await;

    Ok(loaded_summary_from_models(
        state,
        runtime.loaded_models,
        runtime.snapshot.running,
    ))
}

fn loaded_summary_from_models(
    state: &AppState,
    models: Vec<ModelSummary>,
    server_running: bool,
) -> LoadedSummary {
    let aliases: Vec<String> = models.iter().map(|model| model.alias.clone()).collect();

    state.jobs.sync_loaded(&aliases);

    let loaded: Vec<LoadedModel> = models
        .into_iter()
        .map(|model| LoadedModel {
            loaded_since: state
                .jobs
                .loaded_at(&model.alias)
                .unwrap_or_else(super::jobs::now_millis),
            alias: model.alias,
            id: model.id,
            display_name: model.display_name,
            model_type: model.model_type,
            device: model.device,
            file_size_mb: model.file_size_mb,
        })
        .collect();
    let total_bytes = loaded
        .iter()
        .map(|model| megabytes_to_bytes(model.file_size_mb))
        .sum();
    let (available_memory_bytes, total_memory_bytes) = memory_snapshot();

    LoadedSummary {
        models: loaded,
        total_bytes,
        available_memory_bytes,
        total_memory_bytes,
        server_running,
    }
}

async fn read_cache_info(cli: &CliBackend) -> FoundryResult<CacheInfo> {
    let location = cli.cache_location().await?;
    let models = cli.cache_list().await?;
    let total_bytes = models
        .iter()
        .map(|model| megabytes_to_bytes(model.file_size_mb))
        .sum();

    Ok(CacheInfo {
        location: location.path,
        user_set: location.user_set,
        models,
        total_bytes,
    })
}

async fn local_models_for(cli: &CliBackend) -> FoundryResult<Vec<ModelSummary>> {
    let models = cli.list_models().await?;

    Ok(models
        .into_iter()
        .filter(|model| model.source.as_deref() != Some("Public"))
        .collect())
}

fn register_args(input: &RegisterModelInput) -> FoundryResult<Vec<String>> {
    let path = input.path.trim();
    let model_id = input.model_id.trim();
    let task = input.task.trim();

    if path.is_empty() || model_id.is_empty() || task.is_empty() {
        return Err(FoundryError::CommandFailed {
            command: "foundry model register".to_string(),
            message: "folder, model ID and task are required".to_string(),
            hint: None,
        });
    }

    let mut args = vec![
        "model".to_string(),
        "register".to_string(),
        path.to_string(),
        model_id.to_string(),
        "--task".to_string(),
        task.to_string(),
    ];

    push_optional(&mut args, "--display-name", input.display_name.as_deref());
    push_optional(&mut args, "--device", input.device.as_deref());
    push_optional(
        &mut args,
        "--execution-provider",
        input.execution_provider.as_deref(),
    );

    if let Some(length) = input.context_length.filter(|length| *length > 0) {
        args.push("--context-length".to_string());
        args.push(length.to_string());
    }

    if input.supports_tool_calling == Some(true) {
        args.push("--supports-tool-calling".to_string());
    }

    if input.supports_reasoning == Some(true) {
        args.push("--supports-reasoning".to_string());
    }

    Ok(args)
}

fn push_optional(args: &mut Vec<String>, flag: &str, value: Option<&str>) {
    if let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) {
        args.push(flag.to_string());
        args.push(value.to_string());
    }
}

fn classify_model_error(error: FoundryError, alias: &str) -> FoundryError {
    let FoundryError::CommandFailed {
        command,
        message,
        hint,
    } = error
    else {
        return error;
    };
    let lower = message.to_lowercase();

    if ["out of memory", "insufficient memory", "memory allocation"]
        .iter()
        .any(|needle| lower.contains(needle))
    {
        return FoundryError::OutOfMemory { detail: message };
    }

    if lower.contains("not cached") || lower.contains("not in the local cache") {
        return FoundryError::ModelNotCached {
            alias: alias.to_string(),
        };
    }

    if lower.contains("not loaded") {
        return FoundryError::ModelNotLoaded {
            alias: alias.to_string(),
        };
    }

    FoundryError::CommandFailed {
        command,
        message,
        hint,
    }
}

fn memory_snapshot() -> (u64, u64) {
    let Ok(mut system) = sysinfo::System::new() else {
        return (0, 0);
    };

    system.refresh_memory();

    (system.available_memory(), system.total_memory())
}

async fn loaded_aliases(state: &AppState, cli: &CliBackend) -> HashSet<String> {
    loaded_aliases_from_models(&runtime_state(state, cli).await.loaded_models)
}

fn loaded_aliases_from_models(models: &[ModelSummary]) -> HashSet<String> {
    models
        .iter()
        .flat_map(|model| [model.alias.clone(), model.id.clone()])
        .filter(|value| !value.is_empty())
        .collect()
}

struct RuntimeState {
    snapshot: ServerSnapshot,
    loaded_models: Vec<ModelSummary>,
}

async fn runtime_state(state: &AppState, cli: &CliBackend) -> RuntimeState {
    let snapshot = state
        .server
        .observe_cached(cli, SERVER_OBSERVE_CACHE_WINDOW)
        .await
        .unwrap_or_else(|_| state.server.snapshot());

    if !snapshot.running {
        return RuntimeState {
            snapshot,
            loaded_models: Vec::new(),
        };
    }

    RuntimeState {
        snapshot,
        loaded_models: cli.list_loaded_models().await.unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> RegisterModelInput {
        RegisterModelInput {
            path: " /models/demo ".to_string(),
            model_id: "demo:1".to_string(),
            task: "chat-completion".to_string(),
            ..RegisterModelInput::default()
        }
    }

    #[test]
    fn builds_register_arguments_without_empty_options() {
        let args = register_args(&input()).expect("arguments are valid");

        assert_eq!(
            args,
            vec![
                "model",
                "register",
                "/models/demo",
                "demo:1",
                "--task",
                "chat-completion"
            ]
        );
    }

    #[test]
    fn adds_optional_registration_metadata() {
        let mut input = input();

        input.display_name = Some(" Demo ".to_string());
        input.device = Some("gpu".to_string());
        input.context_length = Some(8192);
        input.supports_tool_calling = Some(true);

        let args = register_args(&input).expect("arguments are valid");

        assert!(
            args.windows(2)
                .any(|pair| pair == ["--display-name", "Demo"])
        );
        assert!(args.windows(2).any(|pair| pair == ["--device", "gpu"]));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--context-length", "8192"])
        );
        assert!(args.contains(&"--supports-tool-calling".to_string()));
        assert!(!args.contains(&"--supports-reasoning".to_string()));
    }

    #[test]
    fn requires_the_folder_model_and_task() {
        let mut input = input();

        input.model_id = "  ".to_string();

        assert!(register_args(&input).is_err());
    }

    #[test]
    fn maps_memory_failures_to_the_typed_error() {
        let error = FoundryError::CommandFailed {
            command: "foundry model load".to_string(),
            message: "failed to load model: out of memory".to_string(),
            hint: None,
        };

        assert_eq!(
            classify_model_error(error, "qwen3-0.6b").code(),
            "OutOfMemory"
        );
    }

    #[test]
    fn maps_missing_cache_entries_to_the_typed_error() {
        let error = FoundryError::CommandFailed {
            command: "foundry model load".to_string(),
            message: "Model 'whisper-tiny' is not cached.".to_string(),
            hint: None,
        };

        assert_eq!(
            classify_model_error(error, "whisper-tiny").code(),
            "ModelNotCached"
        );
    }
}
