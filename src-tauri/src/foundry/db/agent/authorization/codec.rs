use super::*;

fn resource(value: &p::Resource) -> Value {
    json!({"root":value.root_id,"object":value.object_id,"version":value.version})
}

fn resources(values: &[p::Resource]) -> Value {
    Value::Array(values.iter().map(resource).collect())
}

fn operation(value: &p::Operation) -> Value {
    match value {
        p::Operation::File(op) => json!([
            "file",
            match op {
                p::FileOperation::Read => "read",
                p::FileOperation::Create => "create",
                p::FileOperation::Edit => "edit",
                p::FileOperation::Delete => "delete",
            }
        ]),
        p::Operation::Command {
            executable,
            argv,
            working_directory,
            environment,
        } => json!([
            "command",
            executable,
            argv,
            resource(working_directory),
            environment
        ]),
        p::Operation::Network {
            method,
            destination,
        } => json!(["network", method, destination]),
        p::Operation::RemoteInference => json!(["remote_inference"]),
        p::Operation::Unknown => json!(["unknown"]),
    }
}

fn recovery(value: &p::Recovery) -> Value {
    match value {
        p::Recovery::NotGuaranteed => json!(["not_guaranteed"]),
        p::Recovery::Protected { protection_id } => json!(["protected", protection_id]),
    }
}

fn requirement(value: &p::Requirement) -> Value {
    match value {
        p::Requirement::Operation => json!(["operation"]),
        p::Requirement::StandaloneRoot { root_id } => json!(["standalone_root", root_id]),
        p::Requirement::ExternalResource { resource: value } => {
            json!(["external_resource", resource(value)])
        }
        p::Requirement::DestructiveConfirmation { recovery: value } => {
            json!(["destructive_confirmation", recovery(value)])
        }
        p::Requirement::AlterationReview { change_id } => json!(["alteration_review", change_id]),
        p::Requirement::NetworkDestination { destination } => {
            json!(["network_destination", destination])
        }
    }
}

pub(super) fn requirement_fingerprint(value: &p::Requirement) -> AgentStoreResult<String> {
    fingerprint(&requirement(value))
}

pub(super) fn grant_fingerprint(
    action: &p::Action,
    value: &p::Requirement,
) -> AgentStoreResult<String> {
    fingerprint(&match value {
        p::Requirement::Operation => json!([
            "operation",
            operation(&action.operation),
            action.arguments_fingerprint,
            resources(&action.resources)
        ]),
        _ => requirement(value),
    })
}

pub(super) fn binding_fingerprint(
    facts: &p::TrustedFacts,
    project: &str,
) -> AgentStoreResult<String> {
    let key = &facts.action.key;
    let context = &key.context;
    let reads = match &facts.read_evidence {
        p::ReadEvidence::ProvenNoReads => json!(["none"]),
        p::ReadEvidence::Unknown => json!(["unknown"]),
        p::ReadEvidence::ProvenReads { resources: values } => json!(["reads", resources(values)]),
    };
    let capability = match &facts.capability {
        p::Capability::MissingImplementation => json!(["missing"]),
        p::Capability::Unproven => json!(["unproven"]),
        p::Capability::Rejected => json!(["rejected"]),
        p::Capability::Proven {
            effects,
            opened_resources_validated,
            process_isolation,
            network,
        } => {
            let effects = match effects {
                p::Effects::ReadOnly => "read_only",
                p::Effects::Alteration => "alteration",
                p::Effects::Destructive => "destructive",
                p::Effects::Unbounded => "unbounded",
                p::Effects::Unknown => "unknown",
            };
            let network = match network {
                p::NetworkControl::Disabled => json!(["disabled"]),
                p::NetworkControl::Opaque => json!(["opaque"]),
                p::NetworkControl::Unknown => json!(["unknown"]),
                p::NetworkControl::Controlled {
                    destinations,
                    audit_ready,
                } => json!(["controlled", destinations, audit_ready]),
            };
            json!([
                "proven",
                effects,
                opened_resources_validated,
                process_isolation,
                network
            ])
        }
    };
    let origin = match facts.action.origin {
        p::InstructionOrigin::User => "user",
        p::InstructionOrigin::Model => "model",
        p::InstructionOrigin::ProjectFile => "project_file",
        p::InstructionOrigin::ApprovedInstructionFile => "approved_instruction_file",
        p::InstructionOrigin::ToolOutput => "tool_output",
        p::InstructionOrigin::AttachedConversation => "attached_conversation",
    };
    fingerprint(
        &json!({"format":1,"key":[context.user_id,context.conversation_id,context.context_revision,context.authorization_revision,context.mode_revision,key.run_id,key.action_id],"operation":operation(&facts.action.operation),"arguments":facts.action.arguments_fingerprint,"resources":resources(&facts.action.resources),"origin":origin,"mode":mode_name(facts.mode),"proposal_mode":mode_name(facts.mode_at_proposal),"project":project,"read_allowed":facts.global_read_allowed,"reads":reads,"capability":capability,"change":facts.concrete_change_id,"recovery":recovery(&facts.recovery)}),
    )
}

pub(super) fn grant_kind(action: &p::Action, requirement: &p::Requirement) -> Option<p::GrantKind> {
    match requirement {
        p::Requirement::Operation => Some(p::GrantKind::Operation {
            operation: action.operation.clone(),
            arguments_fingerprint: action.arguments_fingerprint.clone(),
            resources: action.resources.clone(),
        }),
        p::Requirement::StandaloneRoot { root_id } => Some(p::GrantKind::StandaloneRoot {
            root_id: root_id.clone(),
        }),
        p::Requirement::NetworkDestination { destination } => {
            Some(p::GrantKind::NetworkDestination {
                destination: destination.clone(),
            })
        }
        _ => None,
    }
}

pub(super) fn requirement_display(
    connection: &Connection,
    current: &AuthorizationRevision,
    prompt: &p::Prompt,
    hash: &str,
    facts: Option<&p::TrustedFacts>,
) -> AgentStoreResult<RequirementDisplay> {
    let operation_label = facts
        .map(|facts| operation_preview(&facts.action.operation))
        .transpose()?
        .unwrap_or_else(|| "Preview unavailable: no backend operation supplied".into());
    let targets = facts
        .map(|facts| target_preview(&facts.action.resources))
        .transpose()?
        .unwrap_or_default();
    let mut preview_available = false;
    let (kind, target, protected) = match &prompt.requirement {
        p::Requirement::Operation => (
            RequirementKind::Operation,
            format!("{operation_label}; {targets}"),
            false,
        ),
        p::Requirement::StandaloneRoot { root_id } => {
            preview_available = true;
            (
                RequirementKind::StandaloneRoot,
                root_path(connection, current, root_id, 0)?,
                false,
            )
        }
        p::Requirement::ExternalResource { resource } => {
            let facts = facts.ok_or(AgentStoreError::InvalidInput)?;
            let path = root_path(
                connection,
                current,
                &resource.root_id,
                facts.action.key.action_id,
            )?;
            (
                RequirementKind::ExternalResource,
                format!(
                    "{operation_label}; external root {path}; object {}; version sha256:{}; this action only",
                    identity_label(&resource.object_id)?,
                    fingerprint(&json!(resource.version))?
                ),
                false,
            )
        }
        p::Requirement::DestructiveConfirmation { recovery } => {
            let recovery_label = match recovery {
                p::Recovery::NotGuaranteed => "Recovery not guaranteed".into(),
                p::Recovery::Protected { protection_id } => {
                    format!("Recovery protected: {}", identity_label(protection_id)?)
                }
            };
            let change = facts
                .and_then(|facts| facts.concrete_change_id.as_deref())
                .map(identity_label)
                .transpose()?
                .unwrap_or_else(|| "none".into());
            (
                RequirementKind::DestructiveConfirmation,
                format!(
                    "Destructive: {operation_label}; {targets}; change {change}; {recovery_label}; never remembered"
                ),
                matches!(recovery, p::Recovery::Protected { .. }),
            )
        }
        p::Requirement::AlterationReview { change_id } => {
            let recovery_label = match facts.map(|facts| &facts.recovery) {
                Some(p::Recovery::Protected { protection_id }) => {
                    format!("Recovery protected: {}", identity_label(protection_id)?)
                }
                _ => "Recovery not guaranteed".into(),
            };
            (
                RequirementKind::AlterationReview,
                format!(
                    "Review change {}: {operation_label}; {targets}; {recovery_label}",
                    identity_label(change_id)?
                ),
                matches!(
                    facts.map(|facts| &facts.recovery),
                    Some(p::Recovery::Protected { .. })
                ),
            )
        }
        p::Requirement::NetworkDestination { destination } => {
            let mut url =
                reqwest::Url::parse(destination).map_err(|_| AgentStoreError::InvalidInput)?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                return Err(AgentStoreError::InvalidInput);
            }
            let destination_visible = url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none()
                && url.path() == "/";
            url.set_username("")
                .map_err(|_| AgentStoreError::InvalidInput)?;
            url.set_password(None)
                .map_err(|_| AgentStoreError::InvalidInput)?;
            if !destination_visible {
                url.set_path("/[redacted]");
            }
            url.set_query(None);
            url.set_fragment(None);
            let target = format!(
                "Preview unavailable: bound HTTP method, body and source details not presented; destination {url}"
            );
            (RequirementKind::NetworkDestination, target, false)
        }
    };
    let target = if !preview_available && !target.starts_with("Preview unavailable") {
        format!(
            "Preview unavailable: full operation, native targets or concrete diff not presented; {target}"
        )
    } else {
        target
    };
    if target.len() > 4096 || target.chars().any(char::is_control) {
        return Err(AgentStoreError::InvalidInput);
    }
    let root_identity_key = match &prompt.requirement {
        p::Requirement::StandaloneRoot { root_id } => Some(root_id.clone()),
        p::Requirement::ExternalResource { resource } => Some(resource.root_id.clone()),
        _ => None,
    };
    Ok(RequirementDisplay {
        kind,
        target,
        root_identity_key,
        constraints_fingerprint: hash.into(),
        remember_eligible: prompt.remember == p::Remember::Conversation,
        recovery_guaranteed: protected,
        preview_available,
    })
}

fn identity_label(value: &str) -> AgentStoreResult<String> {
    if value.chars().any(char::is_control) {
        return Err(AgentStoreError::InvalidInput);
    }
    if value.len() <= 128
        && [
            "darwin:",
            "file-",
            "object-",
            "change-",
            "diff-",
            "protection-",
            "recovery-",
        ]
        .iter()
        .any(|prefix| value.starts_with(prefix))
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:-".contains(&byte))
    {
        Ok(value.into())
    } else {
        Ok(format!("sha256:{}", fingerprint(&json!(value))?))
    }
}

fn target_preview(values: &[p::Resource]) -> AgentStoreResult<String> {
    let labels: AgentStoreResult<Vec<_>> = values
        .iter()
        .take(8)
        .map(|value| {
            Ok(format!(
                "{}@sha256:{}",
                identity_label(&value.object_id)?,
                fingerprint(&json!(value.version))?
            ))
        })
        .collect();
    Ok(format!(
        "targets [{}]; count={}; omitted={}; resources sha256:{}",
        labels?.join(", "),
        values.len(),
        values.len().saturating_sub(8),
        fingerprint(&resources(values))?
    ))
}

fn operation_preview(value: &p::Operation) -> AgentStoreResult<String> {
    match value {
        p::Operation::Command {
            executable,
            argv,
            environment,
            ..
        } => {
            if executable.chars().any(char::is_control)
                || argv.iter().any(|arg| arg.chars().any(char::is_control))
            {
                return Err(AgentStoreError::InvalidInput);
            }
            let program = std::path::Path::new(executable)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("");
            let known_program = matches!(program, "git" | "cargo");
            let mut arguments_visible = known_program && !argv.is_empty();
            let mut labels = vec![if known_program {
                program.into()
            } else {
                "program".into()
            }];
            for (index, arg) in argv.iter().enumerate() {
                let safe = if index == 0 {
                    matches!(
                        (program, arg.as_str()),
                        ("git", "status" | "diff" | "log" | "show" | "rev-parse")
                            | ("cargo", "test" | "check" | "build" | "fmt" | "clippy")
                    )
                } else {
                    known_program
                        && matches!(
                            arg.as_str(),
                            "--porcelain"
                                | "--short"
                                | "--branch"
                                | "--all"
                                | "--offline"
                                | "--locked"
                                | "--quiet"
                                | "--verbose"
                                | "--no-default-features"
                                | "--nocapture"
                        )
                };
                arguments_visible &= safe;
                labels.push(if safe {
                    arg.clone()
                } else {
                    "[redacted]".into()
                });
            }
            let preview = format!(
                "{}; argc={}; environment={} values redacted; program sha256:{}",
                labels.join(" "),
                argv.len(),
                environment.len(),
                fingerprint(&json!(executable))?
            );
            Ok(if arguments_visible {
                preview
            } else {
                format!("Preview unavailable: masked or unsupported arguments; {preview}")
            })
        }
        p::Operation::File(operation) => Ok(format!(
            "file {}",
            match operation {
                p::FileOperation::Read => "read",
                p::FileOperation::Create => "create",
                p::FileOperation::Edit => "edit",
                p::FileOperation::Delete => "delete",
            }
        )),
        p::Operation::Network { method, .. } => Ok(format!(
            "network {}",
            if matches!(
                method.as_str(),
                "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS"
            ) {
                method.as_str()
            } else {
                "[redacted method]"
            }
        )),
        _ => Ok("Preview unavailable: unsupported operation".into()),
    }
}

pub(super) fn encode_display(display: &RequirementDisplay) -> AgentStoreResult<String> {
    serde_json::to_string(display).map_err(|_| AgentStoreError::InvalidInput)
}
pub(super) fn decode_display(display: &str) -> AgentStoreResult<RequirementDisplay> {
    let mut display: RequirementDisplay =
        serde_json::from_str(display).map_err(|_| AgentStoreError::Storage)?;
    if display.target.len() > 4096 || display.target.chars().any(char::is_control) {
        return Err(AgentStoreError::Storage);
    }
    display.preview_available &= display.kind == RequirementKind::StandaloneRoot;
    Ok(display)
}
