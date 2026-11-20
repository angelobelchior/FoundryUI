mod types;

pub use crate::foundry::db::agent::AgentMode;
pub use types::*;

pub fn evaluate(action: &Action, facts: &TrustedFacts) -> Decision {
    let categories = CategoryPermissions {
        read_auto: facts.global_read_allowed,
        edit_auto: facts.mode == AgentMode::Automatic
            && facts.mode_at_proposal == AgentMode::Automatic,
        command_auto: false,
        network_auto: false,
    };
    evaluate_with(action, facts, categories)
}

pub fn evaluate_with(
    action: &Action,
    facts: &TrustedFacts,
    categories: CategoryPermissions,
) -> Decision {
    if action.key != facts.current_key {
        return Decision::Deny(Denial::StaleContext);
    }
    if action != &facts.action {
        return Decision::Deny(Denial::ActionFactsMismatch);
    }
    if !valid_action(action) {
        return Decision::Deny(Denial::InvalidAction);
    }
    if facts.denied_actions.contains(action) {
        return Decision::Deny(Denial::UserDenied);
    }
    if matches!(action.operation, Operation::RemoteInference) {
        return Decision::Deny(Denial::RemoteInferenceUnavailable);
    }
    let (effects, opened_resources_validated, process_isolation, network) = match &facts.capability
    {
        Capability::MissingImplementation => {
            return Decision::Deny(Denial::ProductionImplementationMissing);
        }
        Capability::Unproven => return Decision::Deny(Denial::CapabilityUnproven),
        Capability::Rejected => return Decision::Deny(Denial::CapabilityRejected),
        Capability::Proven {
            effects,
            opened_resources_validated,
            process_isolation,
            network,
        } => (
            effects,
            *opened_resources_validated,
            *process_isolation,
            network,
        ),
    };
    if matches!(effects, Effects::Unknown | Effects::Unbounded) {
        return Decision::Deny(Denial::UnboundedEffects);
    }
    if !opened_resources_validated
        || facts.resources.len() != action.resources.len()
        || action.resources.iter().any(|resource| {
            !facts
                .resources
                .iter()
                .any(|resolved| resolved.resource == *resource)
        })
    {
        return Decision::Deny(Denial::ResourceValidationMissing);
    }
    for resolved in &facts.resources {
        let valid = match (&facts.project_root_id, &resolved.scope) {
            (Some(root), ResourceScope::Project) => root == &resolved.resource.root_id,
            (Some(root), ResourceScope::ExternalProject) => root != &resolved.resource.root_id,
            (None, ResourceScope::Standalone) => true,
            _ => false,
        };
        if !valid {
            return Decision::Deny(Denial::ResourceScopeMismatch);
        }
    }
    let reads_resources = match &facts.read_evidence {
        ReadEvidence::ProvenNoReads
            if !matches!(action.operation, Operation::File(FileOperation::Read)) =>
        {
            false
        }
        ReadEvidence::ProvenReads { resources }
            if !resources.is_empty()
                && resources.iter().enumerate().all(|(index, resource)| {
                    action.resources.contains(resource) && !resources[..index].contains(resource)
                }) =>
        {
            true
        }
        _ => return Decision::Deny(Denial::ReadEvidenceUnproven),
    };
    if reads_resources && (!facts.global_read_allowed || !categories.read_auto) {
        return Decision::Deny(Denial::ReadRestricted);
    }
    if matches!(action.operation, Operation::Command { .. }) && !process_isolation {
        return Decision::Deny(Denial::ProcessIsolationMissing);
    }
    let destinations = match network {
        NetworkControl::Disabled => &[][..],
        NetworkControl::Controlled {
            destinations,
            audit_ready: true,
        } if !destinations.is_empty()
            && destinations
                .iter()
                .all(|destination| !destination.is_empty()) =>
        {
            destinations.as_slice()
        }
        _ => return Decision::Deny(Denial::NetworkNotAuditable),
    };
    if let Operation::Network { destination, .. } = &action.operation
        && !destinations.contains(destination)
    {
        return Decision::Deny(Denial::NetworkScopeMismatch);
    }
    let destructive = matches!(effects, Effects::Destructive)
        || matches!(action.operation, Operation::File(FileOperation::Delete));
    let alteration = destructive
        || matches!(effects, Effects::Alteration)
        || matches!(
            action.operation,
            Operation::File(FileOperation::Create | FileOperation::Edit)
        );
    let review = if alteration && !categories.edit_auto {
        match &facts.concrete_change_id {
            Some(change_id) if !change_id.is_empty() => Some(change_id.clone()),
            _ => return Decision::Deny(Denial::ConcreteReviewMissing),
        }
    } else {
        None
    };
    let mut prompts = Vec::new();
    if !categories.command_auto && matches!(action.operation, Operation::Command { .. }) {
        require(
            action,
            facts,
            &mut prompts,
            Requirement::Operation,
            Remember::Conversation,
        );
    }
    for resource in &facts.resources {
        match resource.scope {
            ResourceScope::Project => {}
            ResourceScope::ExternalProject => require(
                action,
                facts,
                &mut prompts,
                Requirement::ExternalResource {
                    resource: resource.resource.clone(),
                },
                Remember::Never,
            ),
            ResourceScope::Standalone => require(
                action,
                facts,
                &mut prompts,
                Requirement::StandaloneRoot {
                    root_id: resource.resource.root_id.clone(),
                },
                Remember::Conversation,
            ),
        }
    }
    if destructive {
        require(
            action,
            facts,
            &mut prompts,
            Requirement::DestructiveConfirmation {
                recovery: facts.recovery.clone(),
            },
            Remember::Never,
        );
    }
    if let Some(change_id) = review {
        require(
            action,
            facts,
            &mut prompts,
            Requirement::AlterationReview { change_id },
            Remember::Never,
        );
    }
    for destination in destinations {
        if categories.network_auto {
            break;
        }
        require(
            action,
            facts,
            &mut prompts,
            Requirement::NetworkDestination {
                destination: destination.clone(),
            },
            Remember::Conversation,
        );
    }
    if prompts.is_empty() {
        Decision::Allow
    } else {
        Decision::Ask(prompts)
    }
}

fn valid_action(action: &Action) -> bool {
    let context = &action.key.context;
    if context.user_id.is_empty()
        || action.arguments_fingerprint.is_empty()
        || context.conversation_id <= 0
        || context.context_revision <= 0
        || context.authorization_revision < 0
        || context.mode_revision < 0
        || action.key.run_id <= 0
        || action.key.action_id <= 0
    {
        return false;
    }
    if action
        .resources
        .iter()
        .enumerate()
        .any(|(index, resource)| {
            resource.root_id.is_empty()
                || resource.object_id.is_empty()
                || resource.version.is_empty()
                || action.resources[..index].contains(resource)
        })
    {
        return false;
    }
    match &action.operation {
        Operation::File(_) => !action.resources.is_empty(),
        Operation::Command {
            executable,
            argv,
            working_directory,
            environment,
        } => {
            executable.starts_with('/')
                && !executable.contains('\0')
                && argv.iter().all(|arg| !arg.contains('\0'))
                && environment.iter().all(|(key, value)| {
                    !key.is_empty() && !key.contains(['=', '\0']) && !value.contains('\0')
                })
                && action.resources.contains(working_directory)
        }
        Operation::Network {
            method,
            destination,
        } => !method.is_empty() && !destination.is_empty(),
        Operation::RemoteInference => true,
        Operation::Unknown => false,
    }
}

fn require(
    action: &Action,
    facts: &TrustedFacts,
    prompts: &mut Vec<Prompt>,
    requirement: Requirement,
    remember: Remember,
) {
    let approved = facts
        .approvals
        .iter()
        .any(|approval| approval.action == *action && approval.requirement == requirement);
    let granted = remember == Remember::Conversation
        && facts.grants.iter().any(|grant| {
            grant.context == action.key.context
                && match (&grant.kind, &requirement) {
                    (
                        GrantKind::Operation {
                            operation,
                            resources,
                            arguments_fingerprint,
                        },
                        Requirement::Operation,
                    ) => {
                        operation == &action.operation
                            && resources == &action.resources
                            && arguments_fingerprint == &action.arguments_fingerprint
                    }
                    (
                        GrantKind::NetworkDestination {
                            destination: granted,
                        },
                        Requirement::NetworkDestination { destination },
                    ) => granted == destination,
                    (
                        GrantKind::StandaloneRoot { root_id: granted },
                        Requirement::StandaloneRoot { root_id },
                    ) => granted == root_id,
                    _ => false,
                }
        });
    if !approved
        && !granted
        && !prompts
            .iter()
            .any(|prompt| prompt.requirement == requirement)
    {
        prompts.push(Prompt {
            requirement,
            remember,
        });
    }
}

#[cfg(test)]
mod tests;
