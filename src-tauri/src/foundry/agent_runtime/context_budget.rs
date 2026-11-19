use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const CONSERVATIVE_CAPACITY_TOKENS: usize = 8_192;
const BYTES_PER_TOKEN: usize = 3;
const MINIMUM_SUMMARY_BYTES: usize = 512;
const MAX_MANIFEST_ENTRIES: usize = 256;
const SUMMARY_NOTICE: &str = "Earlier conversation context was reduced to fit the model input budget. Review the context manifest for the exact sources that were shortened or omitted. Ask the user when an omitted earlier detail is required.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapacityOrigin {
    Declared,
    Conservative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceMode {
    Original,
    Summary,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceEntry {
    pub identity_key: String,
    pub kind: &'static str,
    pub mode: SourceMode,
    pub content_fingerprint: String,
    pub bytes: usize,
    pub tokens: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OmissionEntry {
    pub identity_key: String,
    pub kind: &'static str,
    pub reason: &'static str,
    pub bytes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Manifest {
    pub capacity_tokens: usize,
    pub capacity_origin: CapacityOrigin,
    pub warning: Option<&'static str>,
    pub reserved_output_tokens: usize,
    pub reserved_margin_tokens: usize,
    pub allowance_tokens: usize,
    pub allowance_bytes: usize,
    pub estimated_input_tokens: usize,
    pub estimated_input_bytes: usize,
    pub original_messages: usize,
    pub included_messages: usize,
    pub summarized_messages: usize,
    pub omitted_messages: usize,
    pub truncated: bool,
    pub sources: Vec<SourceEntry>,
    pub omissions: Vec<OmissionEntry>,
}

pub struct PlanRequest {
    pub messages: Vec<Value>,
    pub capacity_tokens: Option<usize>,
    pub max_input_bytes: Option<usize>,
    pub requested_output_tokens: Option<usize>,
}

pub struct Plan {
    pub messages: Vec<Value>,
    pub manifest: Manifest,
}

struct Unit {
    messages: Vec<Value>,
    bytes: usize,
    index: usize,
    shortened: bool,
}

pub fn estimate_tokens(bytes: usize) -> usize {
    bytes.div_ceil(BYTES_PER_TOKEN)
}

pub fn plan(request: PlanRequest) -> Plan {
    let total_messages = request.messages.len();
    let capacity = request
        .capacity_tokens
        .filter(|capacity| *capacity > 0)
        .unwrap_or(CONSERVATIVE_CAPACITY_TOKENS);
    let declared = request.capacity_tokens.is_some_and(|capacity| capacity > 0);
    let reserved_output = request
        .requested_output_tokens
        .filter(|tokens| *tokens > 0)
        .map(|tokens| tokens.min((capacity / 2).max(1)))
        .unwrap_or_else(|| (capacity / 4).clamp(128, 2048));
    let reserved_margin = (capacity / 10).max(64);
    let allowance_tokens = capacity
        .saturating_sub(reserved_output)
        .saturating_sub(reserved_margin)
        .max(1);
    let allowance_bytes =
        (allowance_tokens * BYTES_PER_TOKEN).min(request.max_input_bytes.unwrap_or(usize::MAX));

    let mut messages = request.messages;
    let systems = leading_systems(&mut messages);
    let notice_message = json!({"role": "system", "content": SUMMARY_NOTICE});
    let notice_bytes = message_bytes(&notice_message);
    let system_bytes = systems.iter().map(message_bytes).sum::<usize>() + notice_bytes;
    let mut units = group_units(messages);
    let mut tail = units.pop();
    let mut truncated = false;
    let mut summarized = 0usize;

    let tail_bytes = tail.as_ref().map_or(0, |unit| unit.bytes);
    if system_bytes + tail_bytes > allowance_bytes {
        truncated = true;
        if let Some(unit) = tail.as_mut() {
            let available = allowance_bytes.saturating_sub(system_bytes);
            if unit.shorten(available) {
                summarized += 1;
            }
        }
    }

    let mut remaining = allowance_bytes
        .saturating_sub(system_bytes)
        .saturating_sub(tail.as_ref().map_or(0, |unit| unit.bytes));
    let mut older = units;
    let mut kept: Vec<Unit> = Vec::new();
    let mut dropped: Vec<Unit> = Vec::new();
    while let Some(mut unit) = older.pop() {
        if unit.bytes <= remaining {
            remaining -= unit.bytes;
            kept.push(unit);
            continue;
        }
        if unit.messages.len() == 1 && remaining >= MINIMUM_SUMMARY_BYTES && unit.shorten(remaining)
        {
            summarized += 1;
            kept.push(unit);
            break;
        }
        dropped.push(unit);
        break;
    }
    dropped.extend(older.drain(..).rev());
    kept.reverse();

    let included = systems.len()
        + kept
            .iter()
            .chain(tail.iter())
            .map(|unit| unit.messages.len())
            .sum::<usize>();
    let mut sources = Vec::new();
    for unit in kept.iter().chain(tail.iter()) {
        for (offset, message) in unit.messages.iter().enumerate() {
            if sources.len() >= MAX_MANIFEST_ENTRIES {
                truncated = true;
                break;
            }
            sources.push(source_entry(
                message,
                unit.index + offset,
                if unit.shortened {
                    SourceMode::Summary
                } else {
                    SourceMode::Original
                },
            ));
        }
    }

    let omitted_messages = dropped
        .iter()
        .map(|unit| unit.messages.len())
        .sum::<usize>();
    let mut omission_entries = Vec::new();
    for (position, unit) in dropped.iter().enumerate() {
        for (offset, message) in unit.messages.iter().enumerate() {
            if omission_entries.len() >= MAX_MANIFEST_ENTRIES {
                truncated = true;
                break;
            }
            omission_entries.push(OmissionEntry {
                identity_key: identity(unit.index + offset, message),
                kind: kind_of(message),
                reason: if position == 0 { "budget" } else { "older" },
                bytes: message_bytes(message),
            });
        }
    }

    let mut output = systems;
    if omitted_messages > 0 || summarized > 0 {
        output.push(notice_message);
    }
    for unit in kept {
        output.extend(unit.messages);
    }
    if let Some(unit) = tail {
        output.extend(unit.messages);
    }
    if output.is_empty() {
        output.push(json!({"role": "system", "content": SUMMARY_NOTICE}));
    }

    let estimated_input_bytes = output.iter().map(message_bytes).sum();
    let manifest = Manifest {
        capacity_tokens: capacity,
        capacity_origin: if declared {
            CapacityOrigin::Declared
        } else {
            CapacityOrigin::Conservative
        },
        warning: (!declared).then_some("context.capacity_conservative"),
        reserved_output_tokens: reserved_output,
        reserved_margin_tokens: reserved_margin,
        allowance_tokens,
        allowance_bytes,
        estimated_input_tokens: estimate_tokens(estimated_input_bytes),
        estimated_input_bytes,
        original_messages: total_messages,
        included_messages: included,
        summarized_messages: summarized,
        omitted_messages,
        truncated: truncated || omitted_messages > 0 || summarized > 0,
        sources,
        omissions: omission_entries,
    };
    Plan {
        messages: output,
        manifest,
    }
}

impl Unit {
    fn shorten(&mut self, budget_bytes: usize) -> bool {
        if self.messages.len() != 1 {
            return false;
        }
        let Some(shortened) = shorten_message(&self.messages[0], budget_bytes) else {
            return false;
        };
        self.messages[0] = shortened;
        self.bytes = message_bytes(&self.messages[0]);
        self.shortened = true;
        true
    }
}

fn leading_systems(messages: &mut Vec<Value>) -> Vec<Value> {
    let count = messages
        .iter()
        .take_while(|message| message.get("role").and_then(Value::as_str) == Some("system"))
        .count();
    messages.drain(..count).collect()
}

fn group_units(messages: Vec<Value>) -> Vec<Unit> {
    let mut units: Vec<Unit> = Vec::new();
    for (index, message) in messages.into_iter().enumerate() {
        let role = message.get("role").and_then(Value::as_str);
        let continues = role == Some("tool")
            && units
                .last()
                .and_then(|unit| unit.messages.first())
                .and_then(|first| first.get("role"))
                .and_then(Value::as_str)
                == Some("assistant");
        if continues {
            let unit = units.last_mut().expect("unit");
            unit.bytes += message_bytes(&message);
            unit.messages.push(message);
        } else {
            let bytes = message_bytes(&message);
            units.push(Unit {
                messages: vec![message],
                bytes,
                index,
                shortened: false,
            });
        }
    }
    units
}

fn message_bytes(message: &Value) -> usize {
    serde_json::to_vec(message)
        .map(|encoded| encoded.len())
        .unwrap_or(usize::MAX)
}

fn shorten_message(message: &Value, budget_bytes: usize) -> Option<Value> {
    let mut object = message.as_object()?.clone();
    let content = object.get("content")?;
    match content {
        Value::String(text) => {
            let truncated = truncate_text(text, budget_bytes, &object)?;
            object.insert("content".into(), Value::String(truncated));
            Some(Value::Object(object))
        }
        Value::Array(_) => {
            object.insert(
                "content".into(),
                Value::String(
                    "[Earlier message shortened to fit the context budget; non-text parts were omitted.]"
                        .into(),
                ),
            );
            Some(Value::Object(object))
        }
        _ => None,
    }
}

fn truncate_text(
    text: &str,
    budget_bytes: usize,
    object: &serde_json::Map<String, Value>,
) -> Option<String> {
    let current = message_bytes(&Value::Object(object.clone()));
    let overhead = current.saturating_sub(encoded_string_len(text));
    if overhead >= budget_bytes {
        return None;
    }
    let available = budget_bytes - overhead;
    if encoded_string_len(text) <= available {
        return Some(text.to_string());
    }
    let marker = "\n[Earlier content shortened to fit the context budget]";
    let mut end = available.saturating_sub(encoded_string_len(marker));
    while end >= 64 {
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        let candidate = format!("{}{marker}", &text[..end]);
        let encoded = encoded_string_len(&candidate);
        if encoded <= available {
            return Some(candidate);
        }
        end = end.saturating_sub(encoded - available);
    }
    None
}

fn encoded_string_len(text: &str) -> usize {
    serde_json::to_vec(&Value::String(text.to_string()))
        .map(|encoded| encoded.len())
        .unwrap_or(usize::MAX)
}

fn identity(index: usize, message: &Value) -> String {
    let role = message
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    format!("history.{index}.{role}")
}

fn kind_of(message: &Value) -> &'static str {
    match message.get("role").and_then(Value::as_str) {
        Some("system") => "policy",
        Some("tool") => "tool_result",
        Some("assistant") => "assistant",
        Some("user") => {
            let Some(content) = message.get("content").and_then(Value::as_str) else {
                return "user";
            };
            let Ok(packet) = serde_json::from_str::<Value>(content) else {
                return "user";
            };
            match packet.get("kind").and_then(Value::as_str) {
                Some("retrieved_workspace_context") => "retrieved_context",
                Some("approved_workspace_guidance") => "instructions",
                _ => "user",
            }
        }
        _ => "unknown",
    }
}

fn source_entry(message: &Value, index: usize, mode: SourceMode) -> SourceEntry {
    let bytes = message_bytes(message);
    SourceEntry {
        identity_key: identity(index, message),
        kind: kind_of(message),
        mode,
        content_fingerprint: format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(message).unwrap_or_default())
        ),
        bytes,
        tokens: estimate_tokens(bytes),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn serialized(messages: &[Value]) -> usize {
        messages
            .iter()
            .map(|message| serde_json::to_vec(message).unwrap().len())
            .sum()
    }

    fn history(units: usize) -> Vec<Value> {
        let mut messages = vec![json!({"role": "system", "content": "policy"})];
        for index in 0..units {
            messages.push(json!({
                "role": "user",
                "content": format!("question {index} {}", "q".repeat(1500)),
            }));
            messages.push(json!({
                "role": "assistant",
                "content": format!("answer {index} {}", "a".repeat(1500)),
            }));
        }
        messages
    }

    #[test]
    fn a_large_history_is_bounded_and_keeps_the_latest_turn() {
        let mut messages = history(40);
        messages.push(json!({"role": "user", "content": "latest question"}));

        let plan = plan(PlanRequest {
            messages,
            capacity_tokens: Some(2048),
            max_input_bytes: None,
            requested_output_tokens: Some(512),
        });

        assert_eq!(plan.manifest.capacity_origin, CapacityOrigin::Declared);
        assert_eq!(plan.manifest.warning, None);
        assert!(plan.manifest.omitted_messages > 40);
        assert_eq!(
            plan.manifest.omitted_messages + plan.manifest.included_messages,
            plan.manifest.original_messages
        );
        assert!(plan.manifest.truncated);
        assert_eq!(plan.messages.last().unwrap()["content"], "latest question");
        assert_eq!(plan.messages[0]["content"], "policy");
        assert!(
            plan.messages
                .iter()
                .any(|message| message["content"] == SUMMARY_NOTICE)
        );
        assert!(plan.messages.iter().all(|message| {
            !message["content"]
                .as_str()
                .unwrap_or("")
                .contains("answer 0 ")
        }));
        assert!(serialized(&plan.messages) <= plan.manifest.allowance_bytes);
    }

    #[test]
    fn a_latest_turn_that_cannot_fit_is_shortened_and_marked_as_a_summary() {
        let messages = vec![
            json!({"role": "system", "content": "policy"}),
            json!({"role": "user", "content": "z".repeat(50_000)}),
        ];

        let plan = plan(PlanRequest {
            messages,
            capacity_tokens: Some(1024),
            max_input_bytes: None,
            requested_output_tokens: Some(256),
        });

        let latest = plan.messages.last().unwrap()["content"].as_str().unwrap();
        assert!(latest.contains("shortened to fit the context budget"));
        assert!(latest.len() < 50_000);
        assert_eq!(plan.manifest.summarized_messages, 1);
        assert_eq!(
            plan.manifest.sources.last().unwrap().mode,
            SourceMode::Summary
        );
        assert!(plan.manifest.truncated);
        assert!(serialized(&plan.messages) <= plan.manifest.allowance_bytes);
    }

    #[test]
    fn tool_results_are_never_separated_from_their_assistant_call() {
        let messages = vec![
            json!({"role": "system", "content": "policy"}),
            json!({"role": "user", "content": "q".repeat(4000)}),
            json!({"role": "assistant", "content": "", "tool_calls": [{"id": "call-a", "type": "function", "function": {"name": "search_files", "arguments": "{\"query\":\"needle\"}"}}]}),
            json!({"role": "tool", "tool_call_id": "call-a", "content": "t".repeat(4000)}),
            json!({"role": "user", "content": "latest"}),
        ];

        let plan = plan(PlanRequest {
            messages,
            capacity_tokens: Some(1024),
            max_input_bytes: None,
            requested_output_tokens: Some(256),
        });

        assert!(
            plan.messages
                .iter()
                .all(|message| message["role"] != "tool")
        );
        assert!(
            plan.messages
                .iter()
                .all(|message| message["role"] != "assistant")
        );
        assert_eq!(plan.manifest.omitted_messages, 3);
        assert!(
            plan.manifest
                .omissions
                .iter()
                .any(|entry| entry.kind == "tool_result" && entry.reason == "budget")
        );
        assert_eq!(plan.messages.last().unwrap()["content"], "latest");
    }

    #[test]
    fn a_tool_group_that_fits_stays_whole() {
        let messages = vec![
            json!({"role": "system", "content": "policy"}),
            json!({"role": "assistant", "content": "", "tool_calls": [{"id": "call-a", "type": "function", "function": {"name": "search_files", "arguments": "{}"}}]}),
            json!({"role": "tool", "tool_call_id": "call-a", "content": "small"}),
            json!({"role": "tool", "tool_call_id": "call-b", "content": "smaller"}),
            json!({"role": "user", "content": "latest"}),
        ];

        let plan = plan(PlanRequest {
            messages,
            capacity_tokens: Some(8192),
            max_input_bytes: None,
            requested_output_tokens: Some(512),
        });

        assert_eq!(plan.manifest.omitted_messages, 0);
        let roles = plan
            .messages
            .iter()
            .map(|message| message["role"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(roles, ["system", "assistant", "tool", "tool", "user"]);
        assert_eq!(plan.manifest.sources[0].kind, "assistant");
        assert_eq!(plan.manifest.sources[1].kind, "tool_result");
        assert_eq!(plan.manifest.sources[2].kind, "tool_result");
        assert_eq!(plan.manifest.sources[3].kind, "user");
    }

    #[test]
    fn conservative_capacity_is_reported_when_metadata_is_missing() {
        let messages = vec![json!({"role": "user", "content": "question"})];

        let plan = plan(PlanRequest {
            messages,
            capacity_tokens: None,
            max_input_bytes: None,
            requested_output_tokens: None,
        });

        assert_eq!(plan.manifest.capacity_origin, CapacityOrigin::Conservative);
        assert_eq!(plan.manifest.warning, Some("context.capacity_conservative"));
        assert_eq!(plan.manifest.capacity_tokens, CONSERVATIVE_CAPACITY_TOKENS);
    }

    #[test]
    fn the_manifest_exposes_versions_modes_and_omission_reasons() {
        let mut messages = history(12);
        messages.push(json!({"role": "user", "content": "latest"}));

        let plan = plan(PlanRequest {
            messages,
            capacity_tokens: Some(1024),
            max_input_bytes: None,
            requested_output_tokens: Some(256),
        });

        assert!(!plan.manifest.sources.is_empty());
        assert!(!plan.manifest.omissions.is_empty());
        for source in &plan.manifest.sources {
            assert_eq!(source.content_fingerprint.len(), 64);
            assert!(
                source
                    .content_fingerprint
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            );
            assert!(source.tokens > 0);
            assert!(matches!(
                source.mode,
                SourceMode::Original | SourceMode::Summary
            ));
        }
        for omission in &plan.manifest.omissions {
            assert!(matches!(omission.reason, "budget" | "older"));
            assert!(omission.identity_key.starts_with("history."));
            assert!(omission.bytes > 0);
        }
        assert_eq!(
            plan.manifest.omissions[0].reason, "budget",
            "the first omitted unit is the budget boundary"
        );
    }

    #[test]
    fn the_byte_limit_wins_when_it_is_smaller_than_the_token_allowance() {
        let mut messages = history(20);
        messages.push(json!({"role": "user", "content": "latest"}));

        let plan = plan(PlanRequest {
            messages,
            capacity_tokens: Some(32_768),
            max_input_bytes: Some(4096),
            requested_output_tokens: Some(1024),
        });

        assert_eq!(plan.manifest.allowance_bytes, 4096);
        assert!(serialized(&plan.messages) <= 4096);
        assert!(plan.manifest.omitted_messages > 0);
    }
}
