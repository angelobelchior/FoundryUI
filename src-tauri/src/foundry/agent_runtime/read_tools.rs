use async_trait::async_trait;
use serde_json::{Value, json};

use super::cycle::{ToolExecution, ToolExecutor, ToolHost};
use super::parser::ToolCall;
use super::schema::{ToolAvailability, ToolRegistry, ToolSpec, ToolSpecError};
use crate::foundry::db::agent::authorization::{AuthorizationRevision, RequirementKind};
use crate::foundry::db::agent::{ActionRecord, AgentStore};
use crate::foundry::policy as p;
use crate::foundry::resources::{RelativePath, ResourceError, ResourceErrorKind};

const MAX_READ_BYTES: usize = 256 * 1024;
const MAX_READ_HARD_CAP: usize = 16 * 1024 * 1024;
const PLACEHOLDER_VERSION: &str = "read";

pub fn register(registry: &mut ToolRegistry) -> Result<(), ToolSpecError> {
    registry.register(super::schema::RegisteredTool {
        spec: ToolSpec {
            name: "read_file".into(),
            description: "Read the bounded contents of an authorized file.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "minLength": 1, "maxLength": 4096}
                },
                "required": ["path"],
                "additionalProperties": false
            }),
        },
        availability: ToolAvailability::Available,
    })
}

pub struct ReadExecutor;

#[async_trait]
impl ToolExecutor for ReadExecutor {
    async fn execute(
        &mut self,
        call: &ToolCall,
        _spec: &ToolSpec,
        host: &ToolHost<'_, '_>,
    ) -> ToolExecution {
        if !host.reads_current() {
            return read_failure("read_restricted");
        }
        let Some(path) = call.arguments.get("path").and_then(Value::as_str) else {
            return read_failure("read");
        };
        let Some(resource) = host.facts.action.resources.first() else {
            return read_failure("read");
        };
        let conversation_id = host.facts.action.key.context.conversation_id;
        let action_id = host.facts.action.key.action_id;
        let root =
            match host
                .store
                .open_resource_root(conversation_id, &resource.root_id, action_id)
            {
                Ok(root) => root,
                Err(_) => return read_failure("read"),
            };
        let relative = match RelativePath::parse(path) {
            Ok(relative) => relative,
            Err(_) => return read_failure("read"),
        };
        let mut bytes = match root.read(&relative, MAX_READ_HARD_CAP) {
            Ok(bytes) => bytes,
            Err(error) => return resource_failure(error),
        };
        let truncated = bytes.len() > MAX_READ_BYTES;
        if truncated {
            bytes.truncate(MAX_READ_BYTES);
        }
        if !host.reads_current() {
            return read_failure("read_restricted");
        }
        ToolExecution::Succeeded {
            summary: json!({
                "status": "succeeded",
                "content": String::from_utf8_lossy(&bytes),
                "bytes": bytes.len(),
                "truncated": truncated
            }),
        }
    }
}

fn resource_failure(error: ResourceError) -> ToolExecution {
    let code = match error.kind {
        ResourceErrorKind::Unavailable => "not_found",
        ResourceErrorKind::AccessDenied
        | ResourceErrorKind::Revoked
        | ResourceErrorKind::InvalidPath
        | ResourceErrorKind::NotDirectory
        | ResourceErrorKind::UnsupportedResourceType
        | ResourceErrorKind::LimitExceeded
        | ResourceErrorKind::ResourceChanged
        | ResourceErrorKind::RootChanged
        | ResourceErrorKind::UnsupportedVolume
        | ResourceErrorKind::UnsupportedPlatform
        | ResourceErrorKind::OperationContainmentUnavailable
        | ResourceErrorKind::AlreadyExists
        | ResourceErrorKind::WriteIncomplete => "read",
    };
    ToolExecution::Failed {
        summary: json!({"status": "failed", "code": code}),
    }
}

fn read_failure(code: &str) -> ToolExecution {
    ToolExecution::Failed {
        summary: json!({"status": "failed", "code": code}),
    }
}

pub struct ReadAuthorizer;

impl super::cycle::ToolAuthorizer for ReadAuthorizer {
    fn facts(
        &self,
        store: &AgentStore<'_>,
        revision: &AuthorizationRevision,
        action: &ActionRecord,
        call: &ToolCall,
    ) -> Result<p::TrustedFacts, String> {
        let object_id = call
            .arguments
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| "read_file requires a path argument".to_string())?;
        let (root_id, scope) = match revision.project_root_id() {
            Some(project_root_id) => (project_root_id.to_string(), p::ResourceScope::Project),
            None => {
                let root_id = store
                    .list_grants(revision.key().conversation_id)
                    .map_err(|error| error.to_string())?
                    .into_iter()
                    .find(|grant| grant.requirement.kind == RequirementKind::StandaloneRoot)
                    .and_then(|grant| grant.requirement.root_identity_key)
                    .ok_or_else(|| "no authorized standalone root".to_string())?;
                (root_id, p::ResourceScope::Standalone)
            }
        };
        let resource = p::Resource {
            root_id,
            object_id: object_id.to_string(),
            version: PLACEHOLDER_VERSION.to_string(),
        };
        let policy_action = p::Action {
            key: p::ActionKey {
                context: revision.key().clone(),
                run_id: action.run_id,
                action_id: action.id,
            },
            operation: p::Operation::File(p::FileOperation::Read),
            arguments_fingerprint: action.arguments_fingerprint.clone(),
            resources: vec![resource.clone()],
            origin: p::InstructionOrigin::Model,
        };
        Ok(p::TrustedFacts {
            current_key: policy_action.key.clone(),
            action: policy_action,
            mode: revision.mode(),
            mode_at_proposal: revision.mode(),
            project_root_id: revision.project_root_id().map(str::to_string),
            global_read_allowed: revision.global_read_allowed(),
            read_evidence: p::ReadEvidence::ProvenReads {
                resources: vec![resource.clone()],
            },
            capability: p::Capability::Proven {
                effects: p::Effects::ReadOnly,
                opened_resources_validated: true,
                process_isolation: false,
                network: p::NetworkControl::Disabled,
            },
            resources: vec![p::ResolvedResource { resource, scope }],
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
    use crate::foundry::resources::RootHandle;
    use serde_json::json;
    use std::future::Future;

    struct Fixture {
        database: Database,
        _directory: tempfile::TempDir,
        conversation: i64,
    }

    impl Fixture {
        fn project() -> Self {
            let database = Database::open_in_memory().expect("database");
            let directory = tempfile::tempdir().expect("directory");
            let root = RootHandle::enroll(directory.path()).expect("root");
            let store = database.agent_store();
            let project = store
                .create_project(&root, Some("ReadTools"))
                .expect("project");
            let conversation = store
                .create_project_conversation(Some(project.id), "fixture-model")
                .expect("conversation")
                .conversation
                .id;
            Self {
                database,
                _directory: directory,
                conversation,
            }
        }

        fn store(&self) -> AgentStore<'_> {
            self.database.agent_store()
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(future)
    }

    fn call(path: &str) -> ToolCall {
        ToolCall {
            id: "call-a".into(),
            name: "read_file".into(),
            arguments: json!({"path": path}),
        }
    }

    fn propose(
        store: &AgentStore<'_>,
        conversation: i64,
        path: &str,
    ) -> (ActionRecord, AuthorizationRevision) {
        let run = store.start_run(conversation, "read-tools").expect("run");
        let record = store
            .propose_call(run.id, "call-a", "read_file", &json!({"path": path}))
            .expect("proposal");
        let revision = store
            .authorization_revision(conversation)
            .expect("revision");
        (record, revision)
    }

    fn facts(
        store: &AgentStore<'_>,
        revision: &AuthorizationRevision,
        action: &ActionRecord,
        call: &ToolCall,
    ) -> p::TrustedFacts {
        ReadAuthorizer
            .facts(store, revision, action, call)
            .expect("facts")
    }

    #[test]
    fn an_in_scope_read_is_allowed_without_prompts() {
        let fixture = Fixture::project();
        let store = fixture.store();
        let (record, revision) = propose(&store, fixture.conversation, "a.txt");
        let call = call("a.txt");
        let trusted = facts(&store, &revision, &record, &call);

        assert_eq!(p::evaluate(&trusted.action, &trusted), p::Decision::Allow);
    }

    #[test]
    fn the_executor_reads_a_file_inside_the_root() {
        let fixture = Fixture::project();
        std::fs::write(
            fixture._directory.path().join("a.txt"),
            b"hello from the root",
        )
        .expect("write");
        let store = fixture.store();
        let (record, revision) = propose(&store, fixture.conversation, "a.txt");
        let call = call("a.txt");
        let trusted = facts(&store, &revision, &record, &call);

        let mut executor = ReadExecutor;
        let host = ToolHost {
            store: &store,
            facts: &trusted,
        };
        let execution = block_on(executor.execute(
            &call,
            &ToolSpec {
                name: "read_file".into(),
                description: String::new(),
                parameters: json!({}),
            },
            &host,
        ));

        match execution {
            ToolExecution::Succeeded { summary } => {
                assert_eq!(summary["status"], "succeeded");
                assert_eq!(summary["content"], "hello from the root");
                assert_eq!(summary["bytes"], 19);
                assert_eq!(summary["truncated"], false);
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[test]
    fn the_executor_rejects_a_path_outside_the_root() {
        let fixture = Fixture::project();
        let store = fixture.store();
        let (record, revision) = propose(&store, fixture.conversation, "../outside.txt");
        let call = call("../outside.txt");
        let trusted = facts(&store, &revision, &record, &call);

        let mut executor = ReadExecutor;
        let host = ToolHost {
            store: &store,
            facts: &trusted,
        };
        let execution = block_on(executor.execute(
            &call,
            &ToolSpec {
                name: "read_file".into(),
                description: String::new(),
                parameters: json!({}),
            },
            &host,
        ));

        match execution {
            ToolExecution::Failed { summary } => {
                assert_eq!(summary["status"], "failed");
                assert!(summary["code"].is_string());
            }
            other => panic!("expected failure, got {other:?}"),
        }
    }

    #[test]
    fn the_executor_truncates_content_beyond_the_limit() {
        let fixture = Fixture::project();
        let oversized = vec![b'x'; MAX_READ_BYTES + 100];
        std::fs::write(fixture._directory.path().join("big.txt"), &oversized).expect("write");
        let store = fixture.store();
        let (record, revision) = propose(&store, fixture.conversation, "big.txt");
        let call = call("big.txt");
        let trusted = facts(&store, &revision, &record, &call);

        let mut executor = ReadExecutor;
        let host = ToolHost {
            store: &store,
            facts: &trusted,
        };
        let execution = block_on(executor.execute(
            &call,
            &ToolSpec {
                name: "read_file".into(),
                description: String::new(),
                parameters: json!({}),
            },
            &host,
        ));

        match execution {
            ToolExecution::Succeeded { summary } => {
                assert_eq!(summary["status"], "succeeded");
                assert_eq!(summary["bytes"], MAX_READ_BYTES);
                assert_eq!(summary["truncated"], true);
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[test]
    fn changing_the_global_preference_blocks_a_prepared_read() {
        let fixture = Fixture::project();
        std::fs::write(
            fixture._directory.path().join("secret.txt"),
            b"synthetic-private",
        )
        .unwrap();
        let store = fixture.store();
        let (record, revision) = propose(&store, fixture.conversation, "secret.txt");
        let call = call("secret.txt");
        let trusted = facts(&store, &revision, &record, &call);
        store.set_authorization_read_allowed(false).unwrap();
        let result = block_on(ReadExecutor.execute(
            &call,
            &ToolSpec {
                name: "read_file".into(),
                description: String::new(),
                parameters: json!({}),
            },
            &ToolHost {
                store: &store,
                facts: &trusted,
            },
        ));
        assert!(
            matches!(&result, ToolExecution::Failed { summary } if summary["code"] == "read_restricted")
        );
        assert!(!format!("{result:?}").contains("synthetic-private"));
    }
}
