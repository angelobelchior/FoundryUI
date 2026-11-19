use rusqlite::params;
use serde::Serialize;

use super::{
    AgentStore, AgentStoreError, AgentStoreResult, ContextFact, NetworkEventState, store_error,
};
use crate::foundry::policy::ActionKey;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkLogEntry {
    pub id: i64,
    pub run_id: Option<i64>,
    pub action_id: Option<i64>,
    pub destination: String,
    pub state: NetworkEventState,
    pub http_status: Option<u16>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkLogPage {
    pub entries: Vec<NetworkLogEntry>,
    pub has_more: bool,
    pub next_before: Option<i64>,
}

pub fn sanitized_destination(destination: &str) -> Option<String> {
    let mut url = reqwest::Url::parse(destination).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    url.set_username("").ok()?;
    url.set_password(None).ok()?;
    url.set_path("/[redacted]");
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string())
}

impl AgentStore<'_> {
    pub fn global_read_allowed(&self) -> AgentStoreResult<bool> {
        self.database
            .connection()
            .query_row(
                "SELECT read_allowed FROM agent_authorization_settings WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(store_error)
    }

    pub fn record_network_access(
        &self,
        key: &ActionKey,
        destination: &str,
        state: NetworkEventState,
        http_status: Option<u16>,
    ) -> AgentStoreResult<()> {
        let destination =
            sanitized_destination(destination).ok_or(AgentStoreError::InvalidInput)?;
        let phase = match state {
            NetworkEventState::Requested => "requested",
            NetworkEventState::Blocked => "blocked",
            NetworkEventState::Attempted => "attempted",
            NetworkEventState::Redirected => "redirected",
            NetworkEventState::Completed => "completed",
            NetworkEventState::Failed => "failed",
            NetworkEventState::Cancelled => "cancelled",
        };
        self.record_fact(
            key.context.conversation_id,
            key.context.context_revision,
            Some(key.run_id),
            &format!("network-{}-{phase}", key.action_id),
            &ContextFact::NetworkEvent {
                destination,
                state,
                http_status,
            },
        )?;
        Ok(())
    }

    pub fn network_log(
        &self,
        conversation_id: i64,
        before_id: Option<i64>,
        limit: usize,
    ) -> AgentStoreResult<NetworkLogPage> {
        if !(1..=100).contains(&limit) || before_id.is_some_and(|id| id <= 0) {
            return Err(AgentStoreError::InvalidInput);
        }
        let connection = self.database.connection();
        let exists: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = ?1)",
                [conversation_id],
                |row| row.get(0),
            )
            .map_err(store_error)?;
        if !exists {
            return Err(AgentStoreError::NotFound);
        }
        let mut statement = connection.prepare(
            "SELECT f.id, f.run_id, f.payload_json, f.created_at,
             (SELECT a.id FROM agent_actions a WHERE a.run_id = f.run_id AND a.conversation_id = f.conversation_id
               AND a.tool = 'http_request' AND (f.fact_key = 'network-' || a.id OR f.fact_key LIKE 'network-' || a.id || '-%') LIMIT 1)
             FROM agent_context_facts f WHERE f.conversation_id = ?1
             AND json_extract(f.payload_json, '$.kind') = 'network_event'
             AND (?2 IS NULL OR f.id < ?2) ORDER BY f.id DESC LIMIT ?3",
        ).map_err(store_error)?;
        let rows = statement
            .query_map(params![conversation_id, before_id, limit + 1], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                ))
            })
            .map_err(store_error)?;
        let mut entries = Vec::new();
        for row in rows {
            let (id, run_id, encoded, created_at, action_id) = row.map_err(store_error)?;
            let fact: ContextFact =
                serde_json::from_str(&encoded).map_err(|_| AgentStoreError::Storage)?;
            if let ContextFact::NetworkEvent {
                destination,
                state,
                http_status,
            } = fact
            {
                entries.push(NetworkLogEntry {
                    id,
                    run_id,
                    action_id,
                    destination: sanitized_destination(&destination)
                        .unwrap_or_else(|| "[unavailable]".into()),
                    state,
                    http_status,
                    created_at,
                });
            }
        }
        let has_more = entries.len() > limit;
        entries.truncate(limit);
        let next_before = if has_more {
            entries.last().map(|entry| entry.id)
        } else {
            None
        };
        Ok(NetworkLogPage {
            entries,
            has_more,
            next_before,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foundry::db::Database;
    use crate::foundry::policy::ActionKey;

    #[test]
    fn network_log_is_conversation_scoped_paginated_and_sanitized() {
        let database = Database::open_in_memory().unwrap();
        let first = database.create_conversation("fixture").unwrap();
        let other = database.create_conversation("fixture").unwrap();
        let store = database.agent_store();
        let run = store.start_run(first.id, "run").unwrap();
        let revision = store.authorization_revision(first.id).unwrap();
        let action = store.propose_call(run.id, "call", "http_request", &serde_json::json!({"url":"https://user:secret@example.invalid/private?token=secret#secret"})).unwrap();
        let key = ActionKey {
            context: revision.key().clone(),
            run_id: run.id,
            action_id: action.id,
        };
        for state in [
            NetworkEventState::Requested,
            NetworkEventState::Attempted,
            NetworkEventState::Failed,
        ] {
            store
                .record_network_access(
                    &key,
                    "https://user:secret@example.invalid/private?token=secret#secret",
                    state,
                    None,
                )
                .unwrap();
        }
        let page = store.network_log(first.id, None, 2).unwrap();
        assert!(page.has_more);
        assert_eq!(page.entries[0].action_id, Some(action.id));
        assert_eq!(page.entries[0].state, NetworkEventState::Failed);
        let encoded = serde_json::to_string(&page).unwrap();
        assert!(!encoded.contains("secret"));
        assert!(!encoded.contains("/private"));
        assert!(!encoded.contains("user:"));
        let rest = store.network_log(first.id, page.next_before, 2).unwrap();
        assert_eq!(rest.entries.len(), 1);
        assert!(!rest.has_more);
        assert!(
            store
                .network_log(other.id, None, 100)
                .unwrap()
                .entries
                .is_empty()
        );
    }

    #[test]
    fn global_read_preference_is_persistent_and_invalidates_every_conversation() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("fixture.sqlite");
        {
            let database = Database::open(&path).unwrap();
            let first = database.create_conversation("fixture").unwrap();
            let second = database.create_conversation("fixture").unwrap();
            let store = database.agent_store();
            assert!(store.global_read_allowed().unwrap());
            let before = store.authorization_revision(first.id).unwrap();
            store.authorization_revision(second.id).unwrap();
            store.set_authorization_read_allowed(false).unwrap();
            for id in [first.id, second.id] {
                let revision = store.authorization_revision(id).unwrap();
                assert!(!revision.global_read_allowed());
                assert!(
                    revision.key().authorization_revision > before.key().authorization_revision
                );
            }
        }
        let database = Database::open(&path).unwrap();
        assert!(!database.agent_store().global_read_allowed().unwrap());
    }

    #[test]
    fn deleting_a_conversation_removes_its_network_records_only() {
        let database = Database::open_in_memory().unwrap();
        let first = database.create_conversation("fixture").unwrap();
        let second = database.create_conversation("fixture").unwrap();
        let store = database.agent_store();
        for id in [first.id, second.id] {
            store.context(id).unwrap();
            store
                .record_fact(
                    id,
                    1,
                    None,
                    "log",
                    &ContextFact::NetworkEvent {
                        destination: "https://example.invalid/".into(),
                        state: NetworkEventState::Blocked,
                        http_status: None,
                    },
                )
                .unwrap();
        }
        let plan = store
            .prepare_conversation_deletion("remove", first.id)
            .unwrap();
        store.commit_context_cycle(&plan).unwrap();
        assert!(matches!(
            store.network_log(first.id, None, 100),
            Err(AgentStoreError::NotFound)
        ));
        assert_eq!(
            store
                .network_log(second.id, None, 100)
                .unwrap()
                .entries
                .len(),
            1
        );
    }
}
