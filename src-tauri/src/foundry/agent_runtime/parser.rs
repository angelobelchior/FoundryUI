use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::foundry::harmony::HarmonyStream;

pub const MAX_WIRE_BYTES: usize = 1_048_576;
pub const MAX_CALLS: usize = 16;
pub const MAX_ARGUMENT_BYTES: usize = 65_536;
pub const MAX_NAME_BYTES: usize = 128;
pub const MAX_ID_BYTES: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    Filtered,
    ToolCalls,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParsedTurn {
    pub content: String,
    pub reasoning: String,
    pub calls: Vec<ToolCall>,
    pub finish: FinishReason,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolStreamError {
    #[error("the tool stream exceeded its byte limit")]
    StreamLimit,
    #[error("the tool stream is not valid UTF-8")]
    InvalidUtf8,
    #[error("the tool stream ended inside an event")]
    UnfinishedEvent,
    #[error("the tool stream ended without [DONE]")]
    MissingDone,
    #[error("the tool stream ended without a finish reason")]
    MissingFinish,
    #[error("the tool stream carried data after [DONE]")]
    DataAfterDone,
    #[error("the tool stream carried choices after its finish reason")]
    DataAfterFinish,
    #[error("the tool stream carried an invalid JSON frame")]
    InvalidJson,
    #[error("the provider reported an error frame: {0}")]
    ProviderError(String),
    #[error("the tool stream carried an invalid envelope")]
    InvalidEnvelope,
    #[error("the tool stream carried an unsupported choice")]
    UnsupportedChoice,
    #[error("the tool stream carried an unsupported finish reason")]
    UnsupportedFinish,
    #[error("the tool stream carried an invalid tool part")]
    InvalidToolPart,
    #[error("the tool stream changed a call identifier")]
    ChangedCallId,
    #[error("the tool stream repeated a call identifier")]
    DuplicateCallId,
    #[error("the tool stream carried too many calls")]
    CallLimit,
    #[error("the tool stream carried oversized call fragments")]
    CallFragmentLimit,
    #[error("the tool stream declared tool calls without any call")]
    MissingCalls,
    #[error("the tool stream declared stop but carried tool calls")]
    IncompleteCalls,
    #[error("the tool stream carried incomplete call arguments")]
    IncompleteArguments,
    #[error("the tool stream carried malformed call arguments")]
    InvalidArguments,
    #[error("a tool call carried non-object arguments")]
    NonObjectArguments,
}

#[derive(Debug, Default)]
struct PartialCall {
    id: Option<String>,
    name: String,
    arguments: String,
}

pub struct ToolCallStream {
    pending: Vec<u8>,
    wire_bytes: usize,
    content: String,
    reasoning: String,
    calls: BTreeMap<usize, PartialCall>,
    finish: Option<FinishReason>,
    done: bool,
    harmony: HarmonyStream,
}

impl Default for ToolCallStream {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolCallStream {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            wire_bytes: 0,
            content: String::new(),
            reasoning: String::new(),
            calls: BTreeMap::new(),
            finish: None,
            done: false,
            harmony: HarmonyStream::new(),
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<(), ToolStreamError> {
        self.wire_bytes = self
            .wire_bytes
            .checked_add(chunk.len())
            .ok_or(ToolStreamError::StreamLimit)?;
        if self.wire_bytes > MAX_WIRE_BYTES {
            return Err(ToolStreamError::StreamLimit);
        }
        self.pending.extend_from_slice(chunk);
        let blocks = drain_blocks(&mut self.pending)?;
        for block in blocks {
            self.apply_block(&block)?;
        }
        Ok(())
    }

    pub fn finish(mut self) -> Result<ParsedTurn, ToolStreamError> {
        if !self.pending.is_empty() {
            return Err(ToolStreamError::UnfinishedEvent);
        }
        if !self.done {
            return Err(ToolStreamError::MissingDone);
        }
        let tail = self.harmony.finish();
        self.content.push_str(&tail.content);
        self.reasoning.push_str(&tail.reasoning);
        let finish = self.finish.ok_or(ToolStreamError::MissingFinish)?;
        let calls = self.assemble_calls()?;
        match finish {
            FinishReason::Stop | FinishReason::Length | FinishReason::Filtered
                if !calls.is_empty() =>
            {
                return Err(ToolStreamError::IncompleteCalls);
            }
            FinishReason::ToolCalls if calls.is_empty() => {
                return Err(ToolStreamError::MissingCalls);
            }
            _ => {}
        }
        Ok(ParsedTurn {
            content: self.content,
            reasoning: self.reasoning,
            calls,
            finish,
        })
    }

    fn assemble_calls(&self) -> Result<Vec<ToolCall>, ToolStreamError> {
        let mut calls = Vec::with_capacity(self.calls.len());
        for partial in self.calls.values() {
            let id = partial
                .id
                .clone()
                .filter(|id| !id.is_empty() && id.len() <= MAX_ID_BYTES)
                .ok_or(ToolStreamError::IncompleteArguments)?;
            if partial.name.is_empty() || partial.name.len() > MAX_NAME_BYTES {
                return Err(ToolStreamError::IncompleteArguments);
            }
            let arguments: Value = serde_json::from_str(&partial.arguments)
                .map_err(|_| ToolStreamError::InvalidArguments)?;
            if !arguments.is_object() {
                return Err(ToolStreamError::NonObjectArguments);
            }
            calls.push(ToolCall {
                id,
                name: partial.name.clone(),
                arguments,
            });
        }
        if calls
            .iter()
            .enumerate()
            .any(|(index, call)| calls[..index].iter().any(|other| other.id == call.id))
        {
            return Err(ToolStreamError::DuplicateCallId);
        }
        Ok(calls)
    }

    fn apply_block(&mut self, block: &str) -> Result<(), ToolStreamError> {
        let mut data = String::new();
        for raw in block.split('\n') {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let Some(value) = line.strip_prefix("data:") else {
                continue;
            };
            let value = value.strip_prefix(' ').unwrap_or(value);
            if !data.is_empty() {
                data.push('\n');
            }
            data.push_str(value);
        }
        if data.is_empty() {
            return Ok(());
        }
        if data.trim() == "[DONE]" {
            self.done = true;
            return Ok(());
        }
        if self.done {
            return Err(ToolStreamError::DataAfterDone);
        }
        self.apply_frame(&data)
    }

    fn apply_frame(&mut self, data: &str) -> Result<(), ToolStreamError> {
        let payload: Value =
            serde_json::from_str(data).map_err(|_| ToolStreamError::InvalidJson)?;
        let object = payload
            .as_object()
            .ok_or(ToolStreamError::InvalidEnvelope)?;
        if let Some(error) = object.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();

            return Err(ToolStreamError::ProviderError(message));
        }
        let choices = object
            .get("choices")
            .and_then(Value::as_array)
            .ok_or(ToolStreamError::InvalidEnvelope)?;
        if choices.is_empty() {
            if object.get("usage").is_some_and(Value::is_object) {
                return Ok(());
            }
            return Err(ToolStreamError::InvalidEnvelope);
        }
        if self.finish.is_some() {
            return Err(ToolStreamError::DataAfterFinish);
        }
        if choices.len() != 1 {
            return Err(ToolStreamError::UnsupportedChoice);
        }
        let choice = choices[0]
            .as_object()
            .ok_or(ToolStreamError::InvalidEnvelope)?;
        if choice.get("index").and_then(Value::as_u64).unwrap_or(0) != 0 {
            return Err(ToolStreamError::UnsupportedChoice);
        }
        if let Some(delta) = choice.get("delta") {
            let delta = delta.as_object().ok_or(ToolStreamError::InvalidEnvelope)?;
            if let Some(content) = delta.get("content") {
                if content.is_null() {
                } else if let Some(content) = content.as_str() {
                    let parsed = self.harmony.push(content);

                    self.content.push_str(&parsed.content);
                    self.reasoning.push_str(&parsed.reasoning);
                } else {
                    return Err(ToolStreamError::InvalidEnvelope);
                }
            }
            if let Some(reasoning) = delta.get("reasoning_content") {
                if reasoning.is_null() {
                } else if let Some(reasoning) = reasoning.as_str() {
                    self.reasoning.push_str(reasoning);
                } else {
                    return Err(ToolStreamError::InvalidEnvelope);
                }
            }
            if let Some(parts) = delta.get("tool_calls") {
                let parts = parts.as_array().ok_or(ToolStreamError::InvalidToolPart)?;
                for part in parts {
                    self.apply_part(part)?;
                }
            }
        }
        if let Some(reason) = choice.get("finish_reason")
            && !reason.is_null()
        {
            let reason = reason.as_str().ok_or(ToolStreamError::InvalidEnvelope)?;
            self.finish = Some(match reason {
                "stop" => FinishReason::Stop,
                "length" => FinishReason::Length,
                "content_filter" => FinishReason::Filtered,
                "tool_calls" => FinishReason::ToolCalls,
                _ => return Err(ToolStreamError::UnsupportedFinish),
            });
        }
        Ok(())
    }

    fn apply_part(&mut self, part: &Value) -> Result<(), ToolStreamError> {
        let part = part.as_object().ok_or(ToolStreamError::InvalidToolPart)?;
        match part.get("type") {
            None | Some(Value::Null) => {}
            Some(Value::String(value)) if value == "function" => {}
            _ => return Err(ToolStreamError::InvalidToolPart),
        }
        let index = part
            .get("index")
            .and_then(Value::as_u64)
            .ok_or(ToolStreamError::InvalidToolPart)?;
        let index = usize::try_from(index).map_err(|_| ToolStreamError::InvalidToolPart)?;
        if index >= MAX_CALLS {
            return Err(ToolStreamError::CallLimit);
        }
        let entry = self.calls.entry(index).or_default();
        if let Some(id) = part.get("id")
            && !id.is_null()
        {
            let id = id.as_str().ok_or(ToolStreamError::InvalidToolPart)?;
            if id.is_empty() || id.len() > MAX_ID_BYTES {
                return Err(ToolStreamError::CallFragmentLimit);
            }
            match &entry.id {
                Some(existing) if existing != id => {
                    return Err(ToolStreamError::ChangedCallId);
                }
                Some(_) => {}
                None => entry.id = Some(id.to_string()),
            }
        }
        if let Some(function) = part.get("function") {
            let function = function
                .as_object()
                .ok_or(ToolStreamError::InvalidToolPart)?;
            if let Some(name) = function.get("name")
                && !name.is_null()
            {
                let name = name.as_str().ok_or(ToolStreamError::InvalidToolPart)?;
                entry.name.push_str(name);
                if entry.name.len() > MAX_NAME_BYTES {
                    return Err(ToolStreamError::CallFragmentLimit);
                }
            }
            if let Some(arguments) = function.get("arguments")
                && !arguments.is_null()
            {
                let fragment = arguments.as_str().ok_or(ToolStreamError::InvalidToolPart)?;
                entry.arguments.push_str(fragment);
                if entry.arguments.len() > MAX_ARGUMENT_BYTES {
                    return Err(ToolStreamError::CallFragmentLimit);
                }
            }
        }
        Ok(())
    }
}

fn drain_blocks(pending: &mut Vec<u8>) -> Result<Vec<String>, ToolStreamError> {
    let mut blocks = Vec::new();
    while let Some((end, skip)) = find_separator(pending) {
        let block: Vec<u8> = pending.drain(..end).collect();
        pending.drain(..skip);
        blocks.push(String::from_utf8(block).map_err(|_| ToolStreamError::InvalidUtf8)?);
    }
    Ok(blocks)
}

fn find_separator(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\n' {
            if bytes.get(index + 1) == Some(&b'\n') {
                return Some((index, 2));
            }
            if bytes.get(index + 1) == Some(&b'\r') && bytes.get(index + 2) == Some(&b'\n') {
                return Some((index, 3));
            }
        }
        if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
            if bytes.get(index + 2) == Some(&b'\n') {
                return Some((index, 3));
            }
            if bytes.get(index + 2) == Some(&b'\r') && bytes.get(index + 3) == Some(&b'\n') {
                return Some((index, 4));
            }
        }
        index += 1;
    }
    None
}
