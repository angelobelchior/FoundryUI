use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use super::chat::GenerationParam;
use super::error::{FoundryError, FoundryResult};
use super::json::{ModelSummary, ModelVariant};

const OVERLAY_TEXT: &str = include_str!("../../catalog/overlay.toml");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ModelKind {
    Chat,
    Multimodal,
    Speech,
    Embedding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    Live,
    Curated,
    NotDeclared,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Declared<T> {
    pub value: Option<T>,
    pub origin: Origin,
    pub source: Option<String>,
}

impl<T> Declared<T> {
    fn not_declared() -> Self {
        Self {
            value: None,
            origin: Origin::NotDeclared,
            source: None,
        }
    }

    fn live(value: T) -> Self {
        Self {
            value: Some(value),
            origin: Origin::Live,
            source: None,
        }
    }

    fn curated(value: T, source: &str) -> Self {
        Self {
            value: Some(value),
            origin: Origin::Curated,
            source: Some(source.to_string()),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OverlayEntry {
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub input_modalities: Option<Vec<String>>,
    #[serde(default)]
    pub output_modalities: Option<Vec<String>>,
    #[serde(default)]
    pub context_length: Option<u32>,
    #[serde(default)]
    pub embedding_dimensions: Option<u32>,
    #[serde(default)]
    pub quantization: Option<String>,
    #[serde(default)]
    pub generation_params: Option<Vec<GenerationParam>>,
    pub source: String,
}

pub type Overlay = BTreeMap<String, OverlayEntry>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogModel {
    pub alias: String,
    pub id: String,
    pub display_name: Option<String>,
    pub family: Option<String>,
    pub kind: ModelKind,
    pub model_type: Option<String>,
    pub device: Option<String>,
    pub file_size_mb: Option<f64>,
    pub cached: bool,
    pub loaded: bool,
    pub supports_tool_calling: bool,
    pub supports_reasoning: bool,
    pub license: Option<String>,
    pub publisher: Option<String>,
    pub input_modalities: Declared<Vec<String>>,
    pub output_modalities: Declared<Vec<String>>,
    pub context_length: Declared<u32>,
    pub embedding_dimensions: Declared<u32>,
    pub quantization: Declared<String>,
    pub overlay_source: Option<String>,
    pub supported_generation_params: Vec<GenerationParam>,
    pub variants: Vec<ModelVariant>,
}

pub fn overlay() -> FoundryResult<&'static Overlay> {
    static PARSED: OnceLock<Result<Overlay, String>> = OnceLock::new();

    match PARSED.get_or_init(|| parse_overlay(OVERLAY_TEXT)) {
        Ok(overlay) => Ok(overlay),
        Err(detail) => Err(FoundryError::BadResponse {
            detail: format!("catalog overlay: {detail}"),
        }),
    }
}

fn parse_overlay(text: &str) -> Result<Overlay, String> {
    toml::from_str(text).map_err(|error| error.to_string())
}

pub fn merge_all(
    models: Vec<ModelSummary>,
    loaded: &HashSet<String>,
    overlay: &Overlay,
) -> Vec<CatalogModel> {
    models
        .into_iter()
        .map(|model| merge(model, loaded, overlay))
        .collect()
}

pub fn merge(model: ModelSummary, loaded: &HashSet<String>, overlay: &Overlay) -> CatalogModel {
    let entry = overlay_entry(overlay, &model, None);
    let source = entry.map(|entry| entry.source.as_str());

    let input_modalities = if model.input_modalities.is_empty() {
        declared(
            entry.and_then(|entry| entry.input_modalities.clone()),
            source,
        )
    } else {
        Declared::live(model.input_modalities.clone())
    };
    let output_modalities = declared(
        entry.and_then(|entry| entry.output_modalities.clone()),
        source,
    );
    let context_length = declared(entry.and_then(|entry| entry.context_length), source);
    let embedding_dimensions = declared(entry.and_then(|entry| entry.embedding_dimensions), source);
    let quantization = declared(entry.and_then(|entry| entry.quantization.clone()), source);

    let kind = classify(
        &input_modalities,
        &output_modalities,
        model.model_type.as_deref(),
    );
    let supported_generation_params =
        pick_supported_generation_params(entry, kind, model.supports_reasoning);
    let is_loaded = (!model.alias.is_empty() && loaded.contains(&model.alias))
        || (!model.id.is_empty() && loaded.contains(&model.id));
    let overlay_declares_data = entry.is_some_and(|entry| {
        entry.input_modalities.is_some()
            || entry.output_modalities.is_some()
            || entry.context_length.is_some()
            || entry.embedding_dimensions.is_some()
            || entry.quantization.is_some()
            || entry.family.is_some()
            || entry.generation_params.is_some()
    });

    CatalogModel {
        alias: model.alias,
        id: model.id,
        display_name: model.display_name,
        family: entry.and_then(|entry| entry.family.clone()),
        kind,
        model_type: model.model_type,
        device: model.device,
        file_size_mb: model.file_size_mb,
        cached: model.cached,
        loaded: is_loaded,
        supports_tool_calling: model.supports_tool_calling,
        supports_reasoning: model.supports_reasoning,
        license: model.license,
        publisher: model.publisher,
        input_modalities,
        output_modalities,
        context_length,
        embedding_dimensions,
        quantization,
        overlay_source: if overlay_declares_data {
            source.map(str::to_string)
        } else {
            None
        },
        supported_generation_params,
        variants: model.variants,
    }
}

pub fn supported_generation_params(
    model: &ModelSummary,
    preferred_alias: Option<&str>,
    overlay: &Overlay,
) -> Vec<GenerationParam> {
    let entry = overlay_entry(overlay, model, preferred_alias);
    let source = entry.map(|item| item.source.as_str());
    let input_modalities = if model.input_modalities.is_empty() {
        declared(entry.and_then(|item| item.input_modalities.clone()), source)
    } else {
        Declared::live(model.input_modalities.clone())
    };
    let output_modalities = declared(
        entry.and_then(|item| item.output_modalities.clone()),
        source,
    );
    let kind = classify(
        &input_modalities,
        &output_modalities,
        model.model_type.as_deref(),
    );

    pick_supported_generation_params(entry, kind, model.supports_reasoning)
}

fn overlay_entry<'a>(
    overlay: &'a Overlay,
    model: &ModelSummary,
    preferred_alias: Option<&str>,
) -> Option<&'a OverlayEntry> {
    preferred_alias
        .and_then(|alias| overlay.get(alias))
        .or_else(|| overlay.get(&model.alias))
        .or_else(|| overlay.get(&model.id))
}

fn pick_supported_generation_params(
    entry: Option<&OverlayEntry>,
    kind: ModelKind,
    supports_reasoning: bool,
) -> Vec<GenerationParam> {
    entry
        .and_then(|item| item.generation_params.clone())
        .unwrap_or_else(|| default_supported_generation_params(kind, supports_reasoning))
}

fn default_supported_generation_params(
    kind: ModelKind,
    supports_reasoning: bool,
) -> Vec<GenerationParam> {
    if kind != ModelKind::Chat && kind != ModelKind::Multimodal {
        return Vec::new();
    }

    GenerationParam::ALL
        .into_iter()
        .filter(|field| *field != GenerationParam::ReasoningEffort || supports_reasoning)
        .collect()
}

fn declared<T>(value: Option<T>, source: Option<&str>) -> Declared<T> {
    match (value, source) {
        (Some(value), Some(source)) => Declared::curated(value, source),
        _ => Declared::not_declared(),
    }
}

fn classify(
    input: &Declared<Vec<String>>,
    output: &Declared<Vec<String>>,
    model_type: Option<&str>,
) -> ModelKind {
    if input.value.is_some() || output.value.is_some() {
        return classify_modalities(input, output);
    }

    match model_type.map(str::to_ascii_lowercase).as_deref() {
        Some("multimodal") => ModelKind::Multimodal,
        Some("speech") => ModelKind::Speech,
        Some("embedding") => ModelKind::Embedding,
        _ => ModelKind::Chat,
    }
}

fn classify_modalities(input: &Declared<Vec<String>>, output: &Declared<Vec<String>>) -> ModelKind {
    let inputs = input.value.as_deref().unwrap_or_default();

    if inputs
        .iter()
        .any(|modality| modality == "image" || modality == "video")
    {
        return ModelKind::Multimodal;
    }

    if inputs.iter().any(|modality| modality == "audio") {
        return ModelKind::Speech;
    }

    let is_embedding = output
        .value
        .as_deref()
        .is_some_and(|modalities| modalities.iter().any(|modality| modality == "embedding"));

    if is_embedding {
        ModelKind::Embedding
    } else {
        ModelKind::Chat
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn summary(alias: &str) -> ModelSummary {
        ModelSummary {
            alias: alias.to_string(),
            id: format!("{alias}-generic-gpu:1"),
            model_type: Some("Chat".to_string()),
            ..ModelSummary::default()
        }
    }

    fn overlay() -> Overlay {
        parse_overlay(OVERLAY_TEXT).expect("the overlay parses")
    }

    fn count_kind(overlay: &Overlay, kind: ModelKind) -> usize {
        overlay
            .values()
            .filter(|entry| {
                let input = declared(entry.input_modalities.clone(), Some(&entry.source));
                let output = declared(entry.output_modalities.clone(), Some(&entry.source));

                classify(&input, &output, Some("Chat")) == kind
            })
            .count()
    }

    #[test]
    fn overlay_covers_the_macos_catalog() {
        let overlay = overlay();

        assert_eq!(overlay.len(), 48);
        assert_eq!(count_kind(&overlay, ModelKind::Chat), 28);
        assert_eq!(count_kind(&overlay, ModelKind::Multimodal), 9);
        assert_eq!(count_kind(&overlay, ModelKind::Speech), 9);
        assert_eq!(count_kind(&overlay, ModelKind::Embedding), 2);
        assert!(overlay.contains_key("whisper-tiny"));
        assert!(overlay.contains_key("qwen3-embedding-0.6b"));
    }

    #[test]
    fn overlay_modalities_decide_the_kind_before_the_live_type() {
        let mut model = summary("whisper-tiny");

        model.model_type = Some("Chat".to_string());

        let merged = merge(model, &HashSet::new(), &overlay());

        assert_eq!(merged.kind, ModelKind::Speech);
    }

    #[test]
    fn live_type_is_used_when_the_overlay_is_silent() {
        let mut model = summary("future-embedding");

        model.model_type = Some("Embedding".to_string());

        let merged = merge(model, &HashSet::new(), &overlay());

        assert_eq!(merged.kind, ModelKind::Embedding);
        assert_eq!(merged.input_modalities.origin, Origin::NotDeclared);
    }

    #[test]
    fn every_overlay_entry_declares_a_source() {
        let overlay = overlay();

        for (alias, entry) in &overlay {
            assert!(
                entry.source.contains("2026-11-11"),
                "{alias} has a provenance source"
            );
        }
    }

    #[test]
    fn live_input_modalities_win_over_the_overlay() {
        let mut model = summary("gemma-4-e2b-it");

        model.model_type = Some("Multimodal".to_string());
        model.input_modalities = vec!["text".to_string(), "image".to_string()];

        let merged = merge(model, &HashSet::new(), &overlay());

        assert_eq!(merged.kind, ModelKind::Multimodal);
        assert_eq!(merged.input_modalities.origin, Origin::Live);
        assert_eq!(
            merged.input_modalities.value,
            Some(vec!["text".to_string(), "image".to_string()])
        );
        assert!(merged.input_modalities.source.is_none());
        assert_eq!(merged.output_modalities.origin, Origin::Curated);
        assert_eq!(merged.quantization.origin, Origin::Curated);
        assert!(merged.overlay_source.is_some());
    }

    #[test]
    fn overlay_fills_modalities_and_keeps_provenance() {
        let mut model = summary("qwen3-embedding-0.6b");

        model.model_type = None;

        let merged = merge(model, &HashSet::new(), &overlay());

        assert_eq!(merged.kind, ModelKind::Embedding);
        assert_eq!(merged.input_modalities.origin, Origin::Curated);
        assert_eq!(
            merged.output_modalities.value,
            Some(vec!["embedding".to_string()])
        );
        assert_eq!(merged.embedding_dimensions.value, Some(1024));
        assert_eq!(merged.context_length.value, Some(32768));
        assert!(
            merged
                .overlay_source
                .as_deref()
                .is_some_and(|source| source.contains("model cards"))
        );
    }

    #[test]
    fn models_outside_the_overlay_stay_not_declared() {
        let mut model = summary("future-model");

        model.model_type = None;

        let merged = merge(model, &HashSet::new(), &overlay());

        assert_eq!(merged.kind, ModelKind::Chat);
        assert_eq!(merged.family, None);
        assert_eq!(merged.input_modalities.origin, Origin::NotDeclared);
        assert_eq!(merged.input_modalities.value, None);
        assert_eq!(merged.output_modalities.origin, Origin::NotDeclared);
        assert_eq!(merged.context_length.origin, Origin::NotDeclared);
        assert_eq!(merged.quantization.origin, Origin::NotDeclared);
        assert_eq!(merged.overlay_source, None);
    }

    #[test]
    fn loaded_models_match_by_alias_or_id() {
        let mut model = summary("whisper-tiny");

        model.id = "openai-whisper-tiny-generic-cpu:4".to_string();

        let by_alias = HashSet::from(["whisper-tiny".to_string()]);
        let by_id = HashSet::from(["openai-whisper-tiny-generic-cpu:4".to_string()]);

        assert!(merge(model.clone(), &by_alias, &overlay()).loaded);
        assert!(merge(model, &by_id, &overlay()).loaded);
    }

    #[test]
    fn missing_overlay_fields_render_as_not_declared() {
        let mut model = summary("ministral-3-3b-instruct-2512");

        model.model_type = Some("Multimodal".to_string());

        let merged = merge(model, &HashSet::new(), &overlay());

        assert_eq!(merged.context_length.value, None);
        assert_eq!(merged.context_length.origin, Origin::NotDeclared);
        assert_eq!(merged.embedding_dimensions.origin, Origin::NotDeclared);
    }

    #[test]
    fn default_supported_generation_parameters_follow_model_capabilities() {
        let mut model = summary("qwen3-0.6b");

        model.supports_reasoning = false;

        let merged = merge(model.clone(), &HashSet::new(), &overlay());

        assert!(
            !merged
                .supported_generation_params
                .contains(&GenerationParam::ReasoningEffort)
        );

        model.supports_reasoning = true;

        let merged = merge(model, &HashSet::new(), &overlay());

        assert!(
            merged
                .supported_generation_params
                .contains(&GenerationParam::ReasoningEffort)
        );
    }

    #[test]
    fn overlay_can_override_supported_generation_parameters() {
        let mut model = summary("custom-model");

        model.supports_reasoning = true;

        let custom = parse_overlay(
            r#"
[custom-model]
source = "test"
input_modalities = ["text"]
output_modalities = ["text"]
generation_params = ["temperature", "maxTokens"]
"#,
        )
        .expect("custom overlay");
        let merged = merge(model, &HashSet::new(), &custom);

        assert_eq!(
            merged.supported_generation_params,
            vec![GenerationParam::Temperature, GenerationParam::MaxTokens]
        );
    }
}
