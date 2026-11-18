use super::*;

fn fixture() -> (Action, TrustedFacts) {
    let resource = Resource {
        root_id: "project-root".into(),
        object_id: "file-identity".into(),
        version: "version-1".into(),
    };
    let action = Action {
        key: ActionKey {
            context: ContextKey {
                user_id: "user-1".into(),
                conversation_id: 1,
                context_revision: 1,
                authorization_revision: 1,
                mode_revision: 1,
            },
            run_id: 1,
            action_id: 1,
        },
        operation: Operation::File(FileOperation::Read),
        arguments_fingerprint: "arguments-1".into(),
        resources: vec![resource.clone()],
        origin: InstructionOrigin::Model,
    };
    let facts = TrustedFacts {
        action: action.clone(),
        current_key: action.key.clone(),
        mode: AgentMode::Review,
        mode_at_proposal: AgentMode::Review,
        project_root_id: Some(resource.root_id.clone()),
        global_read_allowed: true,
        read_evidence: ReadEvidence::ProvenReads {
            resources: vec![resource.clone()],
        },
        capability: Capability::Proven {
            effects: Effects::ReadOnly,
            opened_resources_validated: true,
            process_isolation: false,
            network: NetworkControl::Disabled,
        },
        resources: vec![ResolvedResource {
            resource,
            scope: ResourceScope::Project,
        }],
        concrete_change_id: None,
        recovery: Recovery::NotGuaranteed,
        approvals: vec![],
        denied_actions: vec![],
        grants: vec![],
    };
    (action, facts)
}

#[test]
fn unavailable_capability_blocks_even_an_in_project_read() {
    let (action, mut facts) = fixture();
    for (capability, denial) in [
        (
            Capability::MissingImplementation,
            Denial::ProductionImplementationMissing,
        ),
        (Capability::Unproven, Denial::CapabilityUnproven),
        (Capability::Rejected, Denial::CapabilityRejected),
    ] {
        facts.capability = capability;
        assert_eq!(evaluate(&action, &facts), Decision::Deny(denial));
    }
}

#[test]
fn only_current_exact_backend_bound_actions_can_use_positive_evidence() {
    let (action, facts) = fixture();
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
    for field in 0..8 {
        let mut stale = action.clone();
        match field {
            0 => stale.key.context.user_id = "other-user".into(),
            1 => stale.key.context.conversation_id += 1,
            2 => stale.key.context.context_revision += 1,
            3 => stale.key.context.authorization_revision += 1,
            4 => stale.key.context.mode_revision += 1,
            5 => stale.key.run_id += 1,
            6 => stale.key.action_id += 1,
            _ => stale.resources[0].version = "substituted".into(),
        }
        let expected = if field == 7 {
            Denial::ActionFactsMismatch
        } else {
            Denial::StaleContext
        };
        assert_eq!(evaluate(&stale, &facts), Decision::Deny(expected));
    }
}

fn ask(requirement: Requirement, remember: Remember) -> Decision {
    Decision::Ask(vec![Prompt {
        requirement,
        remember,
    }])
}

#[test]
fn external_access_is_action_bound_and_never_remembered() {
    let (action, mut facts) = fixture();
    facts.resources[0].scope = ResourceScope::ExternalProject;
    facts.project_root_id = Some("different-project".into());
    let requirement = Requirement::ExternalResource {
        resource: action.resources[0].clone(),
    };
    assert_eq!(
        evaluate(&action, &facts),
        ask(requirement.clone(), Remember::Never)
    );
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement: requirement.clone(),
    });
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
    let mut next = action.clone();
    next.key.action_id += 1;
    facts.action = next.clone();
    facts.current_key = next.key.clone();
    assert_eq!(evaluate(&next, &facts), ask(requirement, Remember::Never));
}

#[test]
fn standalone_roots_are_explicit_and_conversation_only() {
    let (action, mut facts) = fixture();
    facts.project_root_id = None;
    facts.resources[0].scope = ResourceScope::Standalone;
    let root_id = action.resources[0].root_id.clone();
    let expected = ask(
        Requirement::StandaloneRoot {
            root_id: root_id.clone(),
        },
        Remember::Conversation,
    );
    assert_eq!(evaluate(&action, &facts), expected);
    let grant = ConversationGrant {
        context: action.key.context.clone(),
        kind: GrantKind::StandaloneRoot { root_id },
    };
    facts.grants.push(grant.clone());
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
    for field in 0..6 {
        facts.grants[0] = grant.clone();
        match field {
            0 => facts.grants[0].context.user_id = "other".into(),
            1 => facts.grants[0].context.conversation_id += 1,
            2 => facts.grants[0].context.context_revision += 1,
            3 => facts.grants[0].context.authorization_revision += 1,
            4 => facts.grants[0].context.mode_revision += 1,
            _ => {
                facts.grants[0].kind = GrantKind::StandaloneRoot {
                    root_id: "different-root".into(),
                }
            }
        }
        assert_eq!(evaluate(&action, &facts), expected);
    }
    facts.grants.clear();
    assert_eq!(evaluate(&action, &facts), expected);
}

fn prove(facts: &mut TrustedFacts, effects: Effects, isolation: bool, network: NetworkControl) {
    facts.capability = Capability::Proven {
        effects,
        opened_resources_validated: true,
        process_isolation: isolation,
        network,
    };
}

#[test]
fn unknown_effects_and_missing_resource_guarantees_outrank_prompts() {
    let (action, mut facts) = fixture();
    for effects in [Effects::Unknown, Effects::Unbounded] {
        prove(&mut facts, effects, true, NetworkControl::Disabled);
        assert_eq!(
            evaluate(&action, &facts),
            Decision::Deny(Denial::UnboundedEffects)
        );
    }
    prove(
        &mut facts,
        Effects::ReadOnly,
        false,
        NetworkControl::Disabled,
    );
    facts.resources.clear();
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::ResourceValidationMissing)
    );
    let (_, mut facts) = fixture();
    facts.capability = Capability::Proven {
        effects: Effects::ReadOnly,
        opened_resources_validated: false,
        process_isolation: true,
        network: NetworkControl::Disabled,
    };
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::ResourceValidationMissing)
    );
    let (_, mut facts) = fixture();
    facts.resources[0].scope = ResourceScope::Standalone;
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::ResourceScopeMismatch)
    );
    let (_, mut facts) = fixture();
    facts.global_read_allowed = false;
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::ReadRestricted)
    );
}

fn command_fixture() -> (Action, TrustedFacts) {
    let (mut action, mut facts) = fixture();
    action.operation = Operation::Command {
        executable: "/usr/bin/git".into(),
        argv: vec!["status".into()],
        working_directory: action.resources[0].clone(),
        environment: vec![],
    };
    facts.action = action.clone();
    prove(
        &mut facts,
        Effects::ReadOnly,
        true,
        NetworkControl::Disabled,
    );
    (action, facts)
}

#[test]
fn command_grants_match_the_whole_structured_operation_not_a_prefix() {
    let (action, mut facts) = command_fixture();
    let expected = ask(Requirement::Operation, Remember::Conversation);
    assert_eq!(evaluate(&action, &facts), expected);
    facts.grants.push(ConversationGrant {
        context: action.key.context.clone(),
        kind: GrantKind::Operation {
            operation: action.operation.clone(),
            arguments_fingerprint: action.arguments_fingerprint.clone(),
            resources: action.resources.clone(),
        },
    });
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
    for field in 0..5 {
        let mut next = action.clone();
        if let Operation::Command {
            executable,
            argv,
            working_directory,
            environment,
        } = &mut next.operation
        {
            match field {
                0 => *argv = vec!["push".into()],
                1 => argv.push("; rm -rf .".into()),
                2 => *executable = "/bin/sh".into(),
                3 => working_directory.version = "different-cwd".into(),
                _ => environment.push(("GIT_CONFIG".into(), "other".into())),
            }
        }
        facts.action = next.clone();
        let result = evaluate(&next, &facts);
        assert!(
            matches!(result, Decision::Ask(_) | Decision::Deny(_)),
            "{result:?}"
        );
    }
}

#[test]
fn command_execution_requires_backend_proven_isolation() {
    let (action, mut facts) = command_fixture();
    prove(
        &mut facts,
        Effects::ReadOnly,
        false,
        NetworkControl::Disabled,
    );
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::ProcessIsolationMissing)
    );
}

#[test]
fn destructive_effects_always_need_a_fresh_action_confirmation_in_both_modes() {
    let (mut action, mut facts) = fixture();
    action.operation = Operation::File(FileOperation::Delete);
    facts.action = action.clone();
    facts.concrete_change_id = Some("delete-diff-1".into());
    for mode in [AgentMode::Review, AgentMode::Automatic] {
        facts.mode = mode;
        facts.mode_at_proposal = mode;
        facts.approvals = vec![ActionApproval {
            action: action.clone(),
            requirement: Requirement::AlterationReview {
                change_id: "delete-diff-1".into(),
            },
        }];
        prove(
            &mut facts,
            Effects::ReadOnly,
            false,
            NetworkControl::Disabled,
        );
        assert_eq!(
            evaluate(&action, &facts),
            ask(
                Requirement::DestructiveConfirmation {
                    recovery: Recovery::NotGuaranteed
                },
                Remember::Never
            )
        );
        facts.approvals.push(ActionApproval {
            action: action.clone(),
            requirement: Requirement::DestructiveConfirmation {
                recovery: Recovery::NotGuaranteed,
            },
        });
        assert_eq!(evaluate(&action, &facts), Decision::Allow);
        let mut next = action.clone();
        next.key.action_id += 1;
        facts.action = next.clone();
        facts.current_key = next.key.clone();
        assert!(
            matches!(evaluate(&next, &facts), Decision::Ask(prompts) if prompts.contains(&Prompt { requirement: Requirement::DestructiveConfirmation { recovery: Recovery::NotGuaranteed }, remember: Remember::Never }))
        );
        facts.action = action.clone();
        facts.current_key = action.key.clone();
    }
}

#[test]
fn review_mode_requires_concrete_changes_and_exact_review_approval() {
    let (mut action, mut facts) = fixture();
    action.operation = Operation::File(FileOperation::Edit);
    facts.action = action.clone();
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::ConcreteReviewMissing)
    );
    facts.concrete_change_id = Some("diff-1".into());
    let review = Requirement::AlterationReview {
        change_id: "diff-1".into(),
    };
    assert_eq!(
        evaluate(&action, &facts),
        ask(review.clone(), Remember::Never)
    );
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement: review,
    });
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
    facts.concrete_change_id = Some("diff-2".into());
    assert_eq!(
        evaluate(&action, &facts),
        ask(
            Requirement::AlterationReview {
                change_id: "diff-2".into()
            },
            Remember::Never
        )
    );
    facts.mode = AgentMode::Automatic;
    facts.mode_at_proposal = AgentMode::Automatic;
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
}

#[test]
fn backend_effects_override_an_innocent_command_name() {
    let (action, mut facts) = command_fixture();
    facts.mode = AgentMode::Automatic;
    facts.mode_at_proposal = AgentMode::Automatic;
    prove(
        &mut facts,
        Effects::Destructive,
        true,
        NetworkControl::Disabled,
    );
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement: Requirement::Operation,
    });
    assert_eq!(
        evaluate(&action, &facts),
        ask(
            Requirement::DestructiveConfirmation {
                recovery: Recovery::NotGuaranteed
            },
            Remember::Never
        )
    );
}

#[test]
fn network_requires_independent_control_and_each_destination_approval() {
    let (action, mut facts) = command_fixture();
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement: Requirement::Operation,
    });
    for network in [NetworkControl::Opaque, NetworkControl::Unknown] {
        prove(&mut facts, Effects::ReadOnly, true, network);
        assert_eq!(
            evaluate(&action, &facts),
            Decision::Deny(Denial::NetworkNotAuditable)
        );
    }
    let first = "https://example.test/a".to_string();
    let second = "https://redirect.test/b".to_string();
    prove(
        &mut facts,
        Effects::ReadOnly,
        true,
        NetworkControl::Controlled {
            destinations: vec![first.clone()],
            audit_ready: true,
        },
    );
    let permission = Requirement::NetworkDestination {
        destination: first.clone(),
    };
    assert_eq!(
        evaluate(&action, &facts),
        ask(permission.clone(), Remember::Conversation)
    );
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement: permission,
    });
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
    prove(
        &mut facts,
        Effects::ReadOnly,
        true,
        NetworkControl::Controlled {
            destinations: vec![first, second.clone()],
            audit_ready: true,
        },
    );
    assert_eq!(
        evaluate(&action, &facts),
        ask(
            Requirement::NetworkDestination {
                destination: second
            },
            Remember::Conversation
        )
    );
}

#[test]
fn a_network_request_cannot_claim_disabled_transport_or_another_destination() {
    let (mut action, mut facts) = fixture();
    action.operation = Operation::Network {
        method: "GET".into(),
        destination: "https://example.test/a".into(),
    };
    action.resources.clear();
    facts.action = action.clone();
    facts.resources.clear();
    facts.read_evidence = ReadEvidence::ProvenNoReads;
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::NetworkScopeMismatch)
    );
    prove(
        &mut facts,
        Effects::ReadOnly,
        false,
        NetworkControl::Controlled {
            destinations: vec!["https://other.test/".into()],
            audit_ready: true,
        },
    );
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::NetworkScopeMismatch)
    );
    prove(
        &mut facts,
        Effects::ReadOnly,
        false,
        NetworkControl::Controlled {
            destinations: vec!["https://example.test/a".into()],
            audit_ready: true,
        },
    );
    assert_eq!(
        evaluate(&action, &facts),
        ask(
            Requirement::NetworkDestination {
                destination: "https://example.test/a".into()
            },
            Remember::Conversation
        )
    );
}

#[test]
fn network_approval_never_enables_remote_inference() {
    let (mut action, mut facts) = fixture();
    action.operation = Operation::RemoteInference;
    facts.action = action.clone();
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::RemoteInferenceUnavailable)
    );
}

#[test]
fn malformed_or_unbounded_operations_cannot_be_approved_into_execution() {
    let (action, facts) = fixture();
    for field in 0..7 {
        let mut malformed = action.clone();
        let mut resolved = facts.clone();
        match field {
            0 => malformed.operation = Operation::Unknown,
            1 => malformed.key.context.user_id.clear(),
            2 => malformed.key.action_id = 0,
            3 => malformed.resources.clear(),
            4 => malformed.resources[0].object_id.clear(),
            5 => malformed.resources.push(malformed.resources[0].clone()),
            _ => {
                malformed.operation = Operation::Command {
                    executable: "git".into(),
                    argv: vec!["status".into()],
                    working_directory: Resource {
                        root_id: "missing".into(),
                        object_id: "missing".into(),
                        version: "1".into(),
                    },
                    environment: vec![],
                }
            }
        }
        resolved.action = malformed.clone();
        resolved.current_key = malformed.key.clone();
        assert_eq!(
            evaluate(&malformed, &resolved),
            Decision::Deny(Denial::InvalidAction)
        );
    }
}

#[test]
fn switching_to_automatic_does_not_approve_a_pending_review() {
    let (mut action, mut facts) = fixture();
    action.operation = Operation::File(FileOperation::Edit);
    facts.action = action.clone();
    facts.mode = AgentMode::Automatic;
    facts.concrete_change_id = Some("pending-diff".into());
    assert_eq!(
        evaluate(&action, &facts),
        ask(
            Requirement::AlterationReview {
                change_id: "pending-diff".into()
            },
            Remember::Never
        )
    );
}

#[test]
fn destructive_confirmation_is_also_bound_to_the_actual_recovery_guarantee() {
    let (action, mut facts) = command_fixture();
    facts.mode = AgentMode::Automatic;
    facts.mode_at_proposal = AgentMode::Automatic;
    prove(
        &mut facts,
        Effects::Destructive,
        true,
        NetworkControl::Disabled,
    );
    facts.recovery = Recovery::Protected {
        protection_id: "git-protection-1".into(),
    };
    let requirement = Requirement::DestructiveConfirmation {
        recovery: facts.recovery.clone(),
    };
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement: Requirement::Operation,
    });
    assert_eq!(
        evaluate(&action, &facts),
        ask(requirement.clone(), Remember::Never)
    );
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement,
    });
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
    facts.recovery = Recovery::NotGuaranteed;
    assert_eq!(
        evaluate(&action, &facts),
        ask(
            Requirement::DestructiveConfirmation {
                recovery: Recovery::NotGuaranteed
            },
            Remember::Never
        )
    );
}

#[test]
fn explicit_denial_outranks_other_approvals_and_missing_prompts() {
    let (action, mut facts) = command_fixture();
    facts.denied_actions.push(action.clone());
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::UserDenied)
    );
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement: Requirement::Operation,
    });
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::UserDenied)
    );
    facts.denied_actions[0].key.context.conversation_id += 1;
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
}

#[test]
fn failure_to_prepare_audit_blocks_even_controlled_approved_transport() {
    let (action, mut facts) = command_fixture();
    let destination = "https://example.test/a".to_string();
    prove(
        &mut facts,
        Effects::ReadOnly,
        true,
        NetworkControl::Controlled {
            destinations: vec![destination.clone()],
            audit_ready: false,
        },
    );
    facts.approvals = vec![
        ActionApproval {
            action: action.clone(),
            requirement: Requirement::Operation,
        },
        ActionApproval {
            action: action.clone(),
            requirement: Requirement::NetworkDestination { destination },
        },
    ];
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::NetworkNotAuditable)
    );
}

#[test]
fn instruction_origins_never_confer_authority() {
    let (mut action, mut facts) = command_fixture();
    for origin in [
        InstructionOrigin::User,
        InstructionOrigin::Model,
        InstructionOrigin::ProjectFile,
        InstructionOrigin::ApprovedInstructionFile,
        InstructionOrigin::ToolOutput,
        InstructionOrigin::AttachedConversation,
    ] {
        action.origin = origin;
        facts.action = action.clone();
        facts.approvals.clear();
        assert_eq!(
            evaluate(&action, &facts),
            ask(Requirement::Operation, Remember::Conversation)
        );
        facts.approvals.push(ActionApproval {
            action: action.clone(),
            requirement: Requirement::Operation,
        });
        assert_eq!(evaluate(&action, &facts), Decision::Allow);
    }
}

#[test]
fn all_independent_requirements_are_enumerated_with_their_own_remember_rules() {
    let (action, mut facts) = command_fixture();
    facts.resources[0].scope = ResourceScope::ExternalProject;
    facts.project_root_id = Some("other-project".into());
    facts.concrete_change_id = Some("concrete-diff".into());
    let destination = "https://example.test/a".to_string();
    prove(
        &mut facts,
        Effects::Destructive,
        true,
        NetworkControl::Controlled {
            destinations: vec![destination.clone()],
            audit_ready: true,
        },
    );
    let prompts = vec![
        Prompt {
            requirement: Requirement::Operation,
            remember: Remember::Conversation,
        },
        Prompt {
            requirement: Requirement::ExternalResource {
                resource: action.resources[0].clone(),
            },
            remember: Remember::Never,
        },
        Prompt {
            requirement: Requirement::DestructiveConfirmation {
                recovery: Recovery::NotGuaranteed,
            },
            remember: Remember::Never,
        },
        Prompt {
            requirement: Requirement::AlterationReview {
                change_id: "concrete-diff".into(),
            },
            remember: Remember::Never,
        },
        Prompt {
            requirement: Requirement::NetworkDestination { destination },
            remember: Remember::Conversation,
        },
    ];
    assert_eq!(evaluate(&action, &facts), Decision::Ask(prompts.clone()));
    for (index, prompt) in prompts.iter().enumerate() {
        facts.approvals.push(ActionApproval {
            action: action.clone(),
            requirement: prompt.requirement.clone(),
        });
        let expected = if index == 4 {
            Decision::Allow
        } else {
            Decision::Ask(prompts[index + 1..].to_vec())
        };
        assert_eq!(evaluate(&action, &facts), expected);
    }
    facts.capability = Capability::Rejected;
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::CapabilityRejected)
    );
}

#[test]
fn stale_approvals_do_not_survive_arguments_target_or_context_changes() {
    let (action, original) = command_fixture();
    for field in 0..10 {
        let mut next = action.clone();
        match field {
            0 => next.key.context.user_id = "other".into(),
            1 => next.key.context.conversation_id += 1,
            2 => next.key.context.context_revision += 1,
            3 => next.key.context.authorization_revision += 1,
            4 => next.key.context.mode_revision += 1,
            5 => next.key.run_id += 1,
            6 => next.key.action_id += 1,
            7 => next.resources[0].version = "new-version".into(),
            8 => next.resources[0].object_id = "other-object".into(),
            _ => {
                if let Operation::Command { argv, .. } = &mut next.operation {
                    argv.push("--short".into());
                }
            }
        }
        if let Operation::Command {
            working_directory, ..
        } = &mut next.operation
        {
            *working_directory = next.resources[0].clone();
        }
        let mut facts = original.clone();
        facts.action = next.clone();
        facts.current_key = next.key.clone();
        facts.resources[0].resource = next.resources[0].clone();
        facts.read_evidence = ReadEvidence::ProvenReads {
            resources: next.resources.clone(),
        };
        facts.approvals.push(ActionApproval {
            action: action.clone(),
            requirement: Requirement::Operation,
        });
        assert_eq!(
            evaluate(&next, &facts),
            ask(Requirement::Operation, Remember::Conversation)
        );
    }
}

#[test]
fn an_opaque_shell_script_is_blocked_without_bounded_backend_effect_evidence() {
    let (mut action, mut facts) = command_fixture();
    action.operation = Operation::Command {
        executable: "/bin/sh".into(),
        argv: vec!["-c".into(), "cargo test; git push; rm -rf output".into()],
        working_directory: action.resources[0].clone(),
        environment: vec![],
    };
    facts.action = action.clone();
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement: Requirement::Operation,
    });
    for effects in [Effects::Unknown, Effects::Unbounded] {
        prove(&mut facts, effects, true, NetworkControl::Disabled);
        assert_eq!(
            evaluate(&action, &facts),
            Decision::Deny(Denial::UnboundedEffects)
        );
    }
    facts.capability = Capability::Unproven;
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::CapabilityUnproven)
    );
}

#[test]
fn network_grants_are_destination_exact_and_conversation_scoped() {
    let (action, mut facts) = command_fixture();
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement: Requirement::Operation,
    });
    let destination = "https://example.test/a".to_string();
    prove(
        &mut facts,
        Effects::ReadOnly,
        true,
        NetworkControl::Controlled {
            destinations: vec![destination.clone()],
            audit_ready: true,
        },
    );
    facts.grants.push(ConversationGrant {
        context: action.key.context.clone(),
        kind: GrantKind::NetworkDestination {
            destination: destination.clone(),
        },
    });
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
    facts.grants[0].context.conversation_id += 1;
    let expected = ask(
        Requirement::NetworkDestination {
            destination: destination.clone(),
        },
        Remember::Conversation,
    );
    assert_eq!(evaluate(&action, &facts), expected);
    facts.grants[0].context = action.key.context.clone();
    facts.grants[0].kind = GrantKind::NetworkDestination {
        destination: "https://example.test/".into(),
    };
    assert_eq!(evaluate(&action, &facts), expected);
    facts.grants.clear();
    assert_eq!(evaluate(&action, &facts), expected);
}

#[test]
fn destructive_approval_cannot_be_reused_for_another_payload() {
    let (mut action, mut facts) = fixture();
    action.operation = Operation::File(FileOperation::Edit);
    facts.mode = AgentMode::Automatic;
    facts.mode_at_proposal = AgentMode::Automatic;
    facts.action = action.clone();
    prove(
        &mut facts,
        Effects::Destructive,
        false,
        NetworkControl::Disabled,
    );
    let confirmation = Requirement::DestructiveConfirmation {
        recovery: Recovery::NotGuaranteed,
    };
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement: confirmation.clone(),
    });
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
    action.arguments_fingerprint = "different-file-payload".into();
    facts.action = action.clone();
    assert_eq!(
        evaluate(&action, &facts),
        ask(confirmation, Remember::Never)
    );
    action.arguments_fingerprint.clear();
    facts.action = action.clone();
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::InvalidAction)
    );
}

#[test]
fn operation_grants_also_bind_non_argv_arguments() {
    let (mut action, mut facts) = command_fixture();
    facts.grants.push(ConversationGrant {
        context: action.key.context.clone(),
        kind: GrantKind::Operation {
            operation: action.operation.clone(),
            arguments_fingerprint: action.arguments_fingerprint.clone(),
            resources: action.resources.clone(),
        },
    });
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
    action.arguments_fingerprint = "different-stdin".into();
    facts.action = action.clone();
    assert_eq!(
        evaluate(&action, &facts),
        ask(Requirement::Operation, Remember::Conversation)
    );
}

#[test]
fn approved_isolated_command_cannot_read_when_global_reads_are_denied() {
    let (mut action, mut facts) = command_fixture();
    action.operation = Operation::Command {
        executable: "/bin/cat".into(),
        argv: vec!["restricted.txt".into()],
        working_directory: action.resources[0].clone(),
        environment: vec![],
    };
    facts.action = action.clone();
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement: Requirement::Operation,
    });
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
    facts.global_read_allowed = false;
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::ReadRestricted)
    );
}

#[test]
fn approved_mutating_commands_cannot_read_when_global_reads_are_denied() {
    let (action, mut facts) = command_fixture();
    facts.mode = AgentMode::Automatic;
    facts.mode_at_proposal = AgentMode::Automatic;
    facts.approvals = vec![
        ActionApproval {
            action: action.clone(),
            requirement: Requirement::Operation,
        },
        ActionApproval {
            action: action.clone(),
            requirement: Requirement::DestructiveConfirmation {
                recovery: Recovery::NotGuaranteed,
            },
        },
    ];
    for effects in [Effects::Alteration, Effects::Destructive] {
        prove(&mut facts, effects, true, NetworkControl::Disabled);
        facts.global_read_allowed = true;
        assert_eq!(evaluate(&action, &facts), Decision::Allow);
        facts.global_read_allowed = false;
        assert_eq!(
            evaluate(&action, &facts),
            Decision::Deny(Denial::ReadRestricted)
        );
    }
}

#[test]
fn proven_pure_write_command_does_not_require_read_permission() {
    let (mut action, mut facts) = command_fixture();
    action.operation = Operation::Command {
        executable: "/usr/bin/touch".into(),
        argv: vec!["output.txt".into()],
        working_directory: action.resources[0].clone(),
        environment: vec![],
    };
    facts.action = action.clone();
    facts.mode = AgentMode::Automatic;
    facts.mode_at_proposal = AgentMode::Automatic;
    facts.global_read_allowed = false;
    facts.read_evidence = ReadEvidence::ProvenNoReads;
    prove(
        &mut facts,
        Effects::Alteration,
        true,
        NetworkControl::Disabled,
    );
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement: Requirement::Operation,
    });
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
}

#[test]
fn unknown_or_incomplete_read_evidence_blocks_even_approved_commands() {
    let (action, mut facts) = command_fixture();
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement: Requirement::Operation,
    });
    for evidence in [
        ReadEvidence::Unknown,
        ReadEvidence::ProvenReads { resources: vec![] },
        ReadEvidence::ProvenReads {
            resources: vec![Resource {
                root_id: "outside".into(),
                object_id: "unvalidated-source".into(),
                version: "1".into(),
            }],
        },
        ReadEvidence::ProvenReads {
            resources: vec![action.resources[0].clone(), action.resources[0].clone()],
        },
    ] {
        facts.read_evidence = evidence;
        for allowed in [true, false] {
            facts.global_read_allowed = allowed;
            assert_eq!(
                evaluate(&action, &facts),
                Decision::Deny(Denial::ReadEvidenceUnproven)
            );
        }
    }
}

#[test]
fn explicit_file_read_cannot_claim_proven_no_reads() {
    let (action, mut facts) = fixture();
    facts.read_evidence = ReadEvidence::ProvenNoReads;
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::ReadEvidenceUnproven)
    );
}

#[test]
fn approved_upload_cannot_read_its_source_when_global_reads_are_denied() {
    let (mut action, mut facts) = fixture();
    let destination = "https://example.test/upload".to_string();
    action.operation = Operation::Network {
        method: "POST".into(),
        destination: destination.clone(),
    };
    facts.action = action.clone();
    facts.mode = AgentMode::Automatic;
    facts.mode_at_proposal = AgentMode::Automatic;
    prove(
        &mut facts,
        Effects::Alteration,
        false,
        NetworkControl::Controlled {
            destinations: vec![destination.clone()],
            audit_ready: true,
        },
    );
    facts.approvals.push(ActionApproval {
        action: action.clone(),
        requirement: Requirement::NetworkDestination { destination },
    });
    assert_eq!(evaluate(&action, &facts), Decision::Allow);
    facts.global_read_allowed = false;
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::ReadRestricted)
    );
}

#[test]
fn zero_context_revision_is_not_a_valid_backend_context() {
    let (mut action, mut facts) = fixture();
    action.key.context.context_revision = 0;
    facts.action = action.clone();
    facts.current_key = action.key.clone();
    assert_eq!(
        evaluate(&action, &facts),
        Decision::Deny(Denial::InvalidAction)
    );
}
