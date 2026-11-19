use async_trait::async_trait;
use serde_json::{Value, json};

use super::cycle::{ToolExecution, ToolExecutor, ToolHost};
use super::parser::ToolCall;
use super::schema::{ToolAvailability, ToolRegistry, ToolSpec, ToolSpecError};
use crate::foundry::db::agent::authorization::{AuthorizationRevision, RequirementKind};
use crate::foundry::db::agent::{ActionRecord, AgentStore};
use crate::foundry::policy as p;
use crate::foundry::resources::{DirectoryEntryKind, RelativePath, ResourceErrorKind, RootHandle};

const MAX_SEARCH_DEPTH: usize = 8;
const MAX_SEARCH_ENTRIES: usize = 2000;
const MAX_SEARCH_READ_BYTES: usize = 256 * 1024;
const MAX_SEARCH_MATCHES: usize = 50;
const MAX_SEARCH_SNIPPET: usize = 200;
const PLACEHOLDER_VERSION: &str = "search";

pub fn register(registry: &mut ToolRegistry) -> Result<(), ToolSpecError> {
    registry.register(super::schema::RegisteredTool {
        spec: ToolSpec {
            name: "search_files".into(),
            description: "Search the authorized project files for a literal query.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": {"type": "string", "minLength": 1, "maxLength": 512},
                    "path": {"type": "string", "minLength": 1, "maxLength": 4096}
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        },
        availability: ToolAvailability::Available,
    })
}

pub struct SearchExecutor;

#[async_trait]
impl ToolExecutor for SearchExecutor {
    async fn execute(
        &mut self,
        call: &ToolCall,
        _spec: &ToolSpec,
        host: &ToolHost<'_, '_>,
    ) -> ToolExecution {
        if !host.reads_current() {
            return ToolExecution::Failed {
                summary: json!({"status": "failed", "code": "read_restricted"}),
            };
        }
        let Some(query) = call.arguments.get("query").and_then(Value::as_str) else {
            return search_failure();
        };
        if query.is_empty() {
            return search_failure();
        }
        let Some(resource) = host.facts.action.resources.first() else {
            return search_failure();
        };
        let conversation_id = host.facts.action.key.context.conversation_id;
        let action_id = host.facts.action.key.action_id;
        let root =
            match host
                .store
                .open_resource_root(conversation_id, &resource.root_id, action_id)
            {
                Ok(root) => root,
                Err(_) => return search_failure(),
            };
        let base = match call.arguments.get("path").and_then(Value::as_str) {
            Some(path) => match RelativePath::parse(path) {
                Ok(relative) => Some(relative),
                Err(_) => return search_failure(),
            },
            None => None,
        };
        let mut walker = SearchWalker {
            root: &root,
            read_allowed: &|| host.reads_current(),
            authority_changed: false,
            query: query.to_lowercase(),
            matches: Vec::new(),
            omitted: Vec::new(),
            entries_seen: 0usize,
            matches_found: 0usize,
            truncated: false,
        };
        let base_path = base
            .as_ref()
            .map(|relative| relative.as_path().to_string_lossy().into_owned())
            .unwrap_or_default();
        walker.walk(base.as_ref(), &base_path, 0);
        if walker.authority_changed || !host.reads_current() {
            return ToolExecution::Failed {
                summary: json!({"status": "failed", "code": "read_restricted"}),
            };
        }
        ToolExecution::Succeeded {
            summary: json!({
                "status": "succeeded",
                "matches": walker.matches,
                "omitted": walker.omitted,
                "truncated": walker.truncated
                    || walker.matches_found > walker.matches.len()
                    || walker.entries_seen > MAX_SEARCH_ENTRIES,
            }),
        }
    }
}

struct SearchWalker<'a> {
    root: &'a RootHandle,
    read_allowed: &'a (dyn Fn() -> bool + Sync),
    authority_changed: bool,
    query: String,
    matches: Vec<Value>,
    omitted: Vec<Value>,
    entries_seen: usize,
    matches_found: usize,
    truncated: bool,
}

impl SearchWalker<'_> {
    fn walk(&mut self, relative: Option<&RelativePath>, base: &str, depth: usize) {
        if !(self.read_allowed)() {
            self.authority_changed = true;
            return;
        }
        let entries = match self.root.list_directory(relative) {
            Ok(entries) => entries,
            Err(_) => {
                if base.is_empty() {
                    return;
                }
                self.omitted.push(json!({
                    "path": base,
                    "reason": "inaccessible",
                }));
                return;
            }
        };
        for entry in entries {
            if !(self.read_allowed)() {
                self.authority_changed = true;
                return;
            }
            if self.entries_seen >= MAX_SEARCH_ENTRIES {
                self.truncated = true;
                break;
            }
            self.entries_seen += 1;
            let entry_path = join_path(base, &entry.name);
            match entry.kind {
                DirectoryEntryKind::Directory => {
                    if depth < MAX_SEARCH_DEPTH {
                        match RelativePath::parse(&entry_path) {
                            Ok(subdir) => self.walk(Some(&subdir), &entry_path, depth + 1),
                            Err(_) => self.omitted.push(json!({
                                "path": entry_path,
                                "reason": "inaccessible",
                            })),
                        }
                    }
                }
                DirectoryEntryKind::File => {
                    let read = self.read_and_search(&entry_path);
                    if let Some(reason) = read {
                        self.omitted.push(json!({
                            "path": entry_path,
                            "reason": reason,
                        }));
                    }
                }
                DirectoryEntryKind::Symlink => {
                    self.omitted.push(json!({
                        "path": entry_path,
                        "reason": "symlink",
                    }));
                }
                DirectoryEntryKind::Other => {
                    self.omitted.push(json!({
                        "path": entry_path,
                        "reason": "inaccessible",
                    }));
                }
            }
        }
    }

    fn read_and_search(&mut self, path: &str) -> Option<&'static str> {
        if !(self.read_allowed)() {
            self.authority_changed = true;
            return Some("inaccessible");
        }
        let relative = match RelativePath::parse(path) {
            Ok(relative) => relative,
            Err(_) => return Some("inaccessible"),
        };
        let bytes = match self.root.read(&relative, MAX_SEARCH_READ_BYTES) {
            Ok(bytes) => bytes,
            Err(error) => {
                return Some(match error.kind {
                    ResourceErrorKind::LimitExceeded => "large",
                    _ => "inaccessible",
                });
            }
        };
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(_) => return Some("binary"),
        };
        let mut found = 0usize;
        for line in text.split('\n') {
            let line = line.strip_suffix('\r').unwrap_or(line);
            if line.to_lowercase().contains(&self.query) {
                found += 1;
                if self.matches.len() < MAX_SEARCH_MATCHES {
                    self.matches.push(json!({
                        "path": path,
                        "snippet": snippet(line, &self.query),
                    }));
                }
            }
        }
        self.matches_found += found;
        None
    }
}

fn snippet(line: &str, query: &str) -> String {
    let trimmed = line.trim();
    if trimmed.chars().count() <= MAX_SEARCH_SNIPPET {
        return trimmed.to_string();
    }
    let lower = trimmed.to_lowercase();
    let position = match lower.find(query) {
        Some(position) => position,
        None => return trimmed.chars().take(MAX_SEARCH_SNIPPET).collect(),
    };
    let start = position.saturating_sub(MAX_SEARCH_SNIPPET / 2);
    let start = snap_char(trimmed, start);
    let mut end = start + MAX_SEARCH_SNIPPET;
    if end > trimmed.len() {
        end = trimmed.len();
    }
    let end = snap_char(trimmed, end);
    trimmed[start..end].to_string()
}

fn snap_char(text: &str, byte: usize) -> usize {
    if text.is_char_boundary(byte) {
        return byte;
    }
    (0..=byte)
        .rev()
        .find(|candidate| text.is_char_boundary(*candidate))
        .unwrap_or(0)
}

fn join_path(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
}

fn search_failure() -> ToolExecution {
    ToolExecution::Failed {
        summary: json!({"status": "failed", "code": "search"}),
    }
}

pub struct SearchAuthorizer;

impl super::cycle::ToolAuthorizer for SearchAuthorizer {
    fn facts(
        &self,
        store: &AgentStore<'_>,
        revision: &AuthorizationRevision,
        action: &ActionRecord,
        call: &ToolCall,
    ) -> Result<p::TrustedFacts, String> {
        let object_id = call
            .arguments
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(".")
            .to_string();
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
        let resource = p::Resource {
            root_id,
            object_id,
            version: PLACEHOLDER_VERSION.to_string(),
        };
        let policy_action = p::Action {
            key: p::ActionKey {
                context: revision.key().clone(),
                run_id: action.run_id,
                action_id: action.id,
            },
            operation: p::Operation::File(p::FileOperation::Read),
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
            read_evidence: p::ReadEvidence::ProvenReads {
                resources: vec![resource.clone()],
            },
            capability: p::Capability::Proven {
                effects: p::Effects::ReadOnly,
                opened_resources_validated: true,
                process_isolation: false,
                network: p::NetworkControl::Disabled,
            },
            resources: vec![p::ResolvedResource { resource, scope }],
            concrete_change_id: None,
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
    use std::os::unix::fs::symlink;

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
                .create_project(&root, Some("ContextSearch"))
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

    #[test]
    fn scoped_search_keeps_the_relative_directory_in_returned_paths() {
        let fixture = Fixture::project();
        std::fs::create_dir(fixture._directory.path().join("src")).unwrap();
        std::fs::write(
            fixture._directory.path().join("src/file.txt"),
            b"needle in src",
        )
        .unwrap();
        std::fs::write(
            fixture._directory.path().join("file.txt"),
            b"not the target",
        )
        .unwrap();
        let summary = succeeded(execute(&fixture, json!({"query": "needle", "path": "src"})));
        assert_eq!(summary["matches"][0]["path"], "src/file.txt");
    }

    #[test]
    fn hidden_and_git_ignored_files_are_eligible_by_default() {
        let fixture = Fixture::project();
        std::fs::write(fixture._directory.path().join(".env"), b"needle").unwrap();
        std::fs::write(
            fixture._directory.path().join(".gitignore"),
            b"private.txt\n",
        )
        .unwrap();
        std::fs::write(fixture._directory.path().join("private.txt"), b"needle").unwrap();
        let summary = succeeded(execute(&fixture, json!({"query": "needle"})));
        assert!(
            summary["matches"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == ".env")
        );
        assert!(
            summary["matches"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == "private.txt")
        );
    }

    #[test]
    fn prepared_search_does_not_return_content_after_global_restriction() {
        let fixture = Fixture::project();
        std::fs::write(
            fixture._directory.path().join("private.txt"),
            b"needle synthetic-private",
        )
        .unwrap();
        let store = fixture.store();
        let call = call(json!({"query": "needle"}));
        let (record, revision) = propose(&store, fixture.conversation, &call.arguments);
        let trusted = facts(&store, &revision, &record, &call);
        store.set_authorization_read_allowed(false).unwrap();
        let result = block_on(SearchExecutor.execute(
            &call,
            &spec(),
            &ToolHost {
                store: &store,
                facts: &trusted,
            },
        ));
        assert!(
            matches!(&result, ToolExecution::Failed { summary } if summary["code"] == "read_restricted")
        );
        assert!(!format!("{result:?}").contains("synthetic-private"));
    }

    #[test]
    fn a_walk_stops_between_files_when_authority_is_revoked() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("first.txt"), b"needle").unwrap();
        std::fs::write(folder.path().join("second.txt"), b"needle").unwrap();
        let root = RootHandle::enroll(folder.path()).unwrap();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let allowed = || calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) < 3;
        let mut walker = SearchWalker {
            root: &root,
            read_allowed: &allowed,
            authority_changed: false,
            query: "needle".into(),
            matches: Vec::new(),
            omitted: Vec::new(),
            entries_seen: 0,
            matches_found: 0,
            truncated: false,
        };
        walker.walk(None, "", 0);
        assert!(walker.authority_changed);
        assert!(walker.matches.len() < 2);
    }

    fn call(arguments: Value) -> ToolCall {
        ToolCall {
            id: "call-a".into(),
            name: "search_files".into(),
            arguments,
        }
    }

    fn spec() -> ToolSpec {
        let mut registry = ToolRegistry::default();
        register(&mut registry).expect("register");
        registry
            .get("search_files")
            .expect("registered")
            .spec
            .clone()
    }

    fn propose(
        store: &AgentStore<'_>,
        conversation: i64,
        arguments: &Value,
    ) -> (ActionRecord, AuthorizationRevision) {
        let run = store
            .start_run(conversation, "context-search")
            .expect("run");
        let record = store
            .propose_call(run.id, "call-a", "search_files", arguments)
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
        SearchAuthorizer
            .facts(store, revision, action, call)
            .expect("facts")
    }

    fn execute(fixture: &Fixture, arguments: Value) -> ToolExecution {
        let store = fixture.store();
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);
        let mut executor = SearchExecutor;
        let host = ToolHost {
            store: &store,
            facts: &trusted,
        };
        block_on(executor.execute(&call, &spec(), &host))
    }

    fn succeeded(execution: ToolExecution) -> Value {
        match execution {
            ToolExecution::Succeeded { summary } => summary,
            other => panic!("expected success, got {other:?}"),
        }
    }

    fn match_paths(summary: &Value) -> Vec<String> {
        summary["matches"]
            .as_array()
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(|entry| entry["path"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn omitted<'a>(summary: &'a Value, path: &str) -> Option<&'a str> {
        summary["omitted"]
            .as_array()
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry["path"].as_str() == Some(path))
            })
            .and_then(|entry| entry["reason"].as_str())
    }

    #[test]
    fn an_in_scope_search_is_allowed_without_prompts() {
        let fixture = Fixture::project();
        let store = fixture.store();
        let arguments = json!({"query": "needle"});
        let (record, revision) = propose(&store, fixture.conversation, &arguments);
        let call = call(arguments);
        let trusted = facts(&store, &revision, &record, &call);

        assert_eq!(p::evaluate(&trusted.action, &trusted), p::Decision::Allow);
    }

    #[test]
    fn the_executor_finds_a_file_by_literal_query() {
        let fixture = Fixture::project();
        std::fs::write(
            fixture._directory.path().join("a.txt"),
            b"the needle in the haystack\n",
        )
        .expect("write");
        let summary = succeeded(execute(&fixture, json!({"query": "needle"})));

        assert_eq!(summary["status"], "succeeded");
        assert_eq!(match_paths(&summary), vec!["a.txt"]);
        let snippet = summary["matches"][0]["snippet"].as_str().expect("snippet");
        assert!(snippet.to_lowercase().contains("needle"));
    }

    #[test]
    fn a_binary_file_is_omitted_as_binary() {
        let fixture = Fixture::project();
        std::fs::write(
            fixture._directory.path().join("blob.bin"),
            [0u8, 159, 146, 150],
        )
        .expect("write");
        let summary = succeeded(execute(&fixture, json!({"query": "needle"})));

        assert!(match_paths(&summary).is_empty());
        assert_eq!(omitted(&summary, "blob.bin"), Some("binary"));
    }

    #[test]
    fn a_symlink_pointing_outside_the_root_is_omitted_and_never_read() {
        let fixture = Fixture::project();
        let outside = tempfile::tempdir().expect("outside");
        let target = outside.path().join("secret.txt");
        std::fs::write(&target, b"needle outside the root").expect("write");
        symlink(&target, fixture._directory.path().join("link")).expect("symlink");
        let summary = succeeded(execute(&fixture, json!({"query": "needle"})));

        assert!(match_paths(&summary).is_empty());
        assert_eq!(omitted(&summary, "link"), Some("symlink"));
    }

    #[test]
    fn a_query_with_no_matches_returns_an_empty_matches_list() {
        let fixture = Fixture::project();
        std::fs::write(
            fixture._directory.path().join("notes.txt"),
            b"nothing relevant here\n",
        )
        .expect("write");
        let summary = succeeded(execute(&fixture, json!({"query": "absent"})));

        assert_eq!(summary["status"], "succeeded");
        assert!(match_paths(&summary).is_empty());
    }

    #[test]
    fn a_large_file_is_omitted_as_large() {
        let fixture = Fixture::project();
        let oversized = vec![b'x'; MAX_SEARCH_READ_BYTES + 1];
        std::fs::write(fixture._directory.path().join("big.txt"), &oversized).expect("write");
        let summary = succeeded(execute(&fixture, json!({"query": "needle"})));

        assert_eq!(omitted(&summary, "big.txt"), Some("large"));
    }
}
