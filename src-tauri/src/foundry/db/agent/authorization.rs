use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;

use super::history::{fingerprint, read_action};
use super::lifecycle::{checked_arguments, claim_call_in_transaction, require_active_run};
use super::{
    ActionClaim, ActionRecord, ActionState, AgentMode, AgentStore, AgentStoreError,
    AgentStoreResult, read_run, store_error,
};
use crate::foundry::{jobs::now_millis, policy as p, resources::RootHandle};

mod codec;
use codec::*;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationStatus {
    Ready,
    Pending,
    Denied,
    Claimed,
    Invalidated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementKind {
    Operation,
    StandaloneRoot,
    ExternalResource,
    DestructiveConfirmation,
    AlterationReview,
    NetworkDestination,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementDisplay {
    pub kind: RequirementKind,
    pub target: String,
    pub root_identity_key: Option<String>,
    pub constraints_fingerprint: String,
    pub remember_eligible: bool,
    pub recovery_guaranteed: bool,
    #[serde(default)]
    pub preview_available: bool,
    #[serde(default)]
    pub diff: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthorizationRequest {
    pub id: i64,
    pub conversation_id: i64,
    pub action_id: i64,
    pub run_id: i64,
    pub context_revision: i64,
    pub authorization_revision: i64,
    pub mode_revision: i64,
    pub status: AuthorizationStatus,
    pub requirements: Vec<RequirementDisplay>,
    pub remembered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AuthorizationGrant {
    pub id: i64,
    pub conversation_id: i64,
    pub context_revision: i64,
    pub requirement: RequirementDisplay,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(tag = "decision", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequestDecision {
    Approve { remember: bool },
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DecisionReceipt {
    pub request_id: i64,
    pub status: AuthorizationStatus,
    pub grant_id: Option<i64>,
    pub dispatch_started: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RevocationReceipt {
    pub authorization_revision: i64,
    pub started_or_uncertain_action_ids: Vec<i64>,
    pub automatically_cancelled: bool,
}

#[derive(Debug, Clone)]
pub struct AuthorizationRevision {
    authority_instance: Arc<()>,
    key: p::ContextKey,
    mode: AgentMode,
    project_root_id: Option<String>,
    project_fingerprint: String,
    global_read_allowed: bool,
    categories: p::CategoryPermissions,
}

impl AuthorizationRevision {
    pub fn key(&self) -> &p::ContextKey {
        &self.key
    }
    pub fn mode(&self) -> AgentMode {
        self.mode
    }
    pub fn project_root_id(&self) -> Option<&str> {
        self.project_root_id.as_deref()
    }
    pub fn global_read_allowed(&self) -> bool {
        self.global_read_allowed
    }
    pub fn categories(&self) -> p::CategoryPermissions {
        self.categories
    }
}

#[derive(Debug)]
pub struct AuthorizationProposal {
    authority_instance: Arc<()>,
    action_id: i64,
    conversation_id: i64,
    binding_fingerprint: String,
    status: AuthorizationStatus,
    denial: Option<String>,
}

impl AuthorizationProposal {
    pub fn status(&self) -> &AuthorizationStatus {
        &self.status
    }
    pub fn denial(&self) -> Option<&str> {
        self.denial.as_deref()
    }
    pub fn action_id(&self) -> i64 {
        self.action_id
    }
}

#[derive(Debug)]
pub struct AuthorizedDispatchIntent {
    claim: ActionClaim,
}

impl AuthorizedDispatchIntent {
    pub fn action_claim(&self) -> &ActionClaim {
        &self.claim
    }
}

impl AgentStore<'_> {
    pub fn authorization_revision(
        &self,
        conversation_id: i64,
    ) -> AgentStoreResult<AuthorizationRevision> {
        self.context(conversation_id)?;
        revision(
            &self.database.connection(),
            conversation_id,
            &self.database.authority_instance,
        )
    }

    pub fn set_authorization_mode(
        &self,
        expected: &p::ContextKey,
        mode: AgentMode,
    ) -> AgentStoreResult<AuthorizationRevision> {
        let mut connection = self.database.connection();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let current = revision(
            &tx,
            expected.conversation_id,
            &self.database.authority_instance,
        )?;
        if current.key != *expected {
            return Err(AgentStoreError::StaleContext);
        }
        tx.execute(
            "UPDATE agent_contexts SET mode = ?1 WHERE conversation_id = ?2",
            params![mode_name(mode), expected.conversation_id],
        )
        .map_err(store_error)?;
        let current = revision(
            &tx,
            expected.conversation_id,
            &self.database.authority_instance,
        )?;
        tx.commit().map_err(store_error)?;
        Ok(current)
    }

    pub fn set_authorization_read_allowed(&self, allowed: bool) -> AgentStoreResult<()> {
        let mut connection = self.database.connection();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let changed = tx.execute("UPDATE agent_authorization_settings SET read_allowed = ?1 WHERE id = 1 AND read_allowed != ?1", [allowed]).map_err(store_error)?;
        if changed != 0 {
            tx.execute("INSERT OR IGNORE INTO agent_authorization_revisions(conversation_id) SELECT conversation_id FROM agent_contexts", []).map_err(store_error)?;
            tx.execute("UPDATE agent_authorization_revisions SET authorization_revision = authorization_revision + 1", []).map_err(store_error)?;
        }
        tx.commit().map_err(store_error)
    }

    pub fn category_permissions(&self) -> AgentStoreResult<p::CategoryPermissions> {
        self.database
            .connection()
            .query_row(
                "SELECT read_allowed, edit_auto, command_auto, network_auto FROM agent_authorization_settings WHERE id = 1",
                [],
                |r| {
                    Ok(p::CategoryPermissions {
                        read_auto: r.get(0)?,
                        edit_auto: r.get(1)?,
                        command_auto: r.get(2)?,
                        network_auto: r.get(3)?,
                    })
                },
            )
            .map_err(store_error)
    }

    pub fn set_authorization_categories(
        &self,
        categories: p::CategoryPermissions,
    ) -> AgentStoreResult<()> {
        let mut connection = self.database.connection();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let changed = tx.execute("UPDATE agent_authorization_settings SET read_allowed = ?1, edit_auto = ?2, command_auto = ?3, network_auto = ?4 WHERE id = 1 AND (read_allowed != ?1 OR edit_auto != ?2 OR command_auto != ?3 OR network_auto != ?4)", params![categories.read_auto, categories.edit_auto, categories.command_auto, categories.network_auto]).map_err(store_error)?;
        if changed != 0 {
            tx.execute("INSERT OR IGNORE INTO agent_authorization_revisions(conversation_id) SELECT conversation_id FROM agent_contexts", []).map_err(store_error)?;
            tx.execute("UPDATE agent_authorization_revisions SET authorization_revision = authorization_revision + 1", []).map_err(store_error)?;
        }
        tx.commit().map_err(store_error)
    }

    pub fn register_authorization_root(
        &self,
        conversation_id: i64,
        expected_revision: i64,
        root: &RootHandle,
    ) -> AgentStoreResult<()> {
        let mut connection = self.database.connection();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let current = revision(&tx, conversation_id, &self.database.authority_instance)?;
        if current.key.context_revision != expected_revision {
            return Err(AgentStoreError::StaleContext);
        }
        if current.project_root_id.is_some() {
            return Err(AgentStoreError::InvalidInput);
        }
        register_root(&tx, &current, 0, root)?;
        tx.commit().map_err(store_error)
    }

    pub fn reopen_authorization_root(
        &self,
        expected: &p::ContextKey,
        root_id: &str,
    ) -> AgentStoreResult<RootHandle> {
        let mut connection = self.database.connection();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let current = revision(
            &tx,
            expected.conversation_id,
            &self.database.authority_instance,
        )?;
        if current.key != *expected {
            return Err(AgentStoreError::StaleContext);
        }
        if current.project_root_id.is_some() {
            return Err(AgentStoreError::InvalidInput);
        }
        let hash = requirement_fingerprint(&p::Requirement::StandaloneRoot {
            root_id: root_id.into(),
        })?;
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_authorization_grants WHERE conversation_id = ?1 AND context_revision = ?2 AND authorization_revision = ?3 AND mode_revision = ?4 AND match_fingerprint = ?5 AND revoked = 0)", params![expected.conversation_id, expected.context_revision, expected.authorization_revision, expected.mode_revision, hash], |r| r.get(0)).map_err(store_error)?;
        if !exists {
            return Err(AgentStoreError::NotFound);
        }
        let path = root_path(&tx, &current, root_id, 0)?;
        let root = RootHandle::enroll(std::path::Path::new(&path))
            .map_err(|_| AgentStoreError::Conflict)?;
        if root
            .identity()
            .map_err(|_| AgentStoreError::Conflict)?
            .storage_key()
            != root_id
        {
            return Err(AgentStoreError::Conflict);
        }
        tx.commit().map_err(store_error)?;
        Ok(root)
    }

    pub fn open_resource_root(
        &self,
        conversation_id: i64,
        root_id: &str,
        action_id: i64,
    ) -> AgentStoreResult<RootHandle> {
        let connection = self.database.connection();
        let current = revision(
            &connection,
            conversation_id,
            &self.database.authority_instance,
        )?;
        let path = root_path(&connection, &current, root_id, action_id)?;
        let handle = RootHandle::enroll(std::path::Path::new(&path))
            .map_err(|_| AgentStoreError::Conflict)?;
        if handle
            .identity()
            .map_err(|_| AgentStoreError::Conflict)?
            .storage_key()
            != root_id
        {
            return Err(AgentStoreError::Conflict);
        }
        Ok(handle)
    }

    pub fn grant_authorization_root(
        &self,
        expected: &AuthorizationRevision,
        root: &RootHandle,
    ) -> AgentStoreResult<AuthorizationGrant> {
        if !Arc::ptr_eq(
            &expected.authority_instance,
            &self.database.authority_instance,
        ) {
            return Err(AgentStoreError::Conflict);
        }
        let mut connection = self.database.connection();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let current = revision(
            &tx,
            expected.key.conversation_id,
            &self.database.authority_instance,
        )?;
        if current.key != expected.key
            || current.project_fingerprint != expected.project_fingerprint
        {
            return Err(AgentStoreError::StaleContext);
        }
        let expected = &expected.key;
        if current.project_root_id.is_some() {
            return Err(AgentStoreError::InvalidInput);
        }
        register_root(&tx, &current, 0, root)?;
        let root_id = root
            .identity()
            .map_err(|_| AgentStoreError::Conflict)?
            .storage_key();
        let prompt = p::Prompt {
            requirement: p::Requirement::StandaloneRoot { root_id },
            remember: p::Remember::Conversation,
        };
        let hash = requirement_fingerprint(&prompt.requirement)?;
        let display = requirement_display(&tx, &current, &prompt, &hash, None)?;
        let existing: Option<i64> = tx.query_row("SELECT id FROM agent_authorization_grants WHERE conversation_id = ?1 AND context_revision = ?2 AND authorization_revision = ?3 AND mode_revision = ?4 AND match_fingerprint = ?5 AND revoked = 0 ORDER BY id LIMIT 1", params![expected.conversation_id, expected.context_revision, expected.authorization_revision, expected.mode_revision, hash], |r| r.get(0)).optional().map_err(store_error)?;
        let id = if let Some(id) = existing {
            id
        } else {
            tx.execute("INSERT INTO agent_authorization_grants(conversation_id, context_revision, authorization_revision, mode_revision, match_fingerprint, display_json, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![expected.conversation_id, expected.context_revision, expected.authorization_revision, expected.mode_revision, hash, encode_display(&display)?, now_millis()]).map_err(store_error)?;
            tx.last_insert_rowid()
        };
        tx.commit().map_err(store_error)?;
        Ok(AuthorizationGrant {
            id,
            conversation_id: expected.conversation_id,
            context_revision: expected.context_revision,
            requirement: display,
        })
    }

    pub fn register_action_root(
        &self,
        reference: &ActionRecord,
        root: &RootHandle,
    ) -> AgentStoreResult<()> {
        let mut connection = self.database.connection();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let action = read_action(&tx, reference.id)?;
        check_record(&action, reference)?;
        require_active_run(&tx, &read_run(&tx, action.run_id)?)?;
        let current = revision(
            &tx,
            action.conversation_id,
            &self.database.authority_instance,
        )?;
        if current.project_root_id.is_none() {
            return Err(AgentStoreError::InvalidInput);
        }
        register_root(&tx, &current, action.id, root)?;
        tx.commit().map_err(store_error)
    }

    pub fn prepare_request(
        &self,
        reference: &ActionRecord,
        trusted: &p::TrustedFacts,
    ) -> AgentStoreResult<AuthorizationProposal> {
        let mut connection = self.database.connection();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let record = read_action(&tx, reference.id)?;
        check_record(&record, reference)?;
        require_active_run(&tx, &read_run(&tx, record.run_id)?)?;
        let current = revision(
            &tx,
            record.conversation_id,
            &self.database.authority_instance,
        )?;
        let mut facts = bound_facts(&record, trusted, &current)?;
        let binding = binding_fingerprint(&facts, &current.project_fingerprint)?;
        let semantic_fingerprint = grant_fingerprint(&facts.action, &p::Requirement::Operation)?;
        let denied: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_authorization_denial_latches WHERE run_id = ?1 AND conversation_id = ?2 AND context_revision = ?3 AND semantic_fingerprint = ?4)", params![record.run_id, record.conversation_id, current.key.context_revision, semantic_fingerprint], |row| row.get(0)).map_err(store_error)?;
        if !denied {
            validate_action_roots(&tx, &current, &facts.action)?;
        }
        if let Some(existing) = read_proposal(&tx, record.id, &self.database.authority_instance)? {
            if existing.binding_fingerprint != binding {
                return Err(AgentStoreError::Conflict);
            }
            check_proposal_revision(&tx, record.id, &current)?;
            return Ok(existing);
        }
        if record.state != ActionState::Proposed {
            return Err(AgentStoreError::InvalidTransition);
        }
        if denied {
            facts.denied_actions.push(facts.action.clone());
        }
        materialize_grants(&tx, &current, &mut facts)?;
        let decision = p::evaluate_with(&facts.action, &facts, current.categories);
        let (status, denial, prompts) = match decision {
            p::Decision::Allow => ("ready", None, vec![]),
            p::Decision::Ask(prompts) => ("pending", None, prompts),
            p::Decision::Deny(reason) => ("denied", Some(format!("{reason:?}")), vec![]),
        };
        tx.execute("INSERT INTO agent_authorization_proposals(action_id, conversation_id, context_revision, authorization_revision, mode_revision, binding_fingerprint, semantic_fingerprint, project_fingerprint, proposal_mode, status, denial, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)", params![record.id, record.conversation_id, current.key.context_revision, current.key.authorization_revision, current.key.mode_revision, binding, semantic_fingerprint, current.project_fingerprint, mode_name(current.mode), status, denial, now_millis()]).map_err(store_error)?;
        for resource in &facts.action.resources {
            tx.execute("INSERT OR IGNORE INTO agent_authorization_proposal_roots(action_id, root_id) VALUES (?1,?2)", params![record.id, resource.root_id]).map_err(store_error)?;
        }
        for mut prompt in prompts {
            if action_specific_only(&facts) {
                prompt.remember = p::Remember::Never;
            }
            let requirement_hash = requirement_fingerprint(&prompt.requirement)?;
            let match_hash = grant_fingerprint(&facts.action, &prompt.requirement)?;
            let display = requirement_display(&tx, &current, &prompt, &match_hash, Some(&facts))?;
            tx.execute("INSERT INTO agent_authorization_requests(action_id, requirement_fingerprint, match_fingerprint, display_json, remember_eligible) VALUES (?1,?2,?3,?4,?5)", params![record.id, requirement_hash, match_hash, encode_display(&display)?, display.remember_eligible]).map_err(store_error)?;
        }
        if status == "denied" {
            tx.execute(
                "UPDATE agent_actions SET state = 'denied', updated_at = ?2 WHERE id = ?1",
                params![record.id, now_millis()],
            )
            .map_err(store_error)?;
        }
        let proposal = read_proposal(&tx, record.id, &self.database.authority_instance)?
            .ok_or(AgentStoreError::NotFound)?;
        tx.commit().map_err(store_error)?;
        Ok(proposal)
    }

    pub fn list_requests(
        &self,
        conversation_id: i64,
    ) -> AgentStoreResult<Vec<AuthorizationRequest>> {
        self.context(conversation_id)?;
        let connection = self.database.connection();
        let current = revision(
            &connection,
            conversation_id,
            &self.database.authority_instance,
        )?;
        let mut statement = connection.prepare("SELECT r.id, p.action_id, a.run_id, p.context_revision, p.authorization_revision, p.mode_revision, p.project_fingerprint, r.response, r.display_json, r.remembered, p.status FROM agent_authorization_requests r JOIN agent_authorization_proposals p ON p.action_id = r.action_id JOIN agent_actions a ON a.id = p.action_id WHERE p.conversation_id = ?1 ORDER BY r.id").map_err(store_error)?;
        let rows = statement
            .query_map([conversation_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, bool>(9)?,
                    row.get::<_, String>(10)?,
                ))
            })
            .map_err(store_error)?;
        rows.map(|row| {
            let (
                id,
                action_id,
                run_id,
                context_revision,
                authorization_revision,
                mode_revision,
                project,
                response,
                display,
                remembered,
                proposal_status,
            ) = row.map_err(store_error)?;
            let run = read_run(&connection, run_id)?;
            let valid = context_revision == current.key.context_revision
                && authorization_revision == current.key.authorization_revision
                && mode_revision == current.key.mode_revision
                && project == current.project_fingerprint
                && require_active_run(&connection, &run).is_ok();
            let status = if proposal_status == "claimed" {
                AuthorizationStatus::Claimed
            } else if !valid {
                AuthorizationStatus::Invalidated
            } else if response == "denied" || proposal_status == "denied" {
                AuthorizationStatus::Denied
            } else if response == "approved" {
                AuthorizationStatus::Ready
            } else {
                AuthorizationStatus::Pending
            };
            Ok(AuthorizationRequest {
                id,
                conversation_id,
                action_id,
                run_id,
                context_revision,
                authorization_revision,
                mode_revision,
                status,
                requirements: vec![decode_display(&display)?],
                remembered,
            })
        })
        .collect()
    }

    pub fn decide_request(
        &self,
        conversation_id: i64,
        request_id: i64,
        decision: RequestDecision,
    ) -> AgentStoreResult<DecisionReceipt> {
        let mut connection = self.database.connection();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let current = revision(&tx, conversation_id, &self.database.authority_instance)?;
        let (action_id, response, eligible, match_hash, display): (i64,String,bool,String,String) = tx.query_row("SELECT r.action_id, r.response, r.remember_eligible, r.match_fingerprint, r.display_json FROM agent_authorization_requests r JOIN agent_authorization_proposals p ON p.action_id = r.action_id WHERE r.id = ?1 AND p.conversation_id = ?2", params![request_id, conversation_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional().map_err(store_error)?.ok_or(AgentStoreError::NotFound)?;
        if response != "pending" {
            return Err(AgentStoreError::InvalidTransition);
        }
        check_proposal_revision(&tx, action_id, &current)?;
        let action = read_action(&tx, action_id)?;
        require_active_run(&tx, &read_run(&tx, action.run_id)?)?;
        if action.state != ActionState::Proposed {
            return Err(AgentStoreError::InvalidTransition);
        }
        let (response, remember) = match decision {
            RequestDecision::Approve { remember } if !remember || eligible => {
                validate_registered_roots(&tx, &current, action_id)?;
                ("approved", remember)
            }
            RequestDecision::Approve { .. } => return Err(AgentStoreError::InvalidInput),
            RequestDecision::Deny => ("denied", false),
        };
        tx.execute("UPDATE agent_authorization_requests SET response = ?1, remembered = ?2, decided_at = ?3 WHERE id = ?4 AND response = 'pending'", params![response, remember, now_millis(), request_id]).map_err(store_error)?;
        let grant_id = if remember {
            let existing: Option<i64> = tx.query_row("SELECT id FROM agent_authorization_grants WHERE conversation_id = ?1 AND context_revision = ?2 AND authorization_revision = ?3 AND mode_revision = ?4 AND match_fingerprint = ?5 AND revoked = 0 ORDER BY id LIMIT 1", params![conversation_id, current.key.context_revision, current.key.authorization_revision, current.key.mode_revision, match_hash], |r| r.get(0)).optional().map_err(store_error)?;
            Some(if let Some(id) = existing {
                id
            } else {
                tx.execute("INSERT INTO agent_authorization_grants(conversation_id, context_revision, authorization_revision, mode_revision, match_fingerprint, display_json, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)", params![conversation_id, current.key.context_revision, current.key.authorization_revision, current.key.mode_revision, match_hash, display, now_millis()]).map_err(store_error)?;
                tx.last_insert_rowid()
            })
        } else {
            None
        };
        if response == "denied" {
            tx.execute("INSERT OR IGNORE INTO agent_authorization_denial_latches(run_id, conversation_id, context_revision, semantic_fingerprint, source_action_id, created_at) SELECT a.run_id, p.conversation_id, p.context_revision, p.semantic_fingerprint, p.action_id, ?2 FROM agent_authorization_proposals p JOIN agent_actions a ON a.id = p.action_id WHERE p.action_id = ?1", params![action_id, now_millis()]).map_err(store_error)?;
            tx.execute("UPDATE agent_authorization_proposals SET status = 'denied', denial = 'UserDenied' WHERE conversation_id = ?1 AND context_revision = ?2 AND semantic_fingerprint = (SELECT semantic_fingerprint FROM agent_authorization_proposals WHERE action_id = ?3) AND status IN ('ready', 'pending') AND action_id IN (SELECT id FROM agent_actions WHERE run_id = ?4 AND state = 'proposed')", params![conversation_id, current.key.context_revision, action_id, action.run_id]).map_err(store_error)?;
            tx.execute("UPDATE agent_actions SET state = 'denied', updated_at = ?1 WHERE run_id = ?2 AND conversation_id = ?3 AND context_revision = ?4 AND state = 'proposed' AND id IN (SELECT action_id FROM agent_authorization_proposals WHERE status = 'denied' AND denial = 'UserDenied' AND semantic_fingerprint = (SELECT semantic_fingerprint FROM agent_authorization_proposals WHERE action_id = ?5))", params![now_millis(), action.run_id, conversation_id, current.key.context_revision, action_id]).map_err(store_error)?;
            tx.execute("UPDATE agent_authorization_proposals SET status = 'denied', denial = 'UserDenied' WHERE action_id = ?1", [action_id]).map_err(store_error)?;
            tx.execute(
                "UPDATE agent_actions SET state = 'denied', updated_at = ?2 WHERE id = ?1",
                params![action_id, now_millis()],
            )
            .map_err(store_error)?;
        }
        let status = if response == "denied" {
            AuthorizationStatus::Denied
        } else {
            let pending: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_authorization_requests WHERE action_id = ?1 AND response = 'pending')", [action_id], |r| r.get(0)).map_err(store_error)?;
            if pending {
                AuthorizationStatus::Pending
            } else {
                tx.execute("UPDATE agent_authorization_proposals SET status = 'ready' WHERE action_id = ?1 AND status = 'pending'", [action_id]).map_err(store_error)?;
                AuthorizationStatus::Ready
            }
        };
        tx.commit().map_err(store_error)?;
        Ok(DecisionReceipt {
            request_id,
            status,
            grant_id,
            dispatch_started: false,
        })
    }

    pub fn list_grants(&self, conversation_id: i64) -> AgentStoreResult<Vec<AuthorizationGrant>> {
        self.context(conversation_id)?;
        let connection = self.database.connection();
        let current = revision(
            &connection,
            conversation_id,
            &self.database.authority_instance,
        )?;
        let mut statement = connection.prepare("SELECT id, display_json FROM agent_authorization_grants WHERE conversation_id = ?1 AND context_revision = ?2 AND authorization_revision = ?3 AND mode_revision = ?4 AND revoked = 0 ORDER BY id").map_err(store_error)?;
        let rows = statement
            .query_map(
                params![
                    conversation_id,
                    current.key.context_revision,
                    current.key.authorization_revision,
                    current.key.mode_revision
                ],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)),
            )
            .map_err(store_error)?;
        rows.map(|r| {
            let (id, display) = r.map_err(store_error)?;
            Ok(AuthorizationGrant {
                id,
                conversation_id,
                context_revision: current.key.context_revision,
                requirement: decode_display(&display)?,
            })
        })
        .collect()
    }

    pub fn revoke_grant(
        &self,
        conversation_id: i64,
        grant_id: i64,
    ) -> AgentStoreResult<RevocationReceipt> {
        let mut connection = self.database.connection();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let current = revision(&tx, conversation_id, &self.database.authority_instance)?;
        let changed = tx.execute("UPDATE agent_authorization_grants SET revoked = 1 WHERE id = ?1 AND conversation_id = ?2 AND revoked = 0", params![grant_id, conversation_id]).map_err(store_error)?;
        if changed != 1 {
            return Err(AgentStoreError::NotFound);
        }
        let next = current
            .key
            .authorization_revision
            .checked_add(1)
            .ok_or(AgentStoreError::InvalidInput)?;
        tx.execute("INSERT INTO agent_authorization_revisions(conversation_id, authorization_revision) VALUES (?1,?2) ON CONFLICT(conversation_id) DO UPDATE SET authorization_revision = excluded.authorization_revision", params![conversation_id, next]).map_err(store_error)?;
        tx.execute("UPDATE agent_authorization_grants SET authorization_revision = ?1 WHERE conversation_id = ?2 AND context_revision = ?3 AND authorization_revision = ?4 AND mode_revision = ?5 AND revoked = 0", params![next, conversation_id, current.key.context_revision, current.key.authorization_revision, current.key.mode_revision]).map_err(store_error)?;
        let started = started_actions(&tx, conversation_id)?;
        tx.commit().map_err(store_error)?;
        Ok(RevocationReceipt {
            authorization_revision: next,
            started_or_uncertain_action_ids: started,
            automatically_cancelled: false,
        })
    }

    pub fn claim_authorized_call(
        &self,
        proposal: &AuthorizationProposal,
        reference: &ActionRecord,
        arguments: &Value,
        fresh: &p::TrustedFacts,
    ) -> AgentStoreResult<AuthorizedDispatchIntent> {
        if !Arc::ptr_eq(
            &proposal.authority_instance,
            &self.database.authority_instance,
        ) {
            return Err(AgentStoreError::Conflict);
        }
        let mut connection = self.database.connection();
        let tx = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let record = checked_arguments(&tx, reference, arguments)?;
        if proposal.action_id != record.id || proposal.conversation_id != record.conversation_id {
            return Err(AgentStoreError::Conflict);
        }
        if matches!(
            record.state,
            ActionState::Started
                | ActionState::Complete
                | ActionState::Failed
                | ActionState::Interrupted
        ) {
            return Err(AgentStoreError::AlreadyClaimed);
        }
        require_active_run(&tx, &read_run(&tx, record.run_id)?)?;
        let current = revision(
            &tx,
            record.conversation_id,
            &self.database.authority_instance,
        )?;
        check_proposal_revision(&tx, record.id, &current)?;
        let persisted = read_proposal(&tx, record.id, &self.database.authority_instance)?
            .ok_or(AgentStoreError::NotFound)?;
        if persisted.status == AuthorizationStatus::Denied || record.state != ActionState::Proposed
        {
            return Err(AgentStoreError::InvalidTransition);
        }
        let mut facts = authoritative_facts(&tx, &record, fresh, &current)?;
        let proposed_mode: String = tx
            .query_row(
                "SELECT proposal_mode FROM agent_authorization_proposals WHERE action_id = ?1",
                [record.id],
                |r| r.get(0),
            )
            .map_err(store_error)?;
        facts.mode_at_proposal = parse_mode(&proposed_mode)?;
        let binding = binding_fingerprint(&facts, &current.project_fingerprint)?;
        if binding != proposal.binding_fingerprint || binding != persisted.binding_fingerprint {
            return Err(AgentStoreError::Conflict);
        }
        materialize_grants(&tx, &current, &mut facts)?;
        materialize_approvals(&tx, &mut facts, current.categories)?;
        if p::evaluate_with(&facts.action, &facts, current.categories) != p::Decision::Allow {
            return Err(AgentStoreError::InvalidTransition);
        }
        tx.execute("UPDATE agent_actions SET state = 'approved', updated_at = ?2 WHERE id = ?1 AND state = 'proposed'", params![record.id, now_millis()]).map_err(store_error)?;
        let claim = claim_call_in_transaction(&tx, &record, arguments)?;
        tx.execute(
            "UPDATE agent_authorization_proposals SET status = 'claimed' WHERE action_id = ?1",
            [record.id],
        )
        .map_err(store_error)?;
        tx.commit().map_err(store_error)?;
        Ok(AuthorizedDispatchIntent { claim })
    }
}

fn revision(
    connection: &Connection,
    conversation_id: i64,
    authority_instance: &Arc<()>,
) -> AgentStoreResult<AuthorizationRevision> {
    let (context_revision, mode, auth, mode_revision, project_id, root, binding): (i64,String,i64,i64,Option<i64>,Option<String>,Option<i64>) = connection.query_row("SELECT c.context_revision, c.mode, COALESCE(r.authorization_revision,0), COALESCE(r.mode_revision,0), c.project_id, p.root_identity_key, p.binding_revision FROM agent_contexts c LEFT JOIN agent_authorization_revisions r ON r.conversation_id = c.conversation_id LEFT JOIN agent_projects p ON p.id = c.project_id WHERE c.conversation_id = ?1", [conversation_id], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?))).optional().map_err(store_error)?.ok_or(AgentStoreError::NotFound)?;
    let (read_allowed, edit_auto, command_auto, network_auto): (bool, bool, bool, bool) =
        connection
            .query_row(
                "SELECT read_allowed, edit_auto, command_auto, network_auto FROM agent_authorization_settings WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .map_err(store_error)?;
    Ok(AuthorizationRevision {
        authority_instance: authority_instance.clone(),
        key: p::ContextKey {
            user_id: "local".into(),
            conversation_id,
            context_revision,
            authorization_revision: auth,
            mode_revision,
        },
        mode: parse_mode(&mode)?,
        project_fingerprint: fingerprint(&json!([project_id, root, binding]))?,
        project_root_id: root,
        global_read_allowed: read_allowed,
        categories: p::CategoryPermissions {
            read_auto: read_allowed,
            edit_auto,
            command_auto,
            network_auto,
        },
    })
}

fn register_root(
    connection: &Connection,
    current: &AuthorizationRevision,
    action_scope: i64,
    root: &RootHandle,
) -> AgentStoreResult<()> {
    let identity = root
        .identity()
        .map_err(|_| AgentStoreError::Conflict)?
        .storage_key();
    let display = root
        .display_path()
        .to_str()
        .ok_or(AgentStoreError::InvalidInput)?;
    if display.len() > 4096 || display.chars().any(char::is_control) {
        return Err(AgentStoreError::InvalidInput);
    }
    connection.execute("INSERT INTO agent_authorization_roots(conversation_id, context_revision, root_id, action_scope, display_path) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(conversation_id, context_revision, root_id, action_scope) DO UPDATE SET display_path = excluded.display_path", params![current.key.conversation_id, current.key.context_revision, identity, action_scope, display]).map_err(store_error)?;
    Ok(())
}

fn validate_root(path: &str, expected: &str) -> AgentStoreResult<()> {
    let root =
        RootHandle::enroll(std::path::Path::new(path)).map_err(|_| AgentStoreError::Conflict)?;
    if root
        .identity()
        .map_err(|_| AgentStoreError::Conflict)?
        .storage_key()
        != expected
    {
        return Err(AgentStoreError::Conflict);
    }
    Ok(())
}

fn root_path(
    connection: &Connection,
    current: &AuthorizationRevision,
    root_id: &str,
    action_id: i64,
) -> AgentStoreResult<String> {
    let result = if current.project_root_id.as_deref() == Some(root_id) {
        connection
            .query_row(
                "SELECT display_path FROM agent_projects WHERE root_identity_key = ?1",
                [root_id],
                |r| r.get(0),
            )
            .optional()
            .map_err(store_error)?
    } else {
        connection.query_row("SELECT display_path FROM agent_authorization_roots WHERE conversation_id = ?1 AND context_revision = ?2 AND root_id = ?3 AND action_scope = ?4", params![current.key.conversation_id, current.key.context_revision, root_id, if current.project_root_id.is_some() { action_id } else { 0 }], |r| r.get(0)).optional().map_err(store_error)?
    };
    result.ok_or(AgentStoreError::NotFound)
}

fn validate_registered_roots(
    connection: &Connection,
    current: &AuthorizationRevision,
    action_id: i64,
) -> AgentStoreResult<()> {
    let mut statement = connection
        .prepare("SELECT root_id FROM agent_authorization_proposal_roots WHERE action_id = ?1")
        .map_err(store_error)?;
    let rows = statement
        .query_map([action_id], |r| r.get::<_, String>(0))
        .map_err(store_error)?;
    for row in rows {
        let root = row.map_err(store_error)?;
        validate_root(&root_path(connection, current, &root, action_id)?, &root)?;
    }
    Ok(())
}

fn authoritative_facts(
    connection: &Connection,
    record: &ActionRecord,
    trusted: &p::TrustedFacts,
    current: &AuthorizationRevision,
) -> AgentStoreResult<p::TrustedFacts> {
    let facts = bound_facts(record, trusted, current)?;
    validate_action_roots(connection, current, &facts.action)?;
    Ok(facts)
}

fn validate_action_roots(
    connection: &Connection,
    current: &AuthorizationRevision,
    action: &p::Action,
) -> AgentStoreResult<()> {
    for resource in &action.resources {
        let path = root_path(connection, current, &resource.root_id, action.key.action_id)?;
        validate_root(&path, &resource.root_id)?;
    }
    Ok(())
}

fn bound_facts(
    record: &ActionRecord,
    trusted: &p::TrustedFacts,
    current: &AuthorizationRevision,
) -> AgentStoreResult<p::TrustedFacts> {
    let action = &trusted.action;
    if action.key.context != current.key
        || trusted.current_key != action.key
        || record.context_revision != current.key.context_revision
    {
        return Err(AgentStoreError::StaleContext);
    }
    if action.key.run_id != record.run_id
        || action.key.action_id != record.id
        || action.arguments_fingerprint != record.arguments_fingerprint
    {
        return Err(AgentStoreError::Conflict);
    }
    let mut facts = trusted.clone();
    facts.current_key = action.key.clone();
    facts.mode = current.mode;
    facts.mode_at_proposal = current.mode;
    facts.project_root_id = current.project_root_id.clone();
    facts.global_read_allowed = trusted.global_read_allowed && current.global_read_allowed;
    facts.approvals.clear();
    facts.grants.clear();
    facts.denied_actions.clear();
    facts.resources.clear();
    for resource in &action.resources {
        let scope = match &current.project_root_id {
            Some(root) if root == &resource.root_id => p::ResourceScope::Project,
            Some(_) => p::ResourceScope::ExternalProject,
            None => p::ResourceScope::Standalone,
        };
        facts.resources.push(p::ResolvedResource {
            resource: resource.clone(),
            scope,
        });
    }
    Ok(facts)
}

fn check_record(actual: &ActionRecord, expected: &ActionRecord) -> AgentStoreResult<()> {
    if actual.id != expected.id
        || actual.run_id != expected.run_id
        || actual.conversation_id != expected.conversation_id
        || actual.context_revision != expected.context_revision
        || actual.call_id != expected.call_id
        || actual.tool != expected.tool
        || actual.arguments_fingerprint != expected.arguments_fingerprint
    {
        return Err(AgentStoreError::Conflict);
    }
    Ok(())
}

fn check_proposal_revision(
    connection: &Connection,
    action_id: i64,
    current: &AuthorizationRevision,
) -> AgentStoreResult<()> {
    let valid: bool = connection.query_row("SELECT conversation_id = ?2 AND context_revision = ?3 AND authorization_revision = ?4 AND mode_revision = ?5 AND project_fingerprint = ?6 FROM agent_authorization_proposals WHERE action_id = ?1", params![action_id, current.key.conversation_id, current.key.context_revision, current.key.authorization_revision, current.key.mode_revision, current.project_fingerprint], |r| r.get(0)).optional().map_err(store_error)?.ok_or(AgentStoreError::NotFound)?;
    if !valid {
        return Err(AgentStoreError::StaleContext);
    }
    Ok(())
}

fn read_proposal(
    connection: &Connection,
    action_id: i64,
    authority_instance: &Arc<()>,
) -> AgentStoreResult<Option<AuthorizationProposal>> {
    connection.query_row("SELECT conversation_id, binding_fingerprint, status, denial FROM agent_authorization_proposals WHERE action_id = ?1", [action_id], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<String>>(3)?))).optional().map_err(store_error)?.map(|(conversation_id, binding_fingerprint, status, denial)| Ok(AuthorizationProposal { authority_instance: authority_instance.clone(), action_id, conversation_id, binding_fingerprint, status: match status.as_str() { "ready" => AuthorizationStatus::Ready, "pending" => AuthorizationStatus::Pending, "denied" => AuthorizationStatus::Denied, "claimed" => AuthorizationStatus::Claimed, _ => return Err(AgentStoreError::Storage) }, denial })).transpose()
}

fn materialize_grants(
    connection: &Connection,
    current: &AuthorizationRevision,
    facts: &mut p::TrustedFacts,
) -> AgentStoreResult<()> {
    let prompts = possible_prompts(facts, current.categories);
    for prompt in prompts {
        if prompt.remember != p::Remember::Conversation {
            continue;
        }
        let hash = grant_fingerprint(&facts.action, &prompt.requirement)?;
        let exists: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM agent_authorization_grants WHERE conversation_id = ?1 AND context_revision = ?2 AND authorization_revision = ?3 AND mode_revision = ?4 AND match_fingerprint = ?5 AND revoked = 0)", params![current.key.conversation_id, current.key.context_revision, current.key.authorization_revision, current.key.mode_revision, hash], |r| r.get(0)).map_err(store_error)?;
        if exists && let Some(kind) = grant_kind(&facts.action, &prompt.requirement) {
            facts.grants.push(p::ConversationGrant {
                context: current.key.clone(),
                kind,
            });
        }
    }
    Ok(())
}

fn materialize_approvals(
    connection: &Connection,
    facts: &mut p::TrustedFacts,
    categories: p::CategoryPermissions,
) -> AgentStoreResult<()> {
    for prompt in possible_prompts(facts, categories) {
        let hash = requirement_fingerprint(&prompt.requirement)?;
        let approved: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM agent_authorization_requests WHERE action_id = ?1 AND requirement_fingerprint = ?2 AND response = 'approved')", params![facts.action.key.action_id, hash], |r| r.get(0)).map_err(store_error)?;
        if approved {
            facts.approvals.push(p::ActionApproval {
                action: facts.action.clone(),
                requirement: prompt.requirement,
            });
        }
    }
    Ok(())
}

fn possible_prompts(
    facts: &p::TrustedFacts,
    categories: p::CategoryPermissions,
) -> Vec<p::Prompt> {
    let mut clean = facts.clone();
    clean.approvals.clear();
    clean.grants.clear();
    clean.denied_actions.clear();
    match p::evaluate_with(&clean.action, &clean, categories) {
        p::Decision::Ask(prompts) => prompts,
        _ => vec![],
    }
}

fn action_specific_only(facts: &p::TrustedFacts) -> bool {
    matches!(
        facts.action.operation,
        p::Operation::File(p::FileOperation::Delete)
    ) || matches!(
        facts.capability,
        p::Capability::Proven {
            effects: p::Effects::Destructive,
            ..
        }
    ) || facts
        .resources
        .iter()
        .any(|resource| resource.scope == p::ResourceScope::ExternalProject)
}

fn started_actions(connection: &Connection, conversation_id: i64) -> AgentStoreResult<Vec<i64>> {
    let mut statement = connection.prepare("SELECT id FROM agent_actions WHERE conversation_id = ?1 AND claim_token IS NOT NULL AND state IN ('started', 'interrupted') ORDER BY id").map_err(store_error)?;
    statement
        .query_map([conversation_id], |r| r.get(0))
        .map_err(store_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(store_error)
}

fn mode_name(mode: AgentMode) -> &'static str {
    match mode {
        AgentMode::Review => "review",
        AgentMode::Automatic => "automatic",
    }
}
fn parse_mode(mode: &str) -> AgentStoreResult<AgentMode> {
    match mode {
        "review" => Ok(AgentMode::Review),
        "automatic" => Ok(AgentMode::Automatic),
        _ => Err(AgentStoreError::Storage),
    }
}
