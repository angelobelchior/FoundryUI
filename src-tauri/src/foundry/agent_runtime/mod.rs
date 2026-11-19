mod change_tools;
mod command_tools;
mod composite;
mod context_search;
mod cycle;
mod network_tools;
mod parser;
mod provider;
mod read_tools;
mod schema;

use crate::foundry::db::agent::AgentStore;

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
    request: CycleRequest,
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
    let mut approvals = SuspendApprovals;
    let limits = CycleLimits::default();
    run_cycle(
        store,
        CycleDeps {
            registry: &registry,
            provider,
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
