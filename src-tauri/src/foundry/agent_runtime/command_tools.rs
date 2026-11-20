use async_trait::async_trait;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

use super::command_execution::{self, FileChange};
use super::cycle::{ToolExecution, ToolExecutor, ToolHost};
use super::parser::ToolCall;
use super::schema::{
    RegisteredTool, ToolAvailability, ToolRegistry, ToolSpec, ToolSpecError, ToolUnavailable,
};
use super::terminal;
use crate::foundry::db::agent::authorization::{AuthorizationRevision, RequirementKind};
use crate::foundry::db::agent::{ActionRecord, AgentStore, CommandFileChange, ContextFact};
use crate::foundry::policy as p;
use crate::foundry::resources::{RelativePath, RootHandle};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const TERMINAL_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);

fn command_availability() -> ToolAvailability {
    if command_execution::command_sandbox_available() {
        ToolAvailability::Available
    } else {
        ToolAvailability::Unavailable {
            reason: ToolUnavailable::CapabilityBlocked,
        }
    }
}

fn terminal_availability() -> ToolAvailability {
    if terminal::terminal_sandbox_available() {
        ToolAvailability::Available
    } else {
        ToolAvailability::Unavailable {
            reason: ToolUnavailable::CapabilityBlocked,
        }
    }
}

pub fn register(registry: &mut ToolRegistry) -> Result<(), ToolSpecError> {
    registry.register(RegisteredTool {
        spec: ToolSpec {
            name: "run_command".into(),
            description: "Run a command in a sandboxed staging area and review its file changes."
                .into(),
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
        availability: command_availability(),
    })?;
    registry.register(RegisteredTool {
        spec: ToolSpec {
            name: "terminal_run".into(),
            description: "Run a command in the persistent sandboxed terminal and review its file changes."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "minLength": 1, "maxLength": 8192 }
                },
                "required": ["command"],
                "additionalProperties": false
            }),
        },
        availability: terminal_availability(),
    })
}

pub struct CommandExecutor;

#[async_trait]
impl ToolExecutor for CommandExecutor {
    async fn execute(
        &mut self,
        _call: &ToolCall,
        _spec: &ToolSpec,
        host: &ToolHost<'_, '_>,
    ) -> ToolExecution {
        let Some(resource) = host.facts.action.resources.first() else {
            return command_failure("change");
        };
        let conversation_id = host.facts.action.key.context.conversation_id;
        let action_id = host.facts.action.key.action_id;
        let root =
            match host
                .store
                .open_resource_root(conversation_id, &resource.root_id, action_id)
            {
                Ok(root) => root,
                Err(_) => return command_failure("change"),
            };
        let fact = match host
            .store
            .fact_by_key(conversation_id, &format!("command-change-{action_id}"))
        {
            Ok(fact) => fact,
            Err(_) => return command_failure("change"),
        };
        let ContextFact::CommandChange {
            status,
            exit_code,
            stdout,
            stderr,
            files,
        } = &fact.fact
        else {
            return command_failure("change");
        };
        let mut applied = Vec::new();
        let mut conflicts = Vec::new();
        for file in files {
            let relative = match RelativePath::parse(&file.path) {
                Ok(relative) => relative,
                Err(_) => {
                    conflicts.push(file.path.clone());
                    continue;
                }
            };
            match apply_file(&root, &relative, file) {
                Ok(()) => applied.push(file.path.clone()),
                Err(ApplyError::Conflict) => conflicts.push(file.path.clone()),
                Err(ApplyError::Unavailable) => conflicts.push(file.path.clone()),
            }
        }
        if !conflicts.is_empty() {
            return ToolExecution::Failed {
                summary: json!({
                    "status": "failed",
                    "code": "conflict",
                    "applied": applied,
                    "conflicted": conflicts,
                }),
            };
        }
        let exit = exit_code.map(|code| json!(code)).unwrap_or(Value::Null);
        ToolExecution::Succeeded {
            summary: json!({
                "status": status,
                "exitCode": exit,
                "stdout": stdout,
                "stderr": stderr,
                "appliedFiles": applied,
            }),
        }
    }
}

enum ApplyError {
    Conflict,
    Unavailable,
}

fn apply_file(
    root: &RootHandle,
    relative: &RelativePath,
    file: &CommandFileChange,
) -> Result<(), ApplyError> {
    let proposed = file.content.as_bytes();
    match file.operation.as_str() {
        "create" => match root.open_read(relative) {
            Ok(_) => Err(ApplyError::Conflict),
            Err(error) if error.is_missing() => root
                .create_new(relative, proposed)
                .map_err(|_| ApplyError::Unavailable),
            Err(_) => Err(ApplyError::Unavailable),
        },
        "edit" => {
            let current = root
                .open_read(relative)
                .and_then(|opened| opened.read(MAX_FILE_BYTES))
                .map_err(|_| ApplyError::Conflict)?;
            if content_fingerprint(&current) != file.base_fingerprint {
                return Err(ApplyError::Conflict);
            }
            root.rewrite(relative, proposed)
                .map_err(|_| ApplyError::Unavailable)
        }
        "delete" => {
            let current = root
                .open_read(relative)
                .and_then(|opened| opened.read(MAX_FILE_BYTES))
                .map_err(|_| ApplyError::Conflict)?;
            if content_fingerprint(&current) != file.base_fingerprint {
                return Err(ApplyError::Conflict);
            }
            root.remove(relative).map_err(|_| ApplyError::Unavailable)
        }
        _ => Err(ApplyError::Unavailable),
    }
}

const MAX_FILE_BYTES: usize = 1024 * 1024;

fn command_failure(code: &str) -> ToolExecution {
    ToolExecution::Failed {
        summary: json!({"status": "failed", "code": code}),
    }
}

pub struct CommandAuthorizer;

impl super::cycle::ToolAuthorizer for CommandAuthorizer {
    fn facts(
        &self,
        store: &AgentStore<'_>,
        revision: &AuthorizationRevision,
        action: &ActionRecord,
        call: &ToolCall,
    ) -> Result<p::TrustedFacts, String> {
        let argv = command_argv(call);
        if argv.is_empty() || argv.len() > 64 || argv.iter().any(|arg| arg.contains('\0')) {
            return Err("run_command requires a valid argv argument".to_string());
        }
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
        let key = format!("command-change-{}", action.id);
        let existing = store.fact_by_key(revision.key().conversation_id, &key).ok();
        let (change_id, effects) = match &existing {
            Some(fact) => {
                let ContextFact::CommandChange { files, .. } = &fact.fact else {
                    return Err("the staged command change was malformed".to_string());
                };
                let change_set = store
                    .fact_by_key(
                        revision.key().conversation_id,
                        &format!("change-{}", action.id),
                    )
                    .ok();
                let change_id = change_set.and_then(|fact| {
                    if let ContextFact::ChangeSet {
                        proposal_fingerprint,
                        ..
                    } = &fact.fact
                    {
                        Some(format!("change-{proposal_fingerprint}"))
                    } else {
                        None
                    }
                });
                (change_id, effects_of(files))
            }
            None => {
                if !command_execution::command_sandbox_available() {
                    return Err("command isolation is unavailable on this platform".to_string());
                }
                let root = store
                    .open_resource_root(revision.key().conversation_id, &root_id, action.id)
                    .map_err(|error| error.to_string())?;
                let (run, changes) =
                    command_execution::capture_command_effects(&root, &argv, COMMAND_TIMEOUT)?;
                let files = changes.iter().map(command_file_change).collect::<Vec<_>>();
                let additions = changes
                    .iter()
                    .filter(|change| change.operation != "delete")
                    .count() as u32;
                let deletions = changes
                    .iter()
                    .filter(|change| change.operation == "delete")
                    .count() as u32;
                let diff = combined_diff(&changes);
                let change_id = if changes.is_empty() {
                    None
                } else {
                    Some(format!("change-{}", changes_fingerprint(&changes)))
                };
                let effects = effects_of(&files);
                if changes.is_empty() {
                    store
                        .record_fact(
                            revision.key().conversation_id,
                            revision.key().context_revision,
                            Some(action.run_id),
                            &key,
                            &ContextFact::CommandChange {
                                status: run_status(&run),
                                exit_code: run.exit_code,
                                stdout: run.stdout,
                                stderr: run.stderr,
                                files,
                            },
                        )
                        .map_err(|_| "the command change could not be recorded".to_string())?;
                } else {
                    let change_set = ContextFact::ChangeSet {
                        operation: "command".into(),
                        path: ".".into(),
                        base_fingerprint: None,
                        proposed_fingerprint: None,
                        proposal_fingerprint: changes_fingerprint(&changes),
                        additions,
                        deletions,
                        diff,
                        diff_truncated: false,
                        resource_keys: vec![root_id.clone()],
                    };
                    store
                        .record_fact(
                            revision.key().conversation_id,
                            revision.key().context_revision,
                            Some(action.run_id),
                            &format!("change-{}", action.id),
                            &change_set,
                        )
                        .map_err(|_| "the command change set could not be recorded".to_string())?;
                    store
                        .record_fact(
                            revision.key().conversation_id,
                            revision.key().context_revision,
                            Some(action.run_id),
                            &key,
                            &ContextFact::CommandChange {
                                status: run_status(&run),
                                exit_code: run.exit_code,
                                stdout: run.stdout,
                                stderr: run.stderr,
                                files,
                            },
                        )
                        .map_err(|_| "the command change could not be recorded".to_string())?;
                }
                (change_id, effects)
            }
        };
        let working_directory = p::Resource {
            root_id: root_id.clone(),
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
                executable: argv
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "/usr/bin/env".to_string()),
                argv: argv.clone(),
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
            capability: p::Capability::Proven {
                effects,
                opened_resources_validated: true,
                process_isolation: true,
                network: p::NetworkControl::Disabled,
            },
            resources: vec![p::ResolvedResource {
                resource: working_directory,
                scope,
            }],
            concrete_change_id: change_id,
            recovery: p::Recovery::NotGuaranteed,
            approvals: Vec::new(),
            denied_actions: Vec::new(),
            grants: Vec::new(),
        })
    }
}

fn effects_of(files: &[CommandFileChange]) -> p::Effects {
    if files.iter().any(|file| file.operation == "delete") {
        p::Effects::Destructive
    } else if files.is_empty() {
        p::Effects::ReadOnly
    } else {
        p::Effects::Alteration
    }
}

fn run_status(run: &command_execution::CommandRun) -> String {
    match run.status {
        command_execution::CommandStatus::Completed => "completed".into(),
        command_execution::CommandStatus::TimedOut => "timed_out".into(),
        command_execution::CommandStatus::Failed => "failed".into(),
        command_execution::CommandStatus::Unavailable => "unavailable".into(),
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

fn content_fingerprint(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn command_file_change(change: &FileChange) -> CommandFileChange {
    CommandFileChange {
        operation: change.operation.to_string(),
        path: change.path.clone(),
        base_fingerprint: content_fingerprint(&change.base),
        proposed_fingerprint: content_fingerprint(&change.proposed),
        content: String::from_utf8_lossy(&change.proposed).into_owned(),
    }
}

fn combined_diff(changes: &[FileChange]) -> String {
    let mut output = String::new();
    for change in changes {
        output.push_str(&format!("--- a/{}\n+++ b/{}\n", change.path, change.path));
        for line in String::from_utf8_lossy(&change.base).lines() {
            output.push('-');
            output.push_str(line);
            output.push('\n');
        }
        for line in String::from_utf8_lossy(&change.proposed).lines() {
            output.push('+');
            output.push_str(line);
            output.push('\n');
        }
    }
    output
}

fn changes_fingerprint(changes: &[FileChange]) -> String {
    let mut items = changes
        .iter()
        .map(|change| {
            json!([
                change.operation,
                change.path,
                content_fingerprint(&change.base),
                content_fingerprint(&change.proposed),
            ])
        })
        .collect::<Vec<_>>();
    items.sort_by_key(|item| item[1].as_str().unwrap_or_default().to_string());
    content_fingerprint(&serde_json::to_vec(&items).unwrap_or_default())
}

pub struct TerminalExecutor;

#[async_trait]
impl ToolExecutor for TerminalExecutor {
    async fn execute(
        &mut self,
        _call: &ToolCall,
        _spec: &ToolSpec,
        host: &ToolHost<'_, '_>,
    ) -> ToolExecution {
        let Some(resource) = host.facts.action.resources.first() else {
            return command_failure("change");
        };
        let conversation_id = host.facts.action.key.context.conversation_id;
        let action_id = host.facts.action.key.action_id;
        let root =
            match host
                .store
                .open_resource_root(conversation_id, &resource.root_id, action_id)
            {
                Ok(root) => root,
                Err(_) => return command_failure("change"),
            };
        let fact = match host
            .store
            .fact_by_key(conversation_id, &format!("terminal-change-{action_id}"))
        {
            Ok(fact) => fact,
            Err(_) => return command_failure("change"),
        };
        let ContextFact::CommandChange {
            status,
            exit_code,
            stdout,
            stderr,
            files,
        } = &fact.fact
        else {
            return command_failure("change");
        };
        let mut applied = Vec::new();
        let mut conflicts = Vec::new();
        for file in files {
            let relative = match RelativePath::parse(&file.path) {
                Ok(relative) => relative,
                Err(_) => {
                    conflicts.push(file.path.clone());
                    continue;
                }
            };
            match apply_file(&root, &relative, file) {
                Ok(()) => applied.push(file.path.clone()),
                Err(ApplyError::Conflict) => conflicts.push(file.path.clone()),
                Err(ApplyError::Unavailable) => conflicts.push(file.path.clone()),
            }
        }
        if !conflicts.is_empty() {
            return ToolExecution::Failed {
                summary: json!({
                    "status": "failed",
                    "code": "conflict",
                    "applied": applied,
                    "conflicted": conflicts,
                }),
            };
        }
        let exit = exit_code.map(|code| json!(code)).unwrap_or(Value::Null);
        let cwd = terminal::session_statuses()
            .into_iter()
            .find(|session| session.root_id == resource.root_id)
            .map(|session| session.cwd);
        ToolExecution::Succeeded {
            summary: json!({
                "status": status,
                "exitCode": exit,
                "stdout": stdout,
                "stderr": stderr,
                "cwd": cwd,
                "appliedFiles": applied,
            }),
        }
    }
}

pub struct TerminalAuthorizer;

impl super::cycle::ToolAuthorizer for TerminalAuthorizer {
    fn facts(
        &self,
        store: &AgentStore<'_>,
        revision: &AuthorizationRevision,
        action: &ActionRecord,
        call: &ToolCall,
    ) -> Result<p::TrustedFacts, String> {
        let command = call
            .arguments
            .get("command")
            .and_then(Value::as_str)
            .ok_or_else(|| "terminal_run requires a command argument".to_string())?;
        if !valid_terminal_command(command) {
            return Err("terminal_run requires a valid command argument".to_string());
        }
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
        let key = format!("terminal-change-{}", action.id);
        let existing = store.fact_by_key(revision.key().conversation_id, &key).ok();
        let (change_id, effects) = match &existing {
            Some(fact) => {
                let ContextFact::CommandChange { files, .. } = &fact.fact else {
                    return Err("the staged terminal change was malformed".to_string());
                };
                let change_set = store
                    .fact_by_key(revision.key().conversation_id, &format!("change-{}", action.id))
                    .ok();
                let change_id = change_set.and_then(|fact| {
                    if let ContextFact::ChangeSet {
                        proposal_fingerprint,
                        ..
                    } = &fact.fact
                    {
                        Some(format!("change-{proposal_fingerprint}"))
                    } else {
                        None
                    }
                });
                (change_id, effects_of(files))
            }
            None => {
                if !terminal::terminal_sandbox_available() {
                    return Err("terminal isolation is unavailable on this platform".to_string());
                }
                let root = store
                    .open_resource_root(revision.key().conversation_id, &root_id, action.id)
                    .map_err(|error| error.to_string())?;
                let real = root.display_path().to_path_buf();
                let (run, changes) = terminal::run_session_command(
                    &root_id,
                    &real,
                    command,
                    TERMINAL_COMMAND_TIMEOUT,
                )?;
                let files = changes.iter().map(command_file_change).collect::<Vec<_>>();
                let effects = effects_of(&files);
                let change_id = record_terminal_change(
                    store,
                    revision,
                    action,
                    &root_id,
                    &run,
                    files,
                    &changes,
                )?;
                (change_id, effects)
            }
        };
        let working_directory = p::Resource {
            root_id: root_id.clone(),
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
                executable: "/bin/sh".to_string(),
                argv: vec!["/bin/sh".to_string(), "-c".to_string(), command.to_string()],
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
            capability: p::Capability::Proven {
                effects,
                opened_resources_validated: true,
                process_isolation: true,
                network: p::NetworkControl::Disabled,
            },
            resources: vec![p::ResolvedResource {
                resource: working_directory,
                scope,
            }],
            concrete_change_id: change_id,
            recovery: p::Recovery::NotGuaranteed,
            approvals: Vec::new(),
            denied_actions: Vec::new(),
            grants: Vec::new(),
        })
    }
}

fn valid_terminal_command(command: &str) -> bool {
    !command.is_empty() && command.len() <= 8192 && !command.chars().any(char::is_control)
}

fn record_terminal_change(
    store: &AgentStore<'_>,
    revision: &AuthorizationRevision,
    action: &ActionRecord,
    root_id: &str,
    run: &command_execution::CommandRun,
    files: Vec<CommandFileChange>,
    changes: &[FileChange],
) -> Result<Option<String>, String> {
    let key = format!("terminal-change-{}", action.id);
    let additions = changes
        .iter()
        .filter(|change| change.operation != "delete")
        .count() as u32;
    let deletions = changes
        .iter()
        .filter(|change| change.operation == "delete")
        .count() as u32;
    let diff = combined_diff(changes);
    let change_id = if changes.is_empty() {
        None
    } else {
        Some(format!("change-{}", changes_fingerprint(changes)))
    };
    let command_change = ContextFact::CommandChange {
        status: run_status(run),
        exit_code: run.exit_code,
        stdout: run.stdout.clone(),
        stderr: run.stderr.clone(),
        files,
    };
    if changes.is_empty() {
        store
            .record_fact(
                revision.key().conversation_id,
                revision.key().context_revision,
                Some(action.run_id),
                &key,
                &command_change,
            )
            .map_err(|_| "the terminal change could not be recorded".to_string())?;
    } else {
        let change_set = ContextFact::ChangeSet {
            operation: "command".into(),
            path: ".".into(),
            base_fingerprint: None,
            proposed_fingerprint: None,
            proposal_fingerprint: changes_fingerprint(changes),
            additions,
            deletions,
            diff,
            diff_truncated: false,
            resource_keys: vec![root_id.to_string()],
        };
        store
            .record_fact(
                revision.key().conversation_id,
                revision.key().context_revision,
                Some(action.run_id),
                &format!("change-{}", action.id),
                &change_set,
            )
            .map_err(|_| "the terminal change set could not be recorded".to_string())?;
        store
            .record_fact(
                revision.key().conversation_id,
                revision.key().context_revision,
                Some(action.run_id),
                &key,
                &command_change,
            )
            .map_err(|_| "the terminal change could not be recorded".to_string())?;
    }
    Ok(change_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foundry::agent_runtime::{ToolCall, ToolRegistry};

    #[test]
    fn register_exposes_run_command_with_argv_schema() {
        let mut registry = ToolRegistry::default();
        register(&mut registry).expect("register");
        let tool = registry.get("run_command").expect("run_command");
        assert_eq!(tool.spec.name, "run_command");
        assert_eq!(tool.spec.parameters["required"][0], "argv");
    }

    #[test]
    fn command_argv_validation_rejects_empty_or_nul_arguments() {
        let empty = ToolCall {
            id: "call-1".into(),
            name: "run_command".into(),
            arguments: json!({ "argv": [] }),
        };
        assert!(command_argv(&empty).is_empty());
        let with_nul = ToolCall {
            id: "call-2".into(),
            name: "run_command".into(),
            arguments: json!({ "argv": ["/bin/echo", "a\0b"] }),
        };
        assert!(command_argv(&with_nul).iter().any(|arg| arg.contains('\0')));
    }

    #[test]
    fn effects_are_classified_from_file_changes() {
        let empty: Vec<CommandFileChange> = vec![];
        assert_eq!(effects_of(&empty), p::Effects::ReadOnly);
        let edit = vec![CommandFileChange {
            operation: "edit".into(),
            path: "a.txt".into(),
            base_fingerprint: "a".repeat(64),
            proposed_fingerprint: "b".repeat(64),
            content: "b".into(),
        }];
        assert_eq!(effects_of(&edit), p::Effects::Alteration);
        let delete = vec![CommandFileChange {
            operation: "delete".into(),
            path: "a.txt".into(),
            base_fingerprint: "a".repeat(64),
            proposed_fingerprint: "b".repeat(64),
            content: String::new(),
        }];
        assert_eq!(effects_of(&delete), p::Effects::Destructive);
    }

    #[test]
    fn combined_diff_and_fingerprint_are_deterministic() {
        let changes = vec![
            FileChange {
                operation: "edit",
                path: "a.txt".to_string(),
                base: b"before".to_vec(),
                proposed: b"after".to_vec(),
            },
            FileChange {
                operation: "create",
                path: "b.txt".to_string(),
                base: Vec::new(),
                proposed: b"new".to_vec(),
            },
        ];
        let diff = combined_diff(&changes);
        assert!(diff.contains("--- a/a.txt"));
        assert!(diff.contains("+after"));
        assert!(diff.contains("+new"));
        assert_eq!(changes_fingerprint(&changes), changes_fingerprint(&changes));
        assert_ne!(changes_fingerprint(&changes), changes_fingerprint(&[]));
    }

    #[test]
    fn register_exposes_terminal_run_with_command_schema() {
        let mut registry = ToolRegistry::default();
        register(&mut registry).expect("register");
        let tool = registry.get("terminal_run").expect("terminal_run");
        assert_eq!(tool.spec.name, "terminal_run");
        assert_eq!(tool.spec.parameters["required"][0], "command");
    }

    #[test]
    fn valid_terminal_command_rejects_empty_and_control_characters() {
        assert!(valid_terminal_command("echo hello"));
        assert!(!valid_terminal_command(""));
        assert!(!valid_terminal_command("echo a\0b"));
        assert!(!valid_terminal_command("echo a\nb"));
        assert!(!valid_terminal_command(&"x".repeat(8193)));
    }

    #[test]
    fn a_terminal_run_command_is_authorized_through_the_policy() {
        if !terminal::terminal_sandbox_available() {
            return;
        }
        use crate::foundry::agent_runtime::cycle::ToolAuthorizer;
        use crate::foundry::db::Database;
        use crate::foundry::resources::RootHandle;

        let database = Database::open_in_memory().expect("database");
        let directory = tempfile::tempdir().expect("directory");
        let root = RootHandle::enroll(directory.path()).expect("root");
        let store = database.agent_store();
        let project = store
            .create_project(&root, Some("TerminalTools"))
            .expect("project");
        let conversation = store
            .create_project_conversation(Some(project.id), "fixture-model")
            .expect("conversation")
            .conversation
            .id;
        let run = store.start_run(conversation, "terminal-tools").expect("run");
        let call = ToolCall {
            id: "call-a".into(),
            name: "terminal_run".into(),
            arguments: json!({ "command": "printf 'hello'" }),
        };
        let record = store
            .propose_call(run.id, &call.id, &call.name, &call.arguments)
            .expect("proposal");
        let revision = store
            .authorization_revision(conversation)
            .expect("revision");
        let trusted = TerminalAuthorizer
            .facts(&store, &revision, &record, &call)
            .expect("facts");

        match p::evaluate(&trusted.action, &trusted) {
            p::Decision::Ask(prompts) => assert!(
                prompts
                    .iter()
                    .any(|prompt| matches!(prompt.requirement, p::Requirement::Operation)),
                "expected an operation prompt, got {prompts:?}"
            ),
            p::Decision::Allow => panic!("expected Ask, got Allow"),
            p::Decision::Deny(denial) => panic!("expected Ask, got Deny {denial:?}"),
        }
    }

    #[test]
    fn the_terminal_executor_applies_a_detected_file_change() {
        if !terminal::terminal_sandbox_available() {
            return;
        }
        use crate::foundry::agent_runtime::cycle::ToolAuthorizer;
        use crate::foundry::db::Database;
        use crate::foundry::resources::RootHandle;

        let database = Database::open_in_memory().expect("database");
        let directory = tempfile::tempdir().expect("directory");
        let root = RootHandle::enroll(directory.path()).expect("root");
        let store = database.agent_store();
        let project = store
            .create_project(&root, Some("TerminalApply"))
            .expect("project");
        let conversation = store
            .create_project_conversation(Some(project.id), "fixture-model")
            .expect("conversation")
            .conversation
            .id;
        let run = store.start_run(conversation, "terminal-apply").expect("run");
        let call = ToolCall {
            id: "call-a".into(),
            name: "terminal_run".into(),
            arguments: json!({ "command": "printf 'hello' > created.txt" }),
        };
        let record = store
            .propose_call(run.id, &call.id, &call.name, &call.arguments)
            .expect("proposal");
        let revision = store
            .authorization_revision(conversation)
            .expect("revision");
        let trusted = TerminalAuthorizer
            .facts(&store, &revision, &record, &call)
            .expect("facts");

        let mut executor = TerminalExecutor;
        let host = ToolHost {
            store: &store,
            facts: &trusted,
        };
        let execution = tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(executor.execute(
                &call,
                &ToolSpec {
                    name: "terminal_run".into(),
                    description: String::new(),
                    parameters: json!({}),
                },
                &host,
            ));

        match execution {
            ToolExecution::Succeeded { summary } => {
                assert_eq!(summary["appliedFiles"][0], "created.txt");
            }
            other => panic!("expected success, got {other:?}"),
        }
        assert_eq!(
            std::fs::read(directory.path().join("created.txt")).expect("read"),
            b"hello"
        );
    }
}
