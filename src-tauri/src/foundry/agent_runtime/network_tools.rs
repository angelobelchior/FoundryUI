use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{Value, json};

use super::cycle::{ToolExecution, ToolExecutor, ToolHost};
use super::parser::ToolCall;
use super::schema::{RegisteredTool, ToolAvailability, ToolRegistry, ToolSpec, ToolSpecError};
use crate::foundry::db::agent::authorization::AuthorizationRevision;
use crate::foundry::db::agent::{ActionRecord, AgentStore, ContextFact, NetworkEventState};
use crate::foundry::policy as p;

const MAX_BODY_BYTES: usize = 256 * 1024;

pub fn register(registry: &mut ToolRegistry) -> Result<(), ToolSpecError> {
    registry.register(RegisteredTool {
        spec: ToolSpec {
            name: "http_request".into(),
            description: "Make an audited HTTP GET request to an authorized destination.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "method": {"type": "string", "enum": ["GET"]},
                    "url": {"type": "string", "minLength": 1, "maxLength": 4096}
                },
                "required": ["method", "url"],
                "additionalProperties": false
            }),
        },
        availability: ToolAvailability::Available,
    })
}

pub struct NetworkExecutor;

#[async_trait]
impl ToolExecutor for NetworkExecutor {
    async fn execute(
        &mut self,
        _call: &ToolCall,
        _spec: &ToolSpec,
        host: &ToolHost<'_, '_>,
    ) -> ToolExecution {
        let p::Operation::Network { destination, .. } = &host.facts.action.operation else {
            return ToolExecution::Failed {
                summary: json!({"status": "failed", "code": "invalid_operation"}),
            };
        };
        let key = &host.facts.action.key;
        let client = crate::foundry::http::build_client();
        let response = match client.get(destination.as_str()).send().await {
            Ok(response) => response,
            Err(_) => {
                record_network_event(
                    host.store,
                    key,
                    destination,
                    NetworkEventState::Failed,
                    None,
                );
                return ToolExecution::Failed {
                    summary: json!({"status": "failed", "code": "connection"}),
                };
            }
        };
        let status_code = response.status().as_u16();
        if !response.status().is_success() {
            record_network_event(
                host.store,
                key,
                destination,
                NetworkEventState::Failed,
                Some(status_code),
            );
            return ToolExecution::Failed {
                summary: json!({"status": "failed", "code": "http_status"}),
            };
        }
        let (bytes, truncated) = match read_body(response, MAX_BODY_BYTES).await {
            Ok(count) => count,
            Err(_) => {
                record_network_event(
                    host.store,
                    key,
                    destination,
                    NetworkEventState::Failed,
                    Some(status_code),
                );
                return ToolExecution::Failed {
                    summary: json!({"status": "failed", "code": "connection"}),
                };
            }
        };
        record_network_event(
            host.store,
            key,
            destination,
            NetworkEventState::Completed,
            Some(status_code),
        );
        ToolExecution::Succeeded {
            summary: json!({
                "status": "succeeded",
                "status_code": status_code,
                "bytes": bytes,
                "truncated": truncated,
            }),
        }
    }
}

async fn read_body(
    response: reqwest::Response,
    limit: usize,
) -> Result<(usize, bool), reqwest::Error> {
    let mut stream = response.bytes_stream();
    let mut total = 0usize;
    while let Some(chunk) = stream.next().await {
        total = total.saturating_add(chunk?.len());
        if total > limit {
            return Ok((total, true));
        }
    }
    Ok((total, false))
}

fn record_network_event(
    store: &AgentStore<'_>,
    key: &p::ActionKey,
    destination: &str,
    state: NetworkEventState,
    http_status: Option<u16>,
) {
    let _ = store.record_fact(
        key.context.conversation_id,
        key.context.context_revision,
        Some(key.run_id),
        &format!("network-{}", key.action_id),
        &ContextFact::NetworkEvent {
            destination: destination.to_string(),
            state,
            http_status,
        },
    );
}

pub struct NetworkAuthorizer;

impl super::cycle::ToolAuthorizer for NetworkAuthorizer {
    fn facts(
        &self,
        _store: &AgentStore<'_>,
        revision: &AuthorizationRevision,
        action: &ActionRecord,
        call: &ToolCall,
    ) -> Result<p::TrustedFacts, String> {
        let method = call
            .arguments
            .get("method")
            .and_then(Value::as_str)
            .ok_or_else(|| "http_request requires a method argument".to_string())?
            .to_ascii_uppercase();
        let destination = call
            .arguments
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| "http_request requires a url argument".to_string())?
            .to_string();
        let scheme = destination
            .split_once(':')
            .map(|(scheme, _)| scheme)
            .unwrap_or("");
        if scheme != "http" && scheme != "https" {
            return Err("http_request only supports http and https destinations".to_string());
        }
        let policy_action = p::Action {
            key: p::ActionKey {
                context: revision.key().clone(),
                run_id: action.run_id,
                action_id: action.id,
            },
            operation: p::Operation::Network {
                method,
                destination: destination.clone(),
            },
            arguments_fingerprint: action.arguments_fingerprint.clone(),
            resources: Vec::new(),
            origin: p::InstructionOrigin::Model,
        };
        Ok(p::TrustedFacts {
            current_key: policy_action.key.clone(),
            action: policy_action,
            mode: revision.mode(),
            mode_at_proposal: revision.mode(),
            project_root_id: revision.project_root_id().map(str::to_string),
            global_read_allowed: revision.global_read_allowed(),
            read_evidence: p::ReadEvidence::ProvenNoReads,
            capability: p::Capability::Proven {
                effects: p::Effects::ReadOnly,
                opened_resources_validated: true,
                process_isolation: false,
                network: p::NetworkControl::Controlled {
                    destinations: vec![destination],
                    audit_ready: true,
                },
            },
            resources: Vec::new(),
            concrete_change_id: None,
            recovery: p::Recovery::NotGuaranteed,
            approvals: Vec::new(),
            denied_actions: Vec::new(),
            grants: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foundry::agent_runtime::cycle::ToolAuthorizer;
    use crate::foundry::db::Database;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    struct Fixture {
        database: Database,
        conversation: i64,
    }

    impl Fixture {
        fn new() -> Self {
            let database = Database::open_in_memory().expect("database");
            let conversation = database
                .create_conversation("fixture-model")
                .expect("conversation")
                .id;
            Self {
                database,
                conversation,
            }
        }

        fn store(&self) -> AgentStore<'_> {
            self.database.agent_store()
        }
    }

    fn network_call(
        store: &AgentStore<'_>,
        conversation: i64,
        url: &str,
    ) -> (p::TrustedFacts, ToolCall) {
        let run = store.start_run(conversation, "network-tools").expect("run");
        let call = ToolCall {
            id: "call-a".into(),
            name: "http_request".into(),
            arguments: json!({"method": "GET", "url": url}),
        };
        let record = store
            .propose_call(run.id, &call.id, &call.name, &call.arguments)
            .expect("proposal");
        let revision = store
            .authorization_revision(conversation)
            .expect("revision");
        let facts = NetworkAuthorizer
            .facts(store, &revision, &record, &call)
            .expect("facts");
        (facts, call)
    }

    fn spec() -> ToolSpec {
        let mut registry = ToolRegistry::default();
        register(&mut registry).expect("register");
        registry
            .get("http_request")
            .expect("registered")
            .spec
            .clone()
    }

    fn serve(listener: TcpListener) -> std::thread::JoinHandle<()> {
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = [0u8; 8192];
            let _ = stream.read(&mut request);
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                .expect("write");
        })
    }

    #[test]
    fn a_controlled_destination_is_allowed_or_prompts_for_the_destination() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let (facts, _) = network_call(&store, fixture.conversation, "https://example.com/");
        let decision = p::evaluate(&facts.action, &facts);
        let allowed = match &decision {
            p::Decision::Allow => true,
            p::Decision::Ask(prompts) => prompts.iter().any(|prompt| {
                matches!(
                    prompt.requirement,
                    p::Requirement::NetworkDestination { .. }
                )
            }),
            p::Decision::Deny(_) => false,
        };
        assert!(allowed, "unexpected decision: {decision:?}");
    }

    #[test]
    fn a_non_http_destination_is_rejected() {
        let fixture = Fixture::new();
        let store = fixture.store();
        let (record, revision) = {
            let run = store
                .start_run(fixture.conversation, "network-tools")
                .expect("run");
            let call = ToolCall {
                id: "call-a".into(),
                name: "http_request".into(),
                arguments: json!({"method": "GET", "url": "file:///etc/passwd"}),
            };
            let record = store
                .propose_call(run.id, &call.id, &call.name, &call.arguments)
                .expect("proposal");
            let revision = store
                .authorization_revision(fixture.conversation)
                .expect("revision");
            let _ = call;
            (record, revision)
        };
        let call = ToolCall {
            id: "call-a".into(),
            name: "http_request".into(),
            arguments: json!({"method": "GET", "url": "file:///etc/passwd"}),
        };
        assert!(
            NetworkAuthorizer
                .facts(&store, &revision, &record, &call)
                .is_err()
        );
    }

    #[tokio::test]
    async fn the_executor_gets_the_authorized_destination() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let server = serve(listener);

        let fixture = Fixture::new();
        let store = fixture.store();
        let (facts, call) = network_call(&store, fixture.conversation, &url);

        let mut executor = NetworkExecutor;
        let host = ToolHost {
            store: &store,
            facts: &facts,
        };
        let execution = executor.execute(&call, &spec(), &host).await;
        server.join().expect("join");

        match execution {
            ToolExecution::Succeeded { summary } => {
                assert_eq!(summary["status"], "succeeded");
                assert_eq!(summary["status_code"], 200);
                assert_eq!(summary["bytes"], 2);
                assert_eq!(summary["truncated"], false);
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_executor_records_a_network_event() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        let server = serve(listener);

        let fixture = Fixture::new();
        let store = fixture.store();
        let (facts, call) = network_call(&store, fixture.conversation, &url);

        let mut executor = NetworkExecutor;
        let host = ToolHost {
            store: &store,
            facts: &facts,
        };
        let execution = executor.execute(&call, &spec(), &host).await;
        server.join().expect("join");
        assert!(matches!(execution, ToolExecution::Succeeded { .. }));

        let recorded = store.facts(fixture.conversation).expect("facts");
        assert!(recorded.iter().any(|record| matches!(
            record.fact,
            ContextFact::NetworkEvent {
                state: NetworkEventState::Completed,
                http_status: Some(_),
                ..
            }
        )));
    }
}
