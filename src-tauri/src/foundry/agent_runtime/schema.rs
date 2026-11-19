use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolUnavailable {
    NotImplemented,
    CapabilityBlocked,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ToolAvailability {
    Available,
    Unavailable { reason: ToolUnavailable },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredTool {
    pub spec: ToolSpec,
    pub availability: ToolAvailability,
}

#[derive(Debug, Clone, Default)]
pub struct ToolRegistry {
    tools: Vec<RegisteredTool>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ToolSpecError {
    #[error("the tool name is invalid")]
    InvalidName,
    #[error("a tool with this name is already registered")]
    DuplicateName,
    #[error("the tool description is invalid")]
    InvalidDescription,
    #[error("the tool parameters are not an object schema")]
    InvalidSchema,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{path}: {reason}")]
pub struct ArgumentError {
    pub path: String,
    pub reason: &'static str,
}

impl ToolSpec {
    pub fn validate(&self) -> Result<(), ToolSpecError> {
        if self.name.is_empty()
            || self.name.len() > 128
            || !self
                .name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
        {
            return Err(ToolSpecError::InvalidName);
        }
        if self.description.is_empty() || self.description.len() > 4096 {
            return Err(ToolSpecError::InvalidDescription);
        }
        match &self.parameters {
            Value::Object(schema)
                if schema.get("type").and_then(Value::as_str) == Some("object") =>
            {
                Ok(())
            }
            _ => Err(ToolSpecError::InvalidSchema),
        }
    }

    pub fn validate_arguments(&self, arguments: &Value) -> Result<(), ArgumentError> {
        validate_value(&self.parameters, arguments, "$")
    }

    pub fn model_definition(&self) -> Value {
        serde_json::json!({
            "type": "function",
            "function": {
                "name": self.name,
                "description": self.description,
                "parameters": self.parameters,
            },
        })
    }
}

impl ToolRegistry {
    pub fn register(&mut self, tool: RegisteredTool) -> Result<(), ToolSpecError> {
        tool.spec.validate()?;
        if self.get(&tool.spec.name).is_some() {
            return Err(ToolSpecError::DuplicateName);
        }
        self.tools.push(tool);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&RegisteredTool> {
        self.tools.iter().find(|tool| tool.spec.name == name)
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    pub fn available(&self) -> impl Iterator<Item = &RegisteredTool> {
        self.tools
            .iter()
            .filter(|tool| tool.availability == ToolAvailability::Available)
    }

    pub fn model_definitions(&self) -> Vec<Value> {
        self.available()
            .map(|tool| tool.spec.model_definition())
            .collect()
    }
}

fn validate_value(schema: &Value, value: &Value, path: &str) -> Result<(), ArgumentError> {
    let schema = schema
        .as_object()
        .ok_or_else(|| argument_error(path, "schema is not an object"))?;
    let kind = schema
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| argument_error(path, "schema has no type"))?;
    match kind {
        "object" => validate_object(schema, value, path),
        "array" => validate_array(schema, value, path),
        "string" => validate_string(schema, value, path),
        "number" => validate_number(schema, value, path, false),
        "integer" => validate_number(schema, value, path, true),
        "boolean" => {
            if value.is_boolean() {
                Ok(())
            } else {
                Err(argument_error(path, "expected a boolean"))
            }
        }
        "null" => {
            if value.is_null() {
                Ok(())
            } else {
                Err(argument_error(path, "expected null"))
            }
        }
        _ => Err(argument_error(path, "unsupported schema type")),
    }
}

fn validate_object(
    schema: &Map<String, Value>,
    value: &Value,
    path: &str,
) -> Result<(), ArgumentError> {
    let Some(value) = value.as_object() else {
        return Err(argument_error(path, "expected an object"));
    };
    let properties = match schema.get("properties") {
        None => None,
        Some(Value::Object(properties)) => Some(properties),
        Some(_) => return Err(argument_error(path, "properties is not an object")),
    };
    if let Some(required) = schema.get("required") {
        let required = required
            .as_array()
            .ok_or_else(|| argument_error(path, "required is not an array"))?;
        for name in required {
            let name = name
                .as_str()
                .ok_or_else(|| argument_error(path, "required is not a string"))?;
            if !value.contains_key(name) {
                return Err(argument_error(path, "a required argument is missing"));
            }
        }
    }
    let allow_extra = schema
        .get("additionalProperties")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    for (name, item) in value {
        match properties.and_then(|properties| properties.get(name)) {
            Some(property) => {
                validate_value(property, item, &format!("{path}.{name}"))?;
            }
            None if allow_extra => {}
            None => {
                return Err(argument_error(
                    &format!("{path}.{name}"),
                    "unexpected argument",
                ));
            }
        }
    }
    if let Some(max) = schema.get("maxProperties").and_then(Value::as_u64)
        && value.len() as u64 > max
    {
        return Err(argument_error(path, "too many properties"));
    }
    if let Some(min) = schema.get("minProperties").and_then(Value::as_u64)
        && (value.len() as u64) < min
    {
        return Err(argument_error(path, "too few properties"));
    }
    Ok(())
}

fn validate_array(
    schema: &Map<String, Value>,
    value: &Value,
    path: &str,
) -> Result<(), ArgumentError> {
    let Some(value) = value.as_array() else {
        return Err(argument_error(path, "expected an array"));
    };
    if let Some(max) = schema.get("maxItems").and_then(Value::as_u64)
        && value.len() as u64 > max
    {
        return Err(argument_error(path, "too many items"));
    }
    if let Some(min) = schema.get("minItems").and_then(Value::as_u64)
        && (value.len() as u64) < min
    {
        return Err(argument_error(path, "too few items"));
    }
    if let Some(items) = schema.get("items") {
        for (index, item) in value.iter().enumerate() {
            validate_value(items, item, &format!("{path}[{index}]"))?;
        }
    }
    Ok(())
}

fn validate_string(
    schema: &Map<String, Value>,
    value: &Value,
    path: &str,
) -> Result<(), ArgumentError> {
    let Some(value) = value.as_str() else {
        return Err(argument_error(path, "expected a string"));
    };
    if let Some(max) = schema.get("maxLength").and_then(Value::as_u64)
        && value.chars().count() as u64 > max
    {
        return Err(argument_error(path, "string is too long"));
    }
    if let Some(min) = schema.get("minLength").and_then(Value::as_u64)
        && (value.chars().count() as u64) < min
    {
        return Err(argument_error(path, "string is too short"));
    }
    if let Some(values) = schema.get("enum") {
        let values = values
            .as_array()
            .ok_or_else(|| argument_error(path, "enum is not an array"))?;
        if !values.iter().any(|allowed| allowed.as_str() == Some(value)) {
            return Err(argument_error(path, "value is not allowed"));
        }
    }
    Ok(())
}

fn validate_number(
    schema: &Map<String, Value>,
    value: &Value,
    path: &str,
    integer: bool,
) -> Result<(), ArgumentError> {
    let number = value
        .as_f64()
        .ok_or_else(|| argument_error(path, "expected a number"))?;
    if integer && value.as_i64().is_none() && value.as_u64().is_none() {
        return Err(argument_error(path, "expected an integer"));
    }
    if let Some(max) = schema.get("maximum").and_then(Value::as_f64)
        && number > max
    {
        return Err(argument_error(path, "number is too large"));
    }
    if let Some(min) = schema.get("minimum").and_then(Value::as_f64)
        && number < min
    {
        return Err(argument_error(path, "number is too small"));
    }
    Ok(())
}

fn argument_error(path: &str, reason: &'static str) -> ArgumentError {
    ArgumentError {
        path: path.to_string(),
        reason,
    }
}
