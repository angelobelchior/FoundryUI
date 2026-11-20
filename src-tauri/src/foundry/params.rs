use serde::{Deserialize, Serialize};

use super::chat::{ChatParams, GenerationParam, ReasoningEffort, ResponseVerbosity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamScope {
    Global,
    Model,
    Conversation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ParamOrigin {
    Conversation,
    Model,
    Global,
    Default,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resolved<T> {
    pub value: Option<T>,
    pub origin: ParamOrigin,
}

impl<T> Default for Resolved<T> {
    fn default() -> Self {
        Self {
            value: None,
            origin: ParamOrigin::Default,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedParams {
    pub temperature: Resolved<f64>,
    pub top_p: Resolved<f64>,
    pub max_tokens: Resolved<u32>,
    pub seed: Resolved<i64>,
    pub stop: Resolved<Vec<String>>,
    pub reasoning_effort: Resolved<ReasoningEffort>,
    pub response_verbosity: Resolved<ResponseVerbosity>,
}

impl ResolvedParams {
    pub fn params(&self) -> ChatParams {
        ChatParams {
            temperature: self.temperature.value,
            top_p: self.top_p.value,
            max_tokens: self.max_tokens.value,
            seed: self.seed.value,
            stop: self.stop.value.clone(),
            reasoning_effort: self.reasoning_effort.value,
            response_verbosity: self.response_verbosity.value,
            ..ChatParams::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.params().is_empty()
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamsSnapshot {
    pub global: ChatParams,
    pub model: ChatParams,
    pub conversation: ChatParams,
    pub resolved: ResolvedParams,
}

pub fn resolve(
    conversation: &ChatParams,
    model: &ChatParams,
    global: &ChatParams,
) -> ResolvedParams {
    ResolvedParams {
        temperature: pick(
            conversation.temperature,
            model.temperature,
            global.temperature,
        ),
        top_p: pick(conversation.top_p, model.top_p, global.top_p),
        max_tokens: pick(conversation.max_tokens, model.max_tokens, global.max_tokens),
        seed: pick(conversation.seed, model.seed, global.seed),
        stop: pick(
            conversation.stop.clone(),
            model.stop.clone(),
            global.stop.clone(),
        ),
        reasoning_effort: pick(
            conversation.reasoning_effort,
            model.reasoning_effort,
            global.reasoning_effort,
        ),
        response_verbosity: pick(
            conversation.response_verbosity,
            model.response_verbosity,
            global.response_verbosity,
        ),
    }
}

pub fn filter_supported(
    mut resolved: ResolvedParams,
    supported: &[GenerationParam],
) -> ResolvedParams {
    let supports = |field| supported.contains(&field);

    if !supports(GenerationParam::ReasoningEffort) {
        resolved.reasoning_effort = Resolved::default();
    }

    if !supports(GenerationParam::ResponseVerbosity) {
        resolved.response_verbosity = Resolved::default();
    }

    if !supports(GenerationParam::Temperature) {
        resolved.temperature = Resolved::default();
    }

    if !supports(GenerationParam::TopP) {
        resolved.top_p = Resolved::default();
    }

    if !supports(GenerationParam::MaxTokens) {
        resolved.max_tokens = Resolved::default();
    }

    if !supports(GenerationParam::Seed) {
        resolved.seed = Resolved::default();
    }

    if !supports(GenerationParam::Stop) {
        resolved.stop = Resolved::default();
    }

    resolved
}

fn pick<T>(conversation: Option<T>, model: Option<T>, global: Option<T>) -> Resolved<T> {
    match (conversation, model, global) {
        (Some(value), _, _) => Resolved {
            value: Some(value),
            origin: ParamOrigin::Conversation,
        },
        (None, Some(value), _) => Resolved {
            value: Some(value),
            origin: ParamOrigin::Model,
        },
        (None, None, Some(value)) => Resolved {
            value: Some(value),
            origin: ParamOrigin::Global,
        },
        (None, None, None) => Resolved::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn worldwide() -> ChatParams {
        ChatParams {
            temperature: Some(0.4),
            top_p: Some(0.8),
            max_tokens: Some(2048),
            ..ChatParams::default()
        }
    }

    #[test]
    fn the_conversation_layer_wins_over_the_model_and_the_global_defaults() {
        let global = worldwide();
        let model = ChatParams {
            temperature: Some(0.2),
            ..ChatParams::default()
        };
        let conversation = ChatParams {
            temperature: Some(0.9),
            ..ChatParams::default()
        };
        let resolved = resolve(&conversation, &model, &global);

        assert_eq!(resolved.temperature.value, Some(0.9));
        assert_eq!(resolved.temperature.origin, ParamOrigin::Conversation);
        assert_eq!(resolved.top_p.value, Some(0.8));
        assert_eq!(resolved.top_p.origin, ParamOrigin::Global);
        assert_eq!(resolved.max_tokens.origin, ParamOrigin::Global);
        assert_eq!(resolved.seed.origin, ParamOrigin::Default);
    }

    #[test]
    fn resetting_a_field_falls_back_to_the_previous_level() {
        let global = worldwide();
        let model = ChatParams {
            temperature: Some(0.2),
            ..ChatParams::default()
        };

        let cleared = resolve(&ChatParams::default(), &model, &global);

        assert_eq!(cleared.temperature.value, Some(0.2));
        assert_eq!(cleared.temperature.origin, ParamOrigin::Model);

        let both_cleared = resolve(&ChatParams::default(), &ChatParams::default(), &global);

        assert_eq!(both_cleared.temperature.value, Some(0.4));
        assert_eq!(both_cleared.temperature.origin, ParamOrigin::Global);
    }

    #[test]
    fn nothing_configured_resolves_to_an_empty_default_layer() {
        let resolved = resolve(
            &ChatParams::default(),
            &ChatParams::default(),
            &ChatParams::default(),
        );

        assert!(resolved.is_empty());
        assert_eq!(resolved.temperature.origin, ParamOrigin::Default);
        assert_eq!(resolved.temperature.value, None);

        let mut payload = json!({"model": "qwen3-0.6b"});
        resolved.params().apply(&mut payload);

        assert_eq!(payload, json!({"model": "qwen3-0.6b"}));
    }

    #[test]
    fn resolved_parameters_apply_only_the_fields_that_were_defined() {
        let global = worldwide();
        let conversation = ChatParams {
            seed: Some(7),
            stop: Some(vec!["END".to_string()]),
            response_verbosity: Some(ResponseVerbosity::Detailed),
            ..ChatParams::default()
        };
        let resolved = resolve(&conversation, &ChatParams::default(), &global);
        let mut payload = json!({"stream": true});

        assert_eq!(
            resolved.response_verbosity.value,
            Some(ResponseVerbosity::Detailed)
        );
        assert_eq!(
            resolved.response_verbosity.origin,
            ParamOrigin::Conversation
        );

        resolved.params().apply(&mut payload);

        assert_eq!(payload["temperature"], json!(0.4));
        assert_eq!(payload["top_p"], json!(0.8));
        assert_eq!(payload["max_tokens"], json!(2048));
        assert_eq!(payload["seed"], json!(7));
        assert_eq!(payload["stop"], json!(["END"]));
    }
}
