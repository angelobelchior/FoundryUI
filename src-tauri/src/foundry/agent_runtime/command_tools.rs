use serde_json::{Value, json};

use super::cycle::ToolAuthorizer;
use super::parser::ToolCall;
use super::schema::{
    RegisteredTool, ToolAvailability, ToolRegistry, ToolSpec, ToolSpecError, ToolUnavailable,
};
use crate::foundry::db::agent::authorization::AuthorizationRevision;
use crate::foundry::db::agent::{ActionRecord, AgentStore};
use crate::foundry::policy as p;

pub fn register(registry: &mut ToolRegistry) -> Result<(), ToolSpecError> {
    registry.register(RegisteredTool {
        spec: ToolSpec {
            name: "run_command".into(),
            description: "Run a command with proven process isolation.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "argv": {
                        "type": "array",
                        "items": { "type": "string" },
                        "minItems": 1,
                        "maxItems": 64
                    }
                },
                "required": ["argv"],
                "additionalProperties": false
            }),
        },
        availability: ToolAvailability::Unavailable {
            reason: ToolUnavailable::CapabilityBlocked,
        },
    })
}

pub struct CommandAuthorizer;

impl ToolAuthorizer for CommandAuthorizer {
    fn facts(
        &self,
        _store: &AgentStore<'_>,
        revision: &AuthorizationRevision,
        action: &ActionRecord,
        call: &ToolCall,
    ) -> Result<p::TrustedFacts, String> {
        let working_directory = p::Resource {
            root_id: revision
                .project_root_id()
                .map(str::to_string)
                .unwrap_or_else(|| "/".to_string()),
            object_id: "/".to_string(),
            version: "v1".to_string(),
        };
        let policy_action = p::Action {
            key: p::ActionKey {
                context: revision.key().clone(),
                run_id: action.run_id,
                action_id: action.id,
            },
            operation: p::Operation::Command {
                executable: "/usr/bin/true".into(),
                argv: command_argv(call),
                working_directory: working_directory.clone(),
                environment: Vec::new(),
            },
            arguments_fingerprint: action.arguments_fingerprint.clone(),
            resources: vec![working_directory.clone()],
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
            capability: p::Capability::Rejected,
            resources: vec![p::ResolvedResource {
                resource: working_directory,
                scope: if revision.project_root_id().is_some() {
                    p::ResourceScope::Project
                } else {
                    p::ResourceScope::Standalone
                },
            }],
            concrete_change_id: None,
            recovery: p::Recovery::NotGuaranteed,
            approvals: Vec::new(),
            denied_actions: Vec::new(),
            grants: Vec::new(),
        })
    }
}

fn command_argv(call: &ToolCall) -> Vec<String> {
    call.arguments
        .get("argv")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{CommandAuthorizer, register};
    use crate::foundry::agent_runtime::{
        ToolAuthorizer, ToolAvailability, ToolCall, ToolRegistry, ToolUnavailable,
    };
    use crate::foundry::db::Database;
    use crate::foundry::policy as p;
    use crate::foundry::resources::RootHandle;
    use serde_json::json;

    fn fixture() -> (Database, tempfile::TempDir, i64) {
        let database = Database::open_in_memory().expect("database");
        let directory = tempfile::tempdir().expect("directory");
        let root = RootHandle::enroll(directory.path()).expect("root");
        let store = database.agent_store();
        let conversation = store
            .create_project_conversation(None, "fixture-model")
            .expect("conversation")
            .conversation
            .id;
        store
            .register_authorization_root(conversation, 1, &root)
            .expect("root");
        (database, directory, conversation)
    }

    #[test]
    fn register_marks_run_command_as_capability_blocked() {
        let mut registry = ToolRegistry::default();
        register(&mut registry).expect("register");
        let tool = registry.get("run_command").expect("run_command");
        assert_eq!(tool.spec.name, "run_command");
        assert_eq!(
            tool.availability,
            ToolAvailability::Unavailable {
                reason: ToolUnavailable::CapabilityBlocked
            }
        );
    }

    #[test]
    fn run_command_is_excluded_from_model_definitions() {
        let mut registry = ToolRegistry::default();
        register(&mut registry).expect("register");
        assert!(registry.model_definitions().is_empty());
    }

    #[test]
    fn command_facts_are_denied_by_policy() {
        let (database, _directory, conversation) = fixture();
        let store = database.agent_store();
        let run = store.start_run(conversation, "request").expect("run");
        let record = store
            .propose_call(
                run.id,
                "call-command",
                "run_command",
                &json!({ "argv": ["echo", "hello"] }),
            )
            .expect("proposal");
        let revision = store
            .authorization_revision(conversation)
            .expect("revision");
        let call = ToolCall {
            id: "call-command".into(),
            name: "run_command".into(),
            arguments: json!({ "argv": ["echo", "hello"] }),
        };
        let facts = CommandAuthorizer
            .facts(&store, &revision, &record, &call)
            .expect("facts");
        assert!(matches!(
            p::evaluate(&facts.action, &facts),
            p::Decision::Deny(p::Denial::CapabilityRejected)
        ));
    }
}
