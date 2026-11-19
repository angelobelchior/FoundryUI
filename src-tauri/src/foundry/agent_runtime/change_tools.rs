use async_trait::async_trait;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::cycle::{ToolExecution, ToolExecutor, ToolHost};
use super::parser::ToolCall;
use super::schema::{RegisteredTool, ToolAvailability, ToolRegistry, ToolSpec, ToolSpecError};
use crate::foundry::db::agent::authorization::{AuthorizationRevision, RequirementKind};
use crate::foundry::db::agent::{ActionRecord, AgentStore, AgentStoreError, ContextFact};
use crate::foundry::git_protection::{self, ProtectionOperation};
use crate::foundry::policy as p;
use crate::foundry::resources::{RelativePath, ResourceError, RootHandle};

const MAX_CHANGE_BYTES: usize = 1024 * 1024;
const MAX_DIFF_BYTES: usize = 64 * 1024;

pub fn register(registry: &mut ToolRegistry) -> Result<(), ToolSpecError> {
    registry.register(RegisteredTool {
        spec: ToolSpec {
            name: "propose_file_change".into(),
            description: "Propose creating, editing or deleting an authorized file.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "operation": {"type": "string", "enum": ["create", "edit", "delete"]},
                    "path": {"type": "string", "minLength": 1, "maxLength": 4096},
                    "content": {"type": "string", "maxLength": 1048576},
                    "base_version": {"type": "string", "maxLength": 256}
                },
                "required": ["operation", "path"],
                "additionalProperties": false
            }),
        },
        availability: ToolAvailability::Available,
    })
}

struct ChangePlan {
    operation: ProtectionOperation,
    path: String,
    relative: RelativePath,
    base_fingerprint: Option<String>,
    proposed_fingerprint: String,
    proposal_fingerprint: String,
    proposed: Vec<u8>,
    diff: String,
    additions: usize,
    deletions: usize,
    diff_truncated: bool,
}

impl ChangePlan {
    fn change_id(&self) -> String {
        format!("change-{}", self.proposal_fingerprint)
    }

    fn fact(&self, resource_keys: Vec<String>) -> ContextFact {
        ContextFact::ChangeSet {
            operation: operation_name(self.operation).to_string(),
            path: self.path.clone(),
            base_fingerprint: self.base_fingerprint.clone(),
            proposed_fingerprint: Some(self.proposed_fingerprint.clone()),
            proposal_fingerprint: self.proposal_fingerprint.clone(),
            additions: self.additions as u32,
            deletions: self.deletions as u32,
            diff: self.diff.clone(),
            diff_truncated: self.diff_truncated,
            resource_keys,
        }
    }

    fn revalidate(&self, root: &RootHandle) -> bool {
        match self.operation {
            ProtectionOperation::Create => match root.open_read(&self.relative) {
                Ok(_) => false,
                Err(error) if error.is_missing() => true,
                Err(_) => false,
            },
            ProtectionOperation::Edit | ProtectionOperation::Delete => {
                match root
                    .open_read(&self.relative)
                    .and_then(|opened| opened.read_versioned(MAX_CHANGE_BYTES))
                {
                    Ok(read) => {
                        self.base_fingerprint.as_deref() == Some(&content_fingerprint(&read.bytes))
                    }
                    Err(_) => false,
                }
            }
        }
    }

    fn apply(&self, root: &RootHandle) -> Result<(), ResourceError> {
        match self.operation {
            ProtectionOperation::Create => root.create_new(&self.relative, &self.proposed),
            ProtectionOperation::Edit => root.rewrite(&self.relative, &self.proposed),
            ProtectionOperation::Delete => root.remove(&self.relative),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlanError {
    Conflict,
    Unavailable,
}

fn operation_name(operation: ProtectionOperation) -> &'static str {
    match operation {
        ProtectionOperation::Create => "create",
        ProtectionOperation::Edit => "edit",
        ProtectionOperation::Delete => "delete",
    }
}

fn plan_change(root: &RootHandle, call: &ToolCall) -> Result<ChangePlan, PlanError> {
    let operation = call
        .arguments
        .get("operation")
        .and_then(Value::as_str)
        .and_then(ProtectionOperation::parse)
        .ok_or(PlanError::Unavailable)?;
    let path = call
        .arguments
        .get("path")
        .and_then(Value::as_str)
        .ok_or(PlanError::Unavailable)?;
    let relative = RelativePath::parse(path).map_err(|_| PlanError::Unavailable)?;
    let proposed_content = call
        .arguments
        .get("content")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let expected_base = call.arguments.get("base_version").and_then(Value::as_str);

    let base = match operation {
        ProtectionOperation::Create => match root.open_read(&relative) {
            Ok(_) => return Err(PlanError::Conflict),
            Err(error) if error.is_missing() => Vec::new(),
            Err(_) => return Err(PlanError::Unavailable),
        },
        ProtectionOperation::Edit | ProtectionOperation::Delete => {
            match root
                .open_read(&relative)
                .and_then(|opened| opened.read_versioned(MAX_CHANGE_BYTES))
            {
                Ok(read) => {
                    if let Some(expected) = expected_base
                        && expected != content_fingerprint(&read.bytes)
                    {
                        return Err(PlanError::Conflict);
                    }
                    read.bytes
                }
                Err(error) if error.is_missing() => return Err(PlanError::Conflict),
                Err(_) => return Err(PlanError::Unavailable),
            }
        }
    };
    let proposed = if operation == ProtectionOperation::Delete {
        Vec::new()
    } else {
        proposed_content.as_bytes().to_vec()
    };
    let base_fingerprint = if operation == ProtectionOperation::Create {
        None
    } else {
        Some(content_fingerprint(&base))
    };
    let proposed_fingerprint = content_fingerprint(&proposed);
    let proposal_fingerprint = content_fingerprint(
        &serde_json::to_vec(&json!([
            operation_name(operation),
            path,
            base_fingerprint,
            proposed_fingerprint
        ]))
        .unwrap_or_default(),
    );
    let (diff, additions, deletions, diff_truncated) = diff(path, &base, &proposed);
    Ok(ChangePlan {
        operation,
        path: path.to_string(),
        relative,
        base_fingerprint,
        proposed_fingerprint,
        proposal_fingerprint,
        proposed,
        diff,
        additions,
        deletions,
        diff_truncated,
    })
}

fn diff(path: &str, base: &[u8], proposed: &[u8]) -> (String, usize, usize, bool) {
    let mut output = format!("--- a/{path}\n+++ b/{path}\n");
    let mut additions = 0usize;
    let mut deletions = 0usize;
    for line in String::from_utf8_lossy(base).lines() {
        output.push('-');
        output.push_str(line);
        output.push('\n');
        deletions += 1;
    }
    for line in String::from_utf8_lossy(proposed).lines() {
        output.push('+');
        output.push_str(line);
        output.push('\n');
        additions += 1;
    }
    let mut truncated = false;
    if output.len() > MAX_DIFF_BYTES {
        let marker = "\n... [diff truncated]";
        let usable = MAX_DIFF_BYTES.saturating_sub(marker.len());
        let mut end = usable;
        while end > 0 && !output.is_char_boundary(end) {
            end -= 1;
        }
        output.truncate(end);
        output.push_str(marker);
        truncated = true;
    }
    (output, additions, deletions, truncated)
}

pub struct ChangeExecutor;

#[async_trait]
impl ToolExecutor for ChangeExecutor {
    async fn execute(
        &mut self,
        call: &ToolCall,
        _spec: &ToolSpec,
        host: &ToolHost<'_, '_>,
    ) -> ToolExecution {
        let Some(resource) = host.facts.action.resources.first() else {
            return change_failure("change");
        };
        let conversation_id = host.facts.action.key.context.conversation_id;
        let context_revision = host.facts.action.key.context.context_revision;
        let run_id = host.facts.action.key.run_id;
        let action_id = host.facts.action.key.action_id;
        let root =
            match host
                .store
                .open_resource_root(conversation_id, &resource.root_id, action_id)
            {
                Ok(root) => root,
                Err(_) => return change_failure("change"),
            };
        let plan = match plan_change(&root, call) {
            Ok(plan) => plan,
            Err(PlanError::Conflict) => return change_conflict(),
            Err(PlanError::Unavailable) => return change_failure("change"),
        };
        if host.facts.concrete_change_id.as_deref() != Some(plan.change_id().as_str()) {
            return change_conflict();
        }
        let change_record = match host.store.record_fact(
            conversation_id,
            context_revision,
            Some(run_id),
            &format!("change-{action_id}"),
            &plan.fact(vec![resource.root_id.clone()]),
        ) {
            Ok(record) => record,
            Err(AgentStoreError::Conflict) => return change_conflict(),
            Err(_) => return change_failure("change"),
        };
        if !plan.revalidate(&root) {
            return change_conflict();
        }
        let inspection = git_protection::inspect(&root, plan.operation, &plan.relative);
        let prepared = git_protection::prepare(
            &root,
            plan.operation,
            &plan.relative,
            &format!("change-{action_id}"),
        );
        if let Ok(protection) = &prepared {
            let _ = host.store.record_fact(
                conversation_id,
                context_revision,
                Some(run_id),
                &format!("recovery-{action_id}"),
                &ContextFact::RecoveryRecord {
                    change_set_id: change_record.id,
                    git_reference: protection.reference.clone(),
                },
            );
        }
        if !plan.revalidate(&root) {
            return change_conflict();
        }
        match plan.apply(&root) {
            Ok(()) => {
                let undo_available = prepared.is_ok();
                let mut recovery = serde_json::to_value(&inspection).unwrap_or_else(|_| json!({}));
                if let Ok(protection) = &prepared {
                    recovery["reference"] = json!(protection.reference);
                    recovery["commit"] = json!(protection.commit);
                }
                recovery["undoAvailable"] = json!(undo_available);
                ToolExecution::Succeeded {
                    summary: json!({
                        "status": "applied",
                        "operation": operation_name(plan.operation),
                        "path": plan.path,
                        "changeId": plan.change_id(),
                        "baseFingerprint": plan.base_fingerprint,
                        "appliedFingerprint": plan.proposed_fingerprint,
                        "additions": plan.additions,
                        "deletions": plan.deletions,
                        "diff": plan.diff,
                        "diffTruncated": plan.diff_truncated,
                        "undoAvailable": undo_available,
                        "recovery": recovery,
                    }),
                }
            }
            Err(error) => ToolExecution::Failed {
                summary: json!({
                    "status": "failed",
                    "code": "write_failed",
                    "operation": operation_name(plan.operation),
                    "path": plan.path,
                    "reason": error.kind,
                    "recovery": inspection,
                    "undoAvailable": false,
                }),
            },
        }
    }
}

fn change_conflict() -> ToolExecution {
    ToolExecution::Failed {
        summary: json!({"status": "failed", "code": "conflict"}),
    }
}

fn change_failure(code: &str) -> ToolExecution {
    ToolExecution::Failed {
        summary: json!({"status": "failed", "code": code}),
    }
}

fn content_fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub struct ChangeAuthorizer;

impl super::cycle::ToolAuthorizer for ChangeAuthorizer {
    fn facts(
        &self,
        store: &AgentStore<'_>,
        revision: &AuthorizationRevision,
        action: &ActionRecord,
        call: &ToolCall,
    ) -> Result<p::TrustedFacts, String> {
        let operation = call
            .arguments
            .get("operation")
            .and_then(Value::as_str)
            .ok_or_else(|| "propose_file_change requires an operation argument".to_string())?;
        let object_id = call
            .arguments
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| "propose_file_change requires a path argument".to_string())?;
        let base_version = call
            .arguments
            .get("base_version")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty());
        let file_operation = match operation {
            "create" => p::FileOperation::Create,
            "edit" => p::FileOperation::Edit,
            "delete" => p::FileOperation::Delete,
            _ => return Err("unsupported file operation".to_string()),
        };
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
        let root = store
            .open_resource_root(revision.key().conversation_id, &root_id, action.id)
            .map_err(|error| error.to_string())?;
        let plan = plan_change(&root, call).map_err(|error| match error {
            PlanError::Conflict => {
                "the proposed change conflicts with the current file".to_string()
            }
            PlanError::Unavailable => "the proposed change could not be inspected".to_string(),
        })?;
        store
            .record_fact(
                revision.key().conversation_id,
                revision.key().context_revision,
                Some(action.run_id),
                &format!("change-{}", action.id),
                &plan.fact(vec![root_id.clone()]),
            )
            .map_err(|_| "the proposed change could not be recorded".to_string())?;
        let resource = p::Resource {
            root_id,
            object_id: object_id.to_string(),
            version: base_version.unwrap_or("absent").to_string(),
        };
        let effects = match file_operation {
            p::FileOperation::Create | p::FileOperation::Edit => p::Effects::Alteration,
            p::FileOperation::Delete => p::Effects::Destructive,
            p::FileOperation::Read => p::Effects::ReadOnly,
        };
        let read_evidence = match file_operation {
            p::FileOperation::Create => p::ReadEvidence::ProvenNoReads,
            _ => p::ReadEvidence::ProvenReads {
                resources: vec![resource.clone()],
            },
        };
        let policy_action = p::Action {
            key: p::ActionKey {
                context: revision.key().clone(),
                run_id: action.run_id,
                action_id: action.id,
            },
            operation: p::Operation::File(file_operation),
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
            read_evidence,
            capability: p::Capability::Proven {
                effects,
                opened_resources_validated: true,
                process_isolation: false,
                network: p::NetworkControl::Disabled,
            },
            resources: vec![p::ResolvedResource { resource, scope }],
            concrete_change_id: Some(plan.change_id()),
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
                .create_project(&root, Some("ChangeTools"))
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

    fn dummy_spec() -> ToolSpec {
        ToolSpec {
            name: "propose_file_change".into(),
            description: String::new(),
            parameters: json!({}),
        }
    }

    fn call(arguments: Value) -> ToolCall {
        ToolCall {
            id: "call-a".into(),
            name: "propose_file_change".into(),
            arguments,
        }
    }

    fn propose(
        store: &AgentStore<'_>,
        conversation: i64,
        arguments: &Value,
    ) -> (ActionRecord, AuthorizationRevision) {
        let run = store.start_run(conversation, "change-tools").expect("run");
        let record = store
            .propose_call(run.id, "call-a", "propose_file_change", arguments)
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
        ChangeAuthorizer
            .facts(store, revision, action, call)
            .expect("facts")
    }

    fn run_executor(
        store: &AgentStore<'_>,
        trusted: &p::TrustedFacts,
        call: &ToolCall,
    ) -> ToolExecution {
        let mut executor = ChangeExecutor;
        let host = ToolHost {
            store,
            facts: trusted,
        };
        block_on(executor.execute(call, &dummy_spec(), &host))
    }

    #[test]
    fn create_proposal_requires_alteration_review() {
        let fixture = Fixture::project();
        let store = fixture.store();
        let arguments = json!({"operation": "create", "path": "a.txt", "content": "hello"});
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        match p::evaluate(&trusted.action, &trusted) {
            p::Decision::Ask(prompts) => assert!(
                prompts.iter().any(|prompt| matches!(
                    prompt.requirement,
                    p::Requirement::AlterationReview { .. }
                )),
                "expected an alteration review prompt, got {prompts:?}"
            ),
            p::Decision::Allow => panic!("expected Ask, got Allow"),
            p::Decision::Deny(denial) => panic!("expected Ask, got Deny {denial:?}"),
        }
    }

    #[test]
    fn delete_proposal_requires_destructive_confirmation() {
        let fixture = Fixture::project();
        std::fs::write(fixture._directory.path().join("a.txt"), b"content").expect("write");
        let store = fixture.store();
        let arguments = json!({"operation": "delete", "path": "a.txt"});
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        match p::evaluate(&trusted.action, &trusted) {
            p::Decision::Ask(prompts) => assert!(
                prompts.iter().any(|prompt| matches!(
                    prompt.requirement,
                    p::Requirement::DestructiveConfirmation { .. }
                )),
                "expected a destructive confirmation prompt, got {prompts:?}"
            ),
            p::Decision::Allow => panic!("expected Ask, got Allow"),
            p::Decision::Deny(denial) => panic!("expected Ask, got Deny {denial:?}"),
        }
    }

    #[test]
    fn the_authorizer_records_a_change_set_with_the_exact_diff() {
        let fixture = Fixture::project();
        let store = fixture.store();
        let arguments = json!({
            "operation": "create",
            "path": "new.txt",
            "content": "hello from the proposal"
        });
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let change_id = trusted.concrete_change_id.as_deref().expect("change id");
        assert!(change_id.starts_with("change-"));
        let facts = store.facts(fixture.conversation).expect("facts");
        let change = facts
            .iter()
            .find_map(|fact| match &fact.fact {
                ContextFact::ChangeSet {
                    operation,
                    path,
                    diff,
                    additions,
                    deletions,
                    ..
                } if operation == "create" && path == "new.txt" => {
                    Some((diff.clone(), *additions, *deletions))
                }
                _ => None,
            })
            .expect("change set");
        assert_eq!(change.1, 1);
        assert_eq!(change.2, 0);
        assert!(change.0.contains("+hello from the proposal"));
    }

    #[test]
    fn the_executor_applies_a_create_and_reports_recovery() {
        let fixture = Fixture::project();
        let store = fixture.store();
        let arguments = json!({
            "operation": "create",
            "path": "new.txt",
            "content": "hello from the proposal"
        });
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let execution = run_executor(&store, &trusted, &call);

        match execution {
            ToolExecution::Succeeded { summary } => {
                assert_eq!(summary["status"], "applied");
                assert_eq!(summary["operation"], "create");
                assert_eq!(summary["path"], "new.txt");
                assert_eq!(summary["undoAvailable"], false);
            }
            other => panic!("expected success, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(fixture._directory.path().join("new.txt")).expect("read"),
            b"hello from the proposal"
        );
    }

    #[test]
    fn the_executor_applies_an_edit_and_preserves_the_new_content() {
        let fixture = Fixture::project();
        std::fs::write(fixture._directory.path().join("a.txt"), b"before").expect("write");
        let store = fixture.store();
        let arguments = json!({
            "operation": "edit",
            "path": "a.txt",
            "content": "after",
            "base_version": content_fingerprint(b"before")
        });
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let execution = run_executor(&store, &trusted, &call);

        match execution {
            ToolExecution::Succeeded { summary } => {
                assert_eq!(summary["status"], "applied");
                assert_eq!(summary["operation"], "edit");
                assert_eq!(summary["baseFingerprint"], content_fingerprint(b"before"));
                assert_eq!(summary["appliedFingerprint"], content_fingerprint(b"after"));
            }
            other => panic!("expected success, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(fixture._directory.path().join("a.txt")).expect("read"),
            b"after"
        );
    }

    #[test]
    fn the_executor_applies_a_delete_and_removes_the_file() {
        let fixture = Fixture::project();
        std::fs::write(fixture._directory.path().join("a.txt"), b"before").expect("write");
        let store = fixture.store();
        let arguments = json!({
            "operation": "delete",
            "path": "a.txt",
            "base_version": content_fingerprint(b"before")
        });
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let execution = run_executor(&store, &trusted, &call);

        match execution {
            ToolExecution::Succeeded { summary } => {
                assert_eq!(summary["status"], "applied");
                assert_eq!(summary["operation"], "delete");
            }
            other => panic!("expected success, got {other:?}"),
        }
        assert!(!fixture._directory.path().join("a.txt").exists());
    }

    #[test]
    fn a_mismatched_base_version_conflicts_and_does_not_write() {
        let fixture = Fixture::project();
        std::fs::write(fixture._directory.path().join("a.txt"), b"original").expect("write");
        let store = fixture.store();
        let valid = json!({
            "operation": "edit",
            "path": "a.txt",
            "content": "edited",
            "base_version": content_fingerprint(b"original")
        });
        let (record, revision) = propose(&store, fixture.conversation, &valid);
        let trusted = facts(&store, &revision, &record, &call(valid.clone()));
        let stale = json!({
            "operation": "edit",
            "path": "a.txt",
            "content": "edited",
            "base_version": content_fingerprint(b"different")
        });

        let execution = run_executor(&store, &trusted, &call(stale));

        match execution {
            ToolExecution::Failed { summary } => assert_eq!(summary["code"], "conflict"),
            other => panic!("expected failure, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(fixture._directory.path().join("a.txt")).expect("read"),
            b"original"
        );
    }

    #[test]
    fn an_external_change_invalidates_the_approval_before_apply() {
        let fixture = Fixture::project();
        std::fs::write(fixture._directory.path().join("a.txt"), b"before").expect("write");
        let store = fixture.store();
        let arguments = json!({
            "operation": "edit",
            "path": "a.txt",
            "content": "after",
            "base_version": content_fingerprint(b"before")
        });
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);
        std::fs::write(fixture._directory.path().join("a.txt"), b"user work").expect("write");

        let execution = run_executor(&store, &trusted, &call);

        match execution {
            ToolExecution::Failed { summary } => assert_eq!(summary["code"], "conflict"),
            other => panic!("expected failure, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(fixture._directory.path().join("a.txt")).expect("read"),
            b"user work"
        );
    }

    #[test]
    fn creating_a_file_that_already_exists_conflicts() {
        let fixture = Fixture::project();
        std::fs::write(fixture._directory.path().join("existing.txt"), b"existing").expect("write");
        let store = fixture.store();
        let valid = json!({"operation": "create", "path": "other.txt", "content": "new"});
        let (record, revision) = propose(&store, fixture.conversation, &valid);
        let trusted = facts(&store, &revision, &record, &call(valid));
        let arguments = json!({"operation": "create", "path": "existing.txt", "content": "new"});

        let execution = run_executor(&store, &trusted, &call(arguments));

        match execution {
            ToolExecution::Failed { summary } => assert_eq!(summary["code"], "conflict"),
            other => panic!("expected failure, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(fixture._directory.path().join("existing.txt")).expect("read"),
            b"existing"
        );
    }

    fn run_git(directory: &std::path::Path, args: &[&str]) -> bool {
        let Some(git) = crate::foundry::git_protection::git_executable() else {
            return false;
        };
        std::process::Command::new(git)
            .current_dir(directory)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", "Fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.invalid")
            .env("GIT_COMMITTER_NAME", "Fixture")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.invalid")
            .status()
            .is_ok_and(|status| status.success())
    }

    fn initialize_repository(directory: &std::path::Path) -> bool {
        run_git(directory, &["init", "-q"])
            && run_git(directory, &["commit", "--allow-empty", "-qm", "fixture"])
    }

    #[test]
    fn an_untracked_target_applies_with_an_unambiguous_warning() {
        let fixture = Fixture::project();
        assert!(initialize_repository(fixture._directory.path()));
        std::fs::write(fixture._directory.path().join("notes.txt"), b"user work").expect("write");
        let store = fixture.store();
        let arguments = json!({
            "operation": "edit",
            "path": "notes.txt",
            "content": "agent change",
            "base_version": content_fingerprint(b"user work")
        });
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let execution = run_executor(&store, &trusted, &call);

        match execution {
            ToolExecution::Succeeded { summary } => {
                assert_eq!(summary["status"], "applied");
                assert_eq!(summary["undoAvailable"], false);
                assert_eq!(summary["recovery"]["warning"]["code"], "unprotected_files");
                assert_eq!(summary["recovery"]["files"][0]["eligible"], false);
            }
            other => panic!("expected success, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(fixture._directory.path().join("notes.txt")).expect("read"),
            b"agent change"
        );
    }

    #[test]
    fn a_tracked_dirty_target_applies_and_reports_git_ready_with_undo() {
        let fixture = Fixture::project();
        assert!(initialize_repository(fixture._directory.path()));
        std::fs::write(fixture._directory.path().join("tracked.txt"), b"committed").expect("write");
        assert!(run_git(fixture._directory.path(), &["add", "tracked.txt"]));
        assert!(run_git(
            fixture._directory.path(),
            &["commit", "-qm", "tracked"]
        ));
        std::fs::write(
            fixture._directory.path().join("tracked.txt"),
            b"user worktree",
        )
        .expect("write");
        let store = fixture.store();
        let arguments = json!({
            "operation": "edit",
            "path": "tracked.txt",
            "content": "agent change",
            "base_version": content_fingerprint(b"user worktree")
        });
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let execution = run_executor(&store, &trusted, &call);

        match execution {
            ToolExecution::Succeeded { summary } => {
                assert_eq!(summary["status"], "applied");
                assert_eq!(summary["undoAvailable"], true);
                assert_eq!(summary["recovery"]["repository"], "work_tree");
                assert!(summary["recovery"]["warning"].is_null());
                assert_eq!(summary["recovery"]["files"][0]["eligible"], true);
                assert!(
                    summary["recovery"]["reference"]
                        .as_str()
                        .is_some_and(|reference| reference.starts_with("refs/foundryui/recovery/"))
                );
            }
            other => panic!("expected success, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(fixture._directory.path().join("tracked.txt")).expect("read"),
            b"agent change"
        );
    }

    #[test]
    fn the_executor_records_a_change_set_and_recovery_fact() {
        let fixture = Fixture::project();
        assert!(initialize_repository(fixture._directory.path()));
        std::fs::write(fixture._directory.path().join("tracked.txt"), b"committed").expect("write");
        assert!(run_git(fixture._directory.path(), &["add", "tracked.txt"]));
        assert!(run_git(
            fixture._directory.path(),
            &["commit", "-qm", "tracked"]
        ));
        let store = fixture.store();
        let arguments = json!({
            "operation": "edit",
            "path": "tracked.txt",
            "content": "hello",
            "base_version": content_fingerprint(b"committed")
        });
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        let execution = run_executor(&store, &trusted, &call);
        assert!(matches!(execution, ToolExecution::Succeeded { .. }));

        let facts = store.facts(fixture.conversation).expect("facts");
        assert!(
            facts
                .iter()
                .any(|fact| matches!(fact.fact, ContextFact::ChangeSet { .. })),
            "expected a change set fact to be recorded"
        );
        assert!(
            facts
                .iter()
                .any(|fact| matches!(fact.fact, ContextFact::RecoveryRecord { .. })),
            "expected a recovery record fact to be recorded"
        );
    }
}
