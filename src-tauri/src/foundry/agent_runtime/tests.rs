use super::cycle::{ModelRequest, TurnProvider};
use super::parser::{
    FinishReason, MAX_ARGUMENT_BYTES, MAX_WIRE_BYTES, ParsedTurn, ToolCallStream, ToolStreamError,
};
use super::schema::{
    RegisteredTool, ToolAvailability, ToolRegistry, ToolSpec, ToolSpecError, ToolUnavailable,
};
use serde_json::{Value, json};

fn frame(value: Value) -> String {
    format!("data: {}\n\n", serde_json::to_string(&value).unwrap())
}

fn call_part(index: u64, id: &str, name: &str, arguments: &str) -> Value {
    json!({
        "index": index,
        "id": id,
        "type": "function",
        "function": { "name": name, "arguments": arguments }
    })
}

fn delta_turn(calls: Vec<Value>) -> String {
    let mut wire = frame(
        json!({"choices": [{"index": 0, "delta": {"tool_calls": calls}, "finish_reason": "tool_calls"}]}),
    );
    wire.push_str("data: [DONE]\n\n");
    wire
}

fn collected(wire: &str) -> Result<ParsedTurn, ToolStreamError> {
    let mut stream = ToolCallStream::new();
    stream.push(wire.as_bytes())?;
    stream.finish()
}

#[test]
fn a_fragmented_call_is_available_only_after_a_complete_stream() {
    let mut stream = ToolCallStream::new();
    stream
        .push(
            frame(json!({"choices": [{"index": 0, "delta": {"content": "thinking"}}]})).as_bytes(),
        )
        .unwrap();
    stream
        .push(
            frame(json!({"choices": [{"index": 0, "delta": {"tool_calls": [
                call_part(0, "call_a", "read_file", "{\"path\":\"a")
            ]}}]}))
            .as_bytes(),
        )
        .unwrap();
    stream
        .push(
            frame(json!({"choices": [{"index": 0, "delta": {"tool_calls": [
                {"index": 0, "function": {"arguments": ".txt\"}"}}
            ]}, "finish_reason": "tool_calls"}]}))
            .as_bytes(),
        )
        .unwrap();
    assert!(stream.finish().is_err());

    let mut stream = ToolCallStream::new();
    stream
        .push(
            delta_turn(vec![call_part(
                0,
                "call_a",
                "read_file",
                "{\"path\":\"a.txt\"}",
            )])
            .as_bytes(),
        )
        .unwrap();
    let turn = stream.finish().unwrap();
    assert_eq!(turn.calls.len(), 1);
    assert_eq!(turn.calls[0].id, "call_a");
    assert_eq!(turn.calls[0].name, "read_file");
    assert_eq!(turn.calls[0].arguments, json!({"path": "a.txt"}));
    assert_eq!(turn.finish, FinishReason::ToolCalls);
}

#[test]
fn plain_text_never_becomes_a_call() {
    let wire = format!(
        "{}{}",
        frame(
            json!({"choices": [{"index": 0, "delta": {"content": "```sh\nrm -rf /\n```", "reasoning_content": "<|call|>{\"name\":\"shell\"}"}}]})
        ),
        frame(json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}))
    ) + "data: [DONE]\n\n";
    let turn = collected(&wire).unwrap();
    assert!(turn.calls.is_empty());
    assert!(turn.content.contains("rm -rf /"));
    assert!(turn.reasoning.contains("<|call|>"));
    assert_eq!(turn.finish, FinishReason::Stop);
}

#[test]
fn truncated_and_filtered_finishes_are_reported_without_their_calls() {
    let length = format!(
        "{}{}",
        frame(
            json!({"choices": [{"index": 0, "delta": {"content": "partial"}, "finish_reason": "length"}]})
        ),
        "data: [DONE]\n\n"
    );
    let turn = collected(&length).unwrap();
    assert_eq!(turn.finish, FinishReason::Length);
    assert_eq!(turn.content, "partial");
    assert!(turn.calls.is_empty());

    let filtered = format!(
        "{}{}",
        frame(json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "content_filter"}]})),
        "data: [DONE]\n\n"
    );
    let turn = collected(&filtered).unwrap();
    assert_eq!(turn.finish, FinishReason::Filtered);
    assert!(turn.calls.is_empty());
}

#[test]
fn an_invalid_batch_never_produces_executable_calls() {
    let good = call_part(0, "call_a", "read_file", "{\"path\":\"a.txt\"}");
    let cases = [
        frame(
            json!({"choices": [{"index": 0, "delta": {"tool_calls": [good.clone()]}, "finish_reason": "tool_calls"}]}),
        ),
        delta_turn(vec![good.clone()]).replace(
            "\"finish_reason\":\"tool_calls\"",
            "\"finish_reason\":\"length\"",
        ),
        delta_turn(vec![
            good.clone(),
            json!({"index": 1, "id": "call_a", "function": {"name": "read_file", "arguments": "{\"path\":\"b.txt\"}"}}),
        ]),
        delta_turn(vec![call_part(0, "call_a", "read_file", "[1,2]")]),
        delta_turn(vec![call_part(0, "call_a", "read_file", "{\"path\":")]),
        format!(
            "{}\n{}",
            delta_turn(vec![good.clone()]),
            frame(json!({"choices": [{"index": 0, "delta": {"content": "late"}}]}))
        ),
        format!(
            "data: [DONE]\n\n{}",
            frame(json!({"choices": [{"index": 0, "delta": {"tool_calls": [good.clone()]}}]}))
        ),
        delta_turn(vec![
            json!({"index": 0, "id": "call_a", "type": "other", "function": {"name": "read_file", "arguments": "{}"}}),
        ]),
        "data: [DONE]\n\n".to_string(),
    ];
    for wire in cases {
        assert!(collected(&wire).is_err(), "wire should be rejected: {wire}");
    }
}

#[test]
fn call_ids_must_stay_stable_across_fragments() {
    let first = frame(
        json!({"choices": [{"index": 0, "delta": {"tool_calls": [call_part(0, "call_a", "read_file", r#"{"path":"a"#)]}}]}),
    );
    let second = frame(
        json!({"choices": [{"index": 0, "delta": {"tool_calls": [call_part(0, "call_b", "", r#".txt"}"#)]}, "finish_reason": "tool_calls"}]}),
    );
    assert_eq!(
        collected(&(first + &second + "data: [DONE]\n\n")),
        Err(ToolStreamError::ChangedCallId)
    );
}

#[test]
fn oversized_streams_fail_closed() {
    let mut stream = ToolCallStream::new();
    let chunk = vec![b'x'; MAX_WIRE_BYTES + 1];
    assert_eq!(stream.push(&chunk), Err(ToolStreamError::StreamLimit));

    let arguments = "{\"path\":\"".to_string() + &"a".repeat(MAX_ARGUMENT_BYTES) + "\"}";
    assert_eq!(
        collected(&delta_turn(vec![call_part(
            0,
            "call_a",
            "read_file",
            &arguments
        )])),
        Err(ToolStreamError::CallFragmentLimit)
    );
}

#[test]
fn tool_schemas_reject_incomplete_or_unexpected_arguments() {
    let spec = ToolSpec {
        name: "read_file".into(),
        description: "Read a bounded range of an authorized file.".into(),
        parameters: json!({
            "type": "object",
            "properties": {
                "path": {"type": "string", "minLength": 1, "maxLength": 64},
                "limit": {"type": "integer", "minimum": 1, "maximum": 4096}
            },
            "required": ["path"],
            "additionalProperties": false
        }),
    };
    spec.validate().unwrap();
    spec.validate_arguments(&json!({"path": "a.txt"})).unwrap();
    spec.validate_arguments(&json!({"path": "a.txt", "limit": 10}))
        .unwrap();
    for invalid in [
        json!({}),
        json!({"path": 1}),
        json!({"path": "a.txt", "limit": 0}),
        json!({"path": "a.txt", "limit": 1.5}),
        json!({"path": "a.txt", "extra": true}),
        json!({"path": ""}),
    ] {
        assert!(
            spec.validate_arguments(&invalid).is_err(),
            "arguments should be rejected: {invalid}"
        );
    }
}

#[test]
fn registries_only_expose_available_tools() {
    let mut registry = ToolRegistry::default();
    registry
        .register(RegisteredTool {
            spec: ToolSpec {
                name: "read_file".into(),
                description: "Read an authorized file.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {"path": {"type": "string"}},
                    "required": ["path"],
                    "additionalProperties": false
                }),
            },
            availability: ToolAvailability::Available,
        })
        .unwrap();
    registry
        .register(RegisteredTool {
            spec: ToolSpec {
                name: "run_command".into(),
                description: "Run a controlled command.".into(),
                parameters: json!({
                    "type": "object",
                    "properties": {"argv": {"type": "array", "items": {"type": "string"}}},
                    "required": ["argv"],
                    "additionalProperties": false
                }),
            },
            availability: ToolAvailability::Unavailable {
                reason: ToolUnavailable::CapabilityBlocked,
            },
        })
        .unwrap();
    let definitions = registry.model_definitions();
    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0]["function"]["name"], "read_file");
}

#[test]
fn invalid_tool_specs_are_rejected() {
    let invalid = [
        ToolSpec {
            name: "".into(),
            description: "x".into(),
            parameters: json!({"type": "object"}),
        },
        ToolSpec {
            name: "bad name".into(),
            description: "x".into(),
            parameters: json!({"type": "object"}),
        },
        ToolSpec {
            name: "ok".into(),
            description: "".into(),
            parameters: json!({"type": "object"}),
        },
        ToolSpec {
            name: "ok".into(),
            description: "x".into(),
            parameters: json!({"type": "array"}),
        },
    ];
    for spec in invalid {
        assert!(spec.validate().is_err());
    }
}

#[test]
fn duplicate_tool_names_are_rejected() {
    let mut registry = ToolRegistry::default();
    let tool = RegisteredTool {
        spec: ToolSpec {
            name: "read_file".into(),
            description: "Read an authorized file.".into(),
            parameters: json!({"type": "object"}),
        },
        availability: ToolAvailability::Available,
    };
    registry.register(tool.clone()).unwrap();
    assert_eq!(registry.register(tool), Err(ToolSpecError::DuplicateName));
}

#[tokio::test]
async fn the_endpoint_provider_streams_structured_calls() {
    use std::io::{Read, Write};
    use std::net::TcpListener;

    use crate::foundry::agent_runtime::EndpointTurnProvider;
    use crate::foundry::http::StreamTimeouts;
    use crate::foundry::local_inference::{
        RuntimeBinding, RuntimeObservation, VerifiedLocalEndpoint,
    };

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 8192];
        let _ = stream.read(&mut request);
        let body = format!(
            "{}{}{}",
            frame(json!({"choices": [{"index": 0, "delta": {"content": "working"}}]})),
            frame(
                json!({"choices": [{"index": 0, "delta": {"tool_calls": [call_part(0, "call_a", "read_file", "{\"path\":\"a.txt\"}")]}, "finish_reason": "tool_calls"}]})
            ),
            "data: [DONE]\n\n"
        );
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}"
        );
        stream.write_all(response.as_bytes()).unwrap();
    });

    let binding = RuntimeBinding::default();
    binding.replace(Some(
        RuntimeObservation::new(
            &endpoint,
            std::process::id(),
            std::env::current_exe().unwrap(),
            1,
        )
        .unwrap(),
    ));
    let verified = VerifiedLocalEndpoint::verify(&binding).unwrap();
    let mut provider = EndpointTurnProvider {
        endpoint: verified,
        model_id: "fixture-model".into(),
        params: None,
        timeouts: StreamTimeouts::from_seconds(5, 5),
    };
    let turn = provider
        .next_turn(ModelRequest {
            messages: vec![json!({"role": "user", "content": "read"})],
            tools: vec![json!({"type": "function", "function": {"name": "read_file"}})],
        })
        .await
        .unwrap();
    server.join().unwrap();
    assert_eq!(turn.content, "working");
    assert_eq!(turn.calls.len(), 1);
    assert_eq!(turn.calls[0].id, "call_a");
    assert_eq!(turn.calls[0].arguments, json!({"path": "a.txt"}));
}
