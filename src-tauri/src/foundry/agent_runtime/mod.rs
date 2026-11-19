mod approvals;
mod change_tools;
mod command_tools;
mod composite;
mod context_search;
mod cycle;
mod instructions;
mod network_tools;
mod parser;
mod provider;
mod read_tools;
mod schema;

use crate::foundry::db::agent::AgentStore;
pub use approvals::PersistedApprovals;
pub use instructions::with_approved_instructions;

pub use cycle::{
    ApprovalChoice, ApprovalProvider, ChatOnlyReason, CycleDeps, CycleEvent, CycleEventKind,
    CycleFailure, CycleLimits, CycleOutcome, CycleRequest, CycleSink, ModelRequest, ModelTurn,
    StopReason, SuspendApprovals, ToolAuthorizer, ToolExecution, ToolExecutor, ToolHost,
    TurnProvider, UnimplementedTools, run_cycle,
};
pub use parser::{
    FinishReason, MAX_ARGUMENT_BYTES, MAX_CALLS, MAX_ID_BYTES, MAX_NAME_BYTES, MAX_WIRE_BYTES,
    ParsedTurn, ToolCall, ToolCallStream, ToolStreamError,
};
pub use provider::EndpointTurnProvider;
pub use schema::{
    ArgumentError, RegisteredTool, ToolAvailability, ToolRegistry, ToolSpec, ToolSpecError,
    ToolUnavailable,
};

pub use change_tools::{ChangeAuthorizer, ChangeExecutor};
pub use command_tools::CommandAuthorizer;
pub use composite::{ToolAuthorizerSet, ToolExecutorSet};
pub use context_search::{SearchAuthorizer, SearchExecutor};
pub use network_tools::{NetworkAuthorizer, NetworkExecutor};
pub use read_tools::{ReadAuthorizer, ReadExecutor};

pub fn register_agent_tools(registry: &mut ToolRegistry) -> Result<(), ToolSpecError> {
    read_tools::register(registry)?;
    context_search::register(registry)?;
    change_tools::register(registry)?;
    command_tools::register(registry)?;
    network_tools::register(registry)
}

pub async fn run_agent_cycle(
    store: &AgentStore<'_>,
    mut request: CycleRequest,
    provider: &mut dyn TurnProvider,
    sink: &dyn CycleSink,
) -> CycleOutcome {
    let mut registry = ToolRegistry::default();
    if let Err(error) = register_agent_tools(&mut registry) {
        return CycleOutcome::Failed {
            code: "registry_failed".into(),
            detail: error.to_string(),
        };
    }
    let mut executor = ToolExecutorSet;
    let authorizer = ToolAuthorizerSet;
    let mut approvals = PersistedApprovals;
    let limits = CycleLimits::default();
    request.messages.insert(
        0,
        serde_json::json!({"role": "system", "content": instructions::GUIDANCE_POLICY}),
    );
    let mut provider = instructions::InstructionProvider {
        store,
        conversation_id: request.conversation_id,
        provider,
        max_input_bytes: limits.max_input_bytes,
        targets: Vec::new(),
    };
    run_cycle(
        store,
        CycleDeps {
            registry: &registry,
            provider: &mut provider,
            executor: &mut executor,
            authorizer: &authorizer,
            approvals: &mut approvals,
            limits: &limits,
            sink,
        },
        request,
    )
    .await
}

#[cfg(test)]
mod tests;
