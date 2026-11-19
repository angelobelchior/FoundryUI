mod command_tools;
mod cycle;
mod network_tools;
mod parser;
mod provider;
mod read_tools;
mod schema;

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

pub use command_tools::CommandAuthorizer;
pub use network_tools::{NetworkAuthorizer, NetworkExecutor};
pub use read_tools::{ReadAuthorizer, ReadExecutor};

pub fn register_agent_tools(registry: &mut ToolRegistry) -> Result<(), ToolSpecError> {
    read_tools::register(registry)?;
    command_tools::register(registry)?;
    network_tools::register(registry)
}

#[cfg(test)]
mod tests;
