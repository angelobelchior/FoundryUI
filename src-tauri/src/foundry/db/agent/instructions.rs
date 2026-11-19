use std::collections::BTreeSet;
use std::sync::Arc;

use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::authorization::{AuthorizationRevision, RequirementKind};
use super::{AgentStore, AgentStoreError, AgentStoreResult, store_error};
use crate::foundry::jobs::now_millis;
use crate::foundry::policy::ContextKey;
use crate::foundry::resources::{RelativePath, ResourceErrorKind, RootHandle};

const MAX_SOURCE_BYTES: usize = 65_536;
const MAX_CONTEXT_BYTES: usize = 262_144;
const MAX_SOURCES: usize = 32;
const MAX_TARGETS: usize = 16;
const RECOGNIZED_NAMES: [&str; 2] = ["CLAUDE.md", "AGENTS.md"];

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionTarget {
    pub root_id: String,
    pub path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionStatus {
    Pending,
    Approved,
    Denied,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionSource {
    pub root_id: String,
    pub path: String,
    pub identity_key: String,
    pub version_key: String,
    pub content_fingerprint: String,
    pub content: String,
    pub status: InstructionStatus,
    pub preview_available: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstructionOmissionReason {
    Unavailable,
    Blocked,
    TooLarge,
    UnsupportedText,
    LimitReached,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionOmission {
    pub root_id: String,
    pub path: Option<String>,
    pub reason: InstructionOmissionReason,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionDiscovery {
    pub conversation_id: i64,
    pub context_revision: i64,
    pub authorization_revision: i64,
    pub mode_revision: i64,
    pub sources: Vec<InstructionSource>,
    pub omissions: Vec<InstructionOmission>,
}

pub struct InstructionPreview {
    authority_instance: Arc<()>,
    key: ContextKey,
    source: InstructionSource,
}

impl InstructionPreview {
    pub fn source(&self) -> &InstructionSource {
        &self.source
    }
    pub fn key(&self) -> &ContextKey {
        &self.key
    }
}

impl AgentStore<'_> {
    pub fn instruction_root_ids(&self, conversation_id: i64) -> AgentStoreResult<Vec<String>> {
        let revision = self.authorization_revision(conversation_id)?;
        if let Some(root) = revision.project_root_id() {
            return Ok(vec![root.to_string()]);
        }
        let mut roots = self
            .list_grants(conversation_id)?
            .into_iter()
            .filter(|grant| grant.requirement.kind == RequirementKind::StandaloneRoot)
            .filter_map(|grant| grant.requirement.root_identity_key)
            .collect::<Vec<_>>();
        roots.sort();
        roots.dedup();
        Ok(roots)
    }

    pub fn discover_instructions(
        &self,
        conversation_id: i64,
        targets: &[InstructionTarget],
    ) -> AgentStoreResult<InstructionDiscovery> {
        if targets.len() > MAX_TARGETS {
            return Err(AgentStoreError::InvalidInput);
        }
        let revision = self.authorization_revision(conversation_id)?;
        if !revision.global_read_allowed() {
            return Err(AgentStoreError::InvalidTransition);
        }
        let roots = self.instruction_root_ids(conversation_id)?;
        for target in targets {
            if !roots.contains(&target.root_id) {
                return Err(AgentStoreError::NotFound);
            }
            RelativePath::parse(&target.path).map_err(|_| AgentStoreError::InvalidInput)?;
        }
        let mut result = InstructionDiscovery {
            conversation_id,
            context_revision: revision.key().context_revision,
            authorization_revision: revision.key().authorization_revision,
            mode_revision: revision.key().mode_revision,
            sources: Vec::new(),
            omissions: Vec::new(),
        };
        let mut bytes = 0usize;
        for (root_index, root_id) in roots.into_iter().enumerate() {
            if result.sources.len() >= MAX_SOURCES || root_index >= MAX_TARGETS {
                result.omissions.push(InstructionOmission {
                    root_id,
                    path: None,
                    reason: InstructionOmissionReason::LimitReached,
                });
                break;
            }
            let root = match self.instruction_root(&revision, &root_id) {
                Ok(root) => root,
                Err(_) => {
                    result.omissions.push(InstructionOmission {
                        root_id,
                        path: None,
                        reason: InstructionOmissionReason::Unavailable,
                    });
                    continue;
                }
            };
            for path in instruction_paths(&root_id, targets)? {
                if result.sources.len() >= MAX_SOURCES || bytes >= MAX_CONTEXT_BYTES {
                    result.omissions.push(InstructionOmission {
                        root_id: root_id.clone(),
                        path: Some(path),
                        reason: InstructionOmissionReason::LimitReached,
                    });
                    break;
                }
                ensure_current(self, &revision)?;
                match self.read_instruction_source(&revision, &root_id, &root, &path)? {
                    SourceRead::Missing => {}
                    SourceRead::Omitted(reason) => result.omissions.push(InstructionOmission {
                        root_id: root_id.clone(),
                        path: Some(path),
                        reason,
                    }),
                    SourceRead::Found(source) => {
                        if bytes + source.content.len() > MAX_CONTEXT_BYTES {
                            result.omissions.push(InstructionOmission {
                                root_id: root_id.clone(),
                                path: Some(path),
                                reason: InstructionOmissionReason::LimitReached,
                            });
                            break;
                        }
                        bytes += source.content.len();
                        result.sources.push(source);
                    }
                }
            }
        }
        ensure_current(self, &revision)?;
        Ok(result)
    }

    pub fn preview_instruction(
        &self,
        conversation_id: i64,
        root_id: &str,
        path: &str,
    ) -> AgentStoreResult<InstructionPreview> {
        let relative = RelativePath::parse(path).map_err(|_| AgentStoreError::InvalidInput)?;
        let name = relative
            .as_path()
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(AgentStoreError::InvalidInput)?;
        if !RECOGNIZED_NAMES.contains(&name) {
            return Err(AgentStoreError::InvalidInput);
        }
        let revision = self.authorization_revision(conversation_id)?;
        if !revision.global_read_allowed() {
            return Err(AgentStoreError::InvalidTransition);
        }
        let root = self.instruction_root(&revision, root_id)?;
        let path = relative
            .as_path()
            .to_str()
            .ok_or(AgentStoreError::InvalidInput)?;
        let source = match self.read_instruction_source(&revision, root_id, &root, path)? {
            SourceRead::Found(source) => source,
            SourceRead::Missing => return Err(AgentStoreError::NotFound),
            SourceRead::Omitted(_) => return Err(AgentStoreError::InvalidInput),
        };
        ensure_current(self, &revision)?;
        Ok(InstructionPreview {
            authority_instance: self.database.authority_instance.clone(),
            key: revision.key().clone(),
            source,
        })
    }

    pub fn decide_instruction(
        &self,
        preview: &InstructionPreview,
        approve: bool,
    ) -> AgentStoreResult<InstructionSource> {
        if !Arc::ptr_eq(
            &preview.authority_instance,
            &self.database.authority_instance,
        ) {
            return Err(AgentStoreError::Conflict);
        }
        let fresh = self.preview_instruction(
            preview.key.conversation_id,
            &preview.source.root_id,
            &preview.source.path,
        )?;
        if fresh.key != preview.key {
            return Err(AgentStoreError::StaleContext);
        }
        if fresh.source.identity_key != preview.source.identity_key
            || fresh.source.version_key != preview.source.version_key
            || fresh.source.content_fingerprint != preview.source.content_fingerprint
            || fresh.source.content != preview.source.content
        {
            return Err(AgentStoreError::Conflict);
        }
        let mut connection = self.database.connection();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let (context, auth, mode, allowed): (i64, i64, i64, bool) = transaction.query_row(
            "SELECT c.context_revision, COALESCE(r.authorization_revision, 0), COALESCE(r.mode_revision, 0), s.read_allowed
             FROM agent_contexts c LEFT JOIN agent_authorization_revisions r ON r.conversation_id = c.conversation_id
             CROSS JOIN agent_authorization_settings s WHERE c.conversation_id = ?1 AND s.id = 1",
            [preview.key.conversation_id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        ).map_err(store_error)?;
        if !allowed
            || (context, auth, mode)
                != (
                    preview.key.context_revision,
                    preview.key.authorization_revision,
                    preview.key.mode_revision,
                )
        {
            return Err(AgentStoreError::StaleContext);
        }
        let source = &fresh.source;
        let prior: Option<String> = transaction.query_row(
            "SELECT decision FROM agent_instruction_decisions WHERE conversation_id = ?1 AND context_revision = ?2 AND authorization_revision = ?3
             AND root_id = ?4 AND path = ?5 AND identity_key = ?6 AND version_key = ?7 AND content_fingerprint = ?8",
            params![preview.key.conversation_id, context, auth, source.root_id, source.path, source.identity_key, source.version_key, source.content_fingerprint], |row| row.get(0),
        ).optional().map_err(store_error)?;
        let prior = match prior.as_deref() {
            Some("approved") => InstructionStatus::Approved,
            Some("denied") => InstructionStatus::Denied,
            _ => InstructionStatus::Pending,
        };
        if prior != preview.source.status {
            return Err(AgentStoreError::Conflict);
        }
        transaction.execute(
            "INSERT INTO agent_instruction_decisions(conversation_id, context_revision, authorization_revision, root_id, path, identity_key, version_key, content_fingerprint, decision, decided_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
             ON CONFLICT DO UPDATE SET decision = excluded.decision, decided_at = excluded.decided_at",
            params![preview.key.conversation_id, context, auth, source.root_id, source.path, source.identity_key, source.version_key, source.content_fingerprint, if approve { "approved" } else { "denied" }, now_millis()],
        ).map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        let mut source = fresh.source;
        source.status = if approve {
            InstructionStatus::Approved
        } else {
            InstructionStatus::Denied
        };
        Ok(source)
    }

    fn instruction_root(
        &self,
        revision: &AuthorizationRevision,
        root_id: &str,
    ) -> AgentStoreResult<RootHandle> {
        if revision.project_root_id() == Some(root_id) {
            self.open_resource_root(revision.key().conversation_id, root_id, 0)
        } else if revision.project_root_id().is_none() {
            self.reopen_authorization_root(revision.key(), root_id)
        } else {
            Err(AgentStoreError::NotFound)
        }
    }

    fn read_instruction_source(
        &self,
        revision: &AuthorizationRevision,
        root_id: &str,
        root: &RootHandle,
        path: &str,
    ) -> AgentStoreResult<SourceRead> {
        let relative = RelativePath::parse(path).map_err(|_| AgentStoreError::InvalidInput)?;
        let read = root
            .open_read(&relative)
            .and_then(|read| read.read_versioned(MAX_SOURCE_BYTES));
        let read = match read {
            Ok(read) => read,
            Err(error) if error.raw_os_error == Some(libc::ENOENT) => {
                return Ok(SourceRead::Missing);
            }
            Err(error) => {
                return Ok(SourceRead::Omitted(match error.kind {
                    ResourceErrorKind::LimitExceeded => InstructionOmissionReason::TooLarge,
                    ResourceErrorKind::AccessDenied
                    | ResourceErrorKind::Revoked
                    | ResourceErrorKind::UnsupportedResourceType => {
                        InstructionOmissionReason::Blocked
                    }
                    _ => InstructionOmissionReason::Unavailable,
                }));
            }
        };
        let content_fingerprint = format!("{:x}", Sha256::digest(&read.bytes));
        let content = match String::from_utf8(read.bytes) {
            Ok(content) if printable_instruction(&content) => content,
            _ => {
                return Ok(SourceRead::Omitted(
                    InstructionOmissionReason::UnsupportedText,
                ));
            }
        };
        let connection = self.database.connection();
        let status: Option<String> = connection.query_row(
            "SELECT decision FROM agent_instruction_decisions WHERE conversation_id = ?1 AND context_revision = ?2 AND authorization_revision = ?3
             AND root_id = ?4 AND path = ?5 AND identity_key = ?6 AND version_key = ?7 AND content_fingerprint = ?8",
            params![revision.key().conversation_id, revision.key().context_revision, revision.key().authorization_revision, root_id, path, read.identity_key, read.version_key, content_fingerprint], |row| row.get(0),
        ).optional().map_err(store_error)?;
        let status = match status.as_deref() {
            Some("approved") => InstructionStatus::Approved,
            Some("denied") => InstructionStatus::Denied,
            _ => InstructionStatus::Pending,
        };
        Ok(SourceRead::Found(InstructionSource {
            root_id: root_id.into(),
            path: path.into(),
            identity_key: read.identity_key,
            version_key: read.version_key,
            content_fingerprint,
            preview_available: content.len() <= 2048
                && content.lines().count() <= 8
                && content.lines().all(|line| line.chars().count() <= 80)
                && path.chars().count() <= 256,
            content,
            status,
        }))
    }
}

enum SourceRead {
    Missing,
    Omitted(InstructionOmissionReason),
    Found(InstructionSource),
}

fn ensure_current(
    store: &AgentStore<'_>,
    expected: &AuthorizationRevision,
) -> AgentStoreResult<()> {
    let current = store.authorization_revision(expected.key().conversation_id)?;
    if current.key() != expected.key() || !current.global_read_allowed() {
        return Err(AgentStoreError::StaleContext);
    }
    Ok(())
}

fn instruction_paths(
    root_id: &str,
    targets: &[InstructionTarget],
) -> AgentStoreResult<Vec<String>> {
    let mut directories = BTreeSet::from([String::new()]);
    for target in targets.iter().filter(|target| target.root_id == root_id) {
        let relative =
            RelativePath::parse(&target.path).map_err(|_| AgentStoreError::InvalidInput)?;
        let mut prefix = std::path::PathBuf::new();
        if let Some(parent) = relative.as_path().parent() {
            for component in parent.components() {
                prefix.push(component);
                if prefix.components().count() > 16 {
                    return Err(AgentStoreError::InvalidInput);
                }
                directories.insert(
                    prefix
                        .to_str()
                        .ok_or(AgentStoreError::InvalidInput)?
                        .to_string(),
                );
            }
        }
    }
    let mut directories = directories.into_iter().collect::<Vec<_>>();
    directories.sort_by_key(|path| {
        (
            path.split('/').filter(|part| !part.is_empty()).count(),
            path.clone(),
        )
    });
    Ok(directories
        .into_iter()
        .flat_map(|directory| {
            RECOGNIZED_NAMES.into_iter().map(move |name| {
                if directory.is_empty() {
                    name.to_string()
                } else {
                    format!("{directory}/{name}")
                }
            })
        })
        .collect())
}

fn printable_instruction(text: &str) -> bool {
    !text.chars().any(|character| (character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        || matches!(character, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}'))
}
