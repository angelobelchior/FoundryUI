use async_trait::async_trait;
use serde_json::{Value, json};

use super::cycle::{ToolExecution, ToolExecutor, ToolHost};
use super::parser::ToolCall;
use super::schema::{ToolAvailability, ToolRegistry, ToolSpec, ToolSpecError};
use crate::foundry::db::agent::authorization::{AuthorizationRevision, RequirementKind};
use crate::foundry::db::agent::{ActionRecord, AgentStore};
use crate::foundry::policy as p;
use crate::foundry::resources::{RelativePath, ResourceError, ResourceErrorKind};

const MAX_LIST_ENTRIES: usize = 2000;
const PLACEHOLDER_VERSION: &str = "list";

pub fn register(registry: &mut ToolRegistry) -> Result<(), ToolSpecError> {
    registry.register(super::schema::RegisteredTool {
        spec: ToolSpec {
            name: "list_files".into(),
            description: "List the files and directories inside an authorized directory.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "minLength": 1, "maxLength": 4096}
                },
                "additionalProperties": false
            }),
        },
        availability: ToolAvailability::Available,
    })
}

pub struct ListExecutor;

#[async_trait]
impl ToolExecutor for ListExecutor {
    async fn execute(
        &mut self,
        call: &ToolCall,
        _spec: &ToolSpec,
        host: &ToolHost<'_, '_>,
    ) -> ToolExecution {
        if !host.reads_current() {
            return list_failure("read_restricted");
        }
        let Some(resource) = host.facts.action.resources.first() else {
            return list_failure("list");
        };
        let conversation_id = host.facts.action.key.context.conversation_id;
        let action_id = host.facts.action.key.action_id;
        let root =
            match host
                .store
                .open_resource_root(conversation_id, &resource.root_id, action_id)
            {
                Ok(root) => root,
                Err(_) => return list_failure("list"),
            };
        let relative = match call
            .arguments
            .get("path")
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
        {
            Some(path) => match RelativePath::parse(path) {
                Ok(relative) => Some(relative),
                Err(_) => return list_failure("list"),
            },
            None => None,
        };
        let base_path = relative
            .as_ref()
            .map(|relative| relative.as_path().to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut entries = match root.list_directory(relative.as_ref()) {
            Ok(entries) => entries,
            Err(error) => return resource_failure(error),
        };
        if !host.reads_current() {
            return list_failure("read_restricted");
        }
        let truncated = entries.len() > MAX_LIST_ENTRIES;
        if truncated {
            entries.truncate(MAX_LIST_ENTRIES);
        }
        let entries = serde_json::to_value(&entries).unwrap_or_else(|_| Value::Array(Vec::new()));
        ToolExecution::Succeeded {
            summary: json!({
                "status": "succeeded",
                "path": base_path,
                "entries": entries,
                "truncated": truncated,
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
        | ResourceErrorKind::WriteIncomplete => "list",
    };
    ToolExecution::Failed {
        summary: json!({"status": "failed", "code": code}),
    }
}

fn list_failure(code: &str) -> ToolExecution {
    ToolExecution::Failed {
        summary: json!({"status": "failed", "code": code}),
    }
}

pub struct ListAuthorizer;

impl super::cycle::ToolAuthorizer for ListAuthorizer {
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
            .filter(|path| !path.is_empty())
            .unwrap_or(".")
            .to_string();
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
            object_id,
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
                .create_project(&root, Some("ListTools"))
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

    fn call(arguments: Value) -> ToolCall {
        ToolCall {
            id: "call-a".into(),
            name: "list_files".into(),
            arguments,
        }
    }

    fn spec() -> ToolSpec {
        let mut registry = ToolRegistry::default();
        register(&mut registry).expect("register");
        registry
            .get("list_files")
            .expect("registered")
            .spec
            .clone()
    }

    fn propose(
        store: &AgentStore<'_>,
        conversation: i64,
        arguments: &Value,
    ) -> (ActionRecord, AuthorizationRevision) {
        let run = store.start_run(conversation, "list-tools").expect("run");
        let record = store
            .propose_call(run.id, "call-a", "list_files", arguments)
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
        ListAuthorizer
            .facts(store, revision, action, call)
            .expect("facts")
    }

    fn execute(store: &AgentStore<'_>, trusted: &p::TrustedFacts, call: &ToolCall) -> ToolExecution {
        let mut executor = ListExecutor;
        let host = ToolHost {
            store,
            facts: trusted,
        };
        block_on(executor.execute(call, &spec(), &host))
    }

    #[test]
    fn an_in_scope_list_is_allowed_without_prompts() {
        let fixture = Fixture::project();
        let store = fixture.store();
        let arguments = json!({});
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        assert_eq!(p::evaluate(&trusted.action, &trusted), p::Decision::Allow);
    }

    #[test]
    fn the_executor_lists_entries_inside_the_root() {
        let fixture = Fixture::project();
        std::fs::write(fixture._directory.path().join("a.txt"), b"hello").expect("write");
        std::fs::create_dir(fixture._directory.path().join("src")).expect("mkdir");
        let store = fixture.store();
        let arguments = json!({});
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let execution = execute(&store, &trusted, &call);

        match execution {
            ToolExecution::Succeeded { summary } => {
                assert_eq!(summary["status"], "succeeded");
                assert_eq!(summary["truncated"], false);
                let entries = summary["entries"].as_array().expect("entries");
                let names = entries
                    .iter()
                    .map(|entry| entry["name"].as_str().expect("name"))
                    .collect::<Vec<_>>();
                assert!(names.contains(&"a.txt"));
                assert!(names.contains(&"src"));
                let src = entries
                    .iter()
                    .find(|entry| entry["name"] == "src")
                    .expect("src entry");
                assert_eq!(src["kind"], "directory");
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[test]
    fn the_executor_lists_a_nested_directory() {
        let fixture = Fixture::project();
        std::fs::create_dir(fixture._directory.path().join("src")).expect("mkdir");
        std::fs::write(
            fixture._directory.path().join("src/file.txt"),
            b"content",
        )
        .expect("write");
        let store = fixture.store();
        let arguments = json!({"path": "src"});
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let execution = execute(&store, &trusted, &call);

        match execution {
            ToolExecution::Succeeded { summary } => {
                assert_eq!(summary["path"], "src");
                let entries = summary["entries"].as_array().expect("entries");
                assert_eq!(entries.len(), 1);
                assert_eq!(entries[0]["name"], "file.txt");
                assert_eq!(entries[0]["kind"], "file");
            }
            other => panic!("expected success, got {other:?}"),
        }
    }

    #[test]
    fn changing_the_global_preference_blocks_a_prepared_list() {
        let fixture = Fixture::project();
        std::fs::write(
            fixture._directory.path().join("secret.txt"),
            b"synthetic-private",
        )
        .unwrap();
        let store = fixture.store();
        let arguments = json!({});
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);
        store.set_authorization_read_allowed(false).unwrap();

        let result = execute(&store, &trusted, &call);

        assert!(
            matches!(&result, ToolExecution::Failed { summary } if summary["code"] == "read_restricted")
        );
        assert!(!format!("{result:?}").contains("synthetic-private"));
    }
}
