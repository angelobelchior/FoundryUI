use super::*;
use crate::foundry::db::Database;
use crate::foundry::resources::RootHandle;
use serde_json::json;

struct Fixture {
    database: Database,
    _directory: tempfile::TempDir,
    root: RootHandle,
}

impl Fixture {
    fn new() -> Self {
        let database = Database::open_in_memory().expect("database");
        let directory = tempfile::tempdir().expect("directory");
        let root = RootHandle::enroll(directory.path()).expect("root");
        Self {
            database,
            _directory: directory,
            root,
        }
    }

    fn project_conversation(&self) -> i64 {
        let store = self.database.agent_store();
        let project = store
            .create_project(&self.root, Some("fixture"))
            .expect("project");
        store
            .create_project_conversation(Some(project.id), "fixture-model")
            .expect("conversation")
            .conversation
            .id
    }

    fn standalone_conversation(&self) -> i64 {
        let conversation = self
            .database
            .create_conversation("fixture-model")
            .expect("conversation")
            .id;
        let store = self.database.agent_store();
        store.context(conversation).expect("context");
        store
            .register_authorization_root(conversation, 1, &self.root)
            .expect("root");
        conversation
    }

    fn start(&self, conversation: i64) -> i64 {
        self.database
            .agent_store()
            .start_run(conversation, "request")
            .expect("run")
            .id
    }
}

fn propose(store: &AgentStore<'_>, run: i64, call_id: &str) -> ActionRecord {
    store
        .propose_call(run, call_id, "read_file", &json!({"path": "a.txt"}))
        .expect("proposal")
}

fn start_writer(store: &AgentStore<'_>, run: i64, call_id: &str) -> i64 {
    let record = propose(store, run, call_id);
    let approved = store
        .decide_call(
            &record,
            &json!({"path": "a.txt"}),
            ApprovalDecision::Approved,
        )
        .expect("approval");
    store
        .claim_call(&approved, &json!({"path": "a.txt"}))
        .expect("claim");
    record.id
}

#[test]
fn cancel_marks_pending_actions_cancelled_and_blocks_late_decisions() {
    let fixture = Fixture::new();
    let conversation = fixture.project_conversation();
    let store = fixture.database.agent_store();
    let run = fixture.start(conversation);
    let record = propose(&store, run, "call-a");

    let receipt = store.cancel_run(run).expect("cancel");

    assert_eq!(receipt.state, RunState::Cancelled);
    assert_eq!(receipt.cancelled_actions, 1);
    assert_eq!(receipt.interrupted_actions, 0);
    assert_eq!(receipt.writer_remaining, false);
    assert_eq!(store.run(run).expect("run").state, RunState::Cancelled);
    assert_eq!(
        store.action(record.id).expect("action").state,
        ActionState::Cancelled
    );
    assert_eq!(
        store.decide_call(
            &record,
            &json!({"path": "a.txt"}),
            ApprovalDecision::Approved
        ),
        Err(AgentStoreError::InvalidTransition)
    );
    let events = store.events(conversation).expect("events");
    assert!(events.iter().any(|entry| matches!(
        entry.content,
        HistoryContent::StateEvent {
            event: AgentEventState::RunCancelled,
            ..
        }
    )));
    assert!(events.iter().any(|entry| matches!(
        entry.content,
        HistoryContent::StateEvent {
            event: AgentEventState::ActionCancelled,
            ..
        }
    )));
}

#[test]
fn cancel_marks_a_started_action_interrupted_and_keeps_the_writer() {
    let fixture = Fixture::new();
    let conversation = fixture.project_conversation();
    let store = fixture.database.agent_store();
    let run = fixture.start(conversation);
    start_writer(&store, run, "call-a");

    let receipt = store.cancel_run(run).expect("cancel");

    assert_eq!(receipt.state, RunState::Cancelled);
    assert_eq!(receipt.cancelled_actions, 0);
    assert_eq!(receipt.interrupted_actions, 1);
    assert_eq!(receipt.writer_remaining, true);
    assert_eq!(store.run(run).expect("run").state, RunState::Cancelled);
    let actions = store.history(conversation).expect("history");
    assert!(!actions.is_empty());
}

#[test]
fn cancel_is_idempotent() {
    let fixture = Fixture::new();
    let conversation = fixture.project_conversation();
    let store = fixture.database.agent_store();
    let run = fixture.start(conversation);
    propose(&store, run, "call-a");

    let first = store.cancel_run(run).expect("cancel");
    let second = store.cancel_run(run).expect("cancel again");

    assert_eq!(first.state, RunState::Cancelled);
    assert_eq!(second.state, RunState::Cancelled);
    assert_eq!(store.run(run).expect("run").state, RunState::Cancelled);
}

#[test]
fn cancel_of_a_finished_run_is_refused() {
    let fixture = Fixture::new();
    let conversation = fixture.project_conversation();
    let store = fixture.database.agent_store();
    let run = fixture.start(conversation);
    store.finish_run(run).expect("finish");

    assert_eq!(
        store.cancel_run(run),
        Err(AgentStoreError::InvalidTransition)
    );
}

#[test]
fn a_second_conversation_reports_the_run_occupying_its_project() {
    let fixture = Fixture::new();
    let store = fixture.database.agent_store();
    let project = store
        .create_project(&fixture.root, Some("fixture"))
        .expect("project");
    let first = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .expect("first")
        .conversation
        .id;
    let second = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .expect("second")
        .conversation
        .id;
    let run = store.start_run(first, "request").expect("run");

    let holder = store
        .active_run_for_conversation(second)
        .expect("holder")
        .expect("blocked");

    assert_eq!(holder.run_id, run.id);
    assert_eq!(holder.conversation_id, first);
    assert_eq!(holder.state, RunState::Prepared);
}

#[test]
fn overlapping_roots_block_a_second_writer() {
    let fixture = Fixture::new();
    let store = fixture.database.agent_store();
    let project_conversation = fixture.project_conversation();
    let run = fixture.start(project_conversation);
    start_writer(&store, run, "call-a");

    let standalone = fixture.standalone_conversation();
    let second = fixture.start(standalone);
    let record = propose(&store, second, "call-b");
    let approved = store
        .decide_call(
            &record,
            &json!({"path": "a.txt"}),
            ApprovalDecision::Approved,
        )
        .expect("approval");

    assert!(matches!(
        store.claim_call(&approved, &json!({"path": "a.txt"})),
        Err(AgentStoreError::InvalidTransition)
    ));
}

#[test]
fn disjoint_roots_allow_independent_writers() {
    let fixture = Fixture::new();
    let store = fixture.database.agent_store();
    let first = fixture.standalone_conversation();
    let first_run = fixture.start(first);
    start_writer(&store, first_run, "call-a");

    let second_directory = tempfile::tempdir().expect("directory");
    let second_root = RootHandle::enroll(second_directory.path()).expect("root");
    let second = store
        .create_project_conversation(None, "fixture-model")
        .expect("conversation")
        .conversation
        .id;
    store.context(second).expect("context");
    store
        .register_authorization_root(second, 1, &second_root)
        .expect("root");
    let second_run = fixture.start(second);
    start_writer(&store, second_run, "call-b");
}

#[test]
fn overlapping_folder_ancestry_blocks_a_second_writer() {
    let fixture = Fixture::new();
    let store = fixture.database.agent_store();
    let project_conversation = fixture.project_conversation();
    let run = fixture.start(project_conversation);
    start_writer(&store, run, "call-a");

    let subdirectory = fixture._directory.path().join("sub");
    std::fs::create_dir(&subdirectory).expect("subdir");
    let sub_root = RootHandle::enroll(&subdirectory).expect("root");
    let standalone = store
        .create_project_conversation(None, "fixture-model")
        .expect("conversation")
        .conversation
        .id;
    store.context(standalone).expect("context");
    store
        .register_authorization_root(standalone, 1, &sub_root)
        .expect("root");
    let second = fixture.start(standalone);
    let record = propose(&store, second, "call-b");
    let approved = store
        .decide_call(
            &record,
            &json!({"path": "a.txt"}),
            ApprovalDecision::Approved,
        )
        .expect("approval");

    assert!(matches!(
        store.claim_call(&approved, &json!({"path": "a.txt"})),
        Err(AgentStoreError::InvalidTransition)
    ));
}

#[test]
fn a_crashed_run_recovers_as_interrupted_without_replaying() {
    let temporary = tempfile::tempdir().expect("directory");
    let path = temporary.path().join("fixture.sqlite");
    let root = RootHandle::enroll(temporary.path()).expect("root");

    let (conversation, run_id) = {
        let database = Database::open(&path).expect("database");
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
        start_writer(&store, run.id, "call-a");
        (conversation, run.id)
    };

    let database = Database::open(&path).expect("reopen");
    let store = database.agent_store();
    let run = store.run(run_id).expect("run");
    assert_eq!(run.state, RunState::Interrupted);
    let actions = store.history(conversation).expect("history");
    let calls = actions
        .iter()
        .filter(|entry| matches!(entry.content, HistoryContent::ToolCall { .. }))
        .count();
    assert_eq!(calls, 1);
}

#[test]
fn a_cancelled_holder_still_blocks_until_its_writer_is_confirmed() {
    let fixture = Fixture::new();
    let store = fixture.database.agent_store();
    let project = store
        .create_project(&fixture.root, Some("fixture"))
        .expect("project");
    let first = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .expect("first")
        .conversation
        .id;
    let second = store
        .create_project_conversation(Some(project.id), "fixture-model")
        .expect("second")
        .conversation
        .id;
    let run = store.start_run(first, "request").expect("run");
    let action = start_writer(&store, run.id, "call-a");

    let receipt = store.cancel_run(run.id).expect("cancel");
    assert_eq!(receipt.writer_remaining, true);

    let holder = store
        .active_run_for_conversation(second)
        .expect("holder")
        .expect("blocked");
    assert_eq!(holder.run_id, run.id);

    store.confirm_action_terminated(action).expect("confirmed");

    assert_eq!(
        store.active_run_for_conversation(second).expect("holder"),
        None
    );
}

#[test]
fn confirming_termination_releases_the_removal_guard() {
    let fixture = Fixture::new();
    let conversation = fixture.project_conversation();
    let store = fixture.database.agent_store();
    let run = fixture.start(conversation);
    let action = start_writer(&store, run, "call-a");
    store.cancel_run(run).expect("cancel");

    assert_eq!(
        store.advance_context(conversation, 1),
        Err(AgentStoreError::InvalidTransition)
    );
    store.confirm_action_terminated(action).expect("confirmed");
    assert_eq!(store.advance_context(conversation, 1).expect("advance"), 2);
}
