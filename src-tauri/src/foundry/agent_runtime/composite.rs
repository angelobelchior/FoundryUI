use async_trait::async_trait;
use serde_json::json;

use super::change_tools::{ChangeAuthorizer, ChangeExecutor};
use super::command_tools::{CommandAuthorizer, CommandExecutor};
use super::context_search::{SearchAuthorizer, SearchExecutor};
use super::cycle::{ToolAuthorizer, ToolExecution, ToolExecutor, ToolHost};
use super::network_tools::{NetworkAuthorizer, NetworkExecutor};
use super::parser::ToolCall;
use super::read_tools::{ReadAuthorizer, ReadExecutor};
use super::schema::ToolSpec;
use crate::foundry::db::agent::authorization::AuthorizationRevision;
use crate::foundry::db::agent::{ActionRecord, AgentStore};
use crate::foundry::policy as p;

pub struct ToolExecutorSet;

#[async_trait]
impl ToolExecutor for ToolExecutorSet {
    async fn execute(
        &mut self,
        call: &ToolCall,
        spec: &ToolSpec,
        host: &ToolHost<'_, '_>,
    ) -> ToolExecution {
        match call.name.as_str() {
            "read_file" => ReadExecutor.execute(call, spec, host).await,
            "search_files" => SearchExecutor.execute(call, spec, host).await,
            "propose_file_change" => ChangeExecutor.execute(call, spec, host).await,
            "run_command" => CommandExecutor.execute(call, spec, host).await,
            "http_request" => NetworkExecutor.execute(call, spec, host).await,
            _ => ToolExecution::Failed {
                summary: json!({"status": "failed", "code": "unknown_tool"}),
            },
        }
    }
}

pub struct ToolAuthorizerSet;

impl ToolAuthorizer for ToolAuthorizerSet {
    fn facts(
        &self,
        store: &AgentStore<'_>,
        revision: &AuthorizationRevision,
        action: &ActionRecord,
        call: &ToolCall,
    ) -> Result<p::TrustedFacts, String> {
        match call.name.as_str() {
            "read_file" => ReadAuthorizer.facts(store, revision, action, call),
            "search_files" => SearchAuthorizer.facts(store, revision, action, call),
            "propose_file_change" => ChangeAuthorizer.facts(store, revision, action, call),
            "run_command" => CommandAuthorizer.facts(store, revision, action, call),
            "http_request" => NetworkAuthorizer.facts(store, revision, action, call),
            _ => Err("no authorizer is registered for this tool".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foundry::db::Database;
    use crate::foundry::resources::RootHandle;
    use serde_json::json;

    #[test]
    fn the_authorizer_set_dispatches_by_tool_name() {
        let database = Database::open_in_memory().expect("database");
        let directory = tempfile::tempdir().expect("directory");
        let root = RootHandle::enroll(directory.path()).expect("root");
        let store = database.agent_store();
        let project = store
            .create_project(&root, Some("fixture"))
            .expect("project");
        let conversation = store
            .create_project_conversation(Some(project.id), "fixture-model")
            .expect("conversation")
            .conversation
            .id;
        let run = store.start_run(conversation, "request").expect("run");
        let call = ToolCall {
            id: "call-a".into(),
            name: "read_file".into(),
            arguments: json!({"path": "a.txt"}),
        };
        let record = store
            .propose_call(run.id, &call.id, &call.name, &call.arguments)
            .expect("proposal");
        let revision = store
            .authorization_revision(conversation)
            .expect("revision");

        let facts = ToolAuthorizerSet
            .facts(&store, &revision, &record, &call)
            .expect("read facts");
        assert_eq!(p::evaluate(&facts.action, &facts), p::Decision::Allow);

        let unknown = ToolCall {
            id: "call-b".into(),
            name: "missing_tool".into(),
            arguments: json!({}),
        };
        assert!(
            ToolAuthorizerSet
                .facts(&store, &revision, &record, &unknown)
                .is_err()
        );
    }
}
