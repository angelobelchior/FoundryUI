#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextKey {
    pub user_id: String,
    pub conversation_id: i64,
    pub context_revision: i64,
    pub authorization_revision: i64,
    pub mode_revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionKey {
    pub context: ContextKey,
    pub run_id: i64,
    pub action_id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resource {
    pub root_id: String,
    pub object_id: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileOperation {
    Read,
    Create,
    Edit,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    File(FileOperation),
    Command {
        executable: String,
        argv: Vec<String>,
        working_directory: Resource,
        environment: Vec<(String, String)>,
    },
    Network {
        method: String,
        destination: String,
    },
    RemoteInference,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstructionOrigin {
    User,
    Model,
    ProjectFile,
    ApprovedInstructionFile,
    ToolOutput,
    AttachedConversation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    pub key: ActionKey,
    pub operation: Operation,
    pub arguments_fingerprint: String,
    pub resources: Vec<Resource>,
    pub origin: InstructionOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effects {
    ReadOnly,
    Alteration,
    Destructive,
    Unbounded,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadEvidence {
    ProvenNoReads,
    ProvenReads { resources: Vec<Resource> },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkControl {
    Disabled,
    Controlled {
        destinations: Vec<String>,
        audit_ready: bool,
    },
    Opaque,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Capability {
    MissingImplementation,
    Unproven,
    Rejected,
    Proven {
        effects: Effects,
        opened_resources_validated: bool,
        process_isolation: bool,
        network: NetworkControl,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceScope {
    Project,
    ExternalProject,
    Standalone,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedResource {
    pub resource: Resource,
    pub scope: ResourceScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    NotGuaranteed,
    Protected { protection_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requirement {
    Operation,
    StandaloneRoot { root_id: String },
    ExternalResource { resource: Resource },
    DestructiveConfirmation { recovery: Recovery },
    AlterationReview { change_id: String },
    NetworkDestination { destination: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Remember {
    Never,
    Conversation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    pub requirement: Requirement,
    pub remember: Remember,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionApproval {
    pub action: Action,
    pub requirement: Requirement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrantKind {
    Operation {
        operation: Operation,
        arguments_fingerprint: String,
        resources: Vec<Resource>,
    },
    StandaloneRoot {
        root_id: String,
    },
    NetworkDestination {
        destination: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationGrant {
    pub context: ContextKey,
    pub kind: GrantKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedFacts {
    pub action: Action,
    pub current_key: ActionKey,
    pub mode: super::AgentMode,
    pub mode_at_proposal: super::AgentMode,
    pub project_root_id: Option<String>,
    pub global_read_allowed: bool,
    pub read_evidence: ReadEvidence,
    pub capability: Capability,
    pub resources: Vec<ResolvedResource>,
    pub concrete_change_id: Option<String>,
    pub recovery: Recovery,
    pub approvals: Vec<ActionApproval>,
    pub denied_actions: Vec<Action>,
    pub grants: Vec<ConversationGrant>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Denial {
    UserDenied,
    StaleContext,
    ActionFactsMismatch,
    InvalidAction,
    ProductionImplementationMissing,
    CapabilityUnproven,
    CapabilityRejected,
    UnboundedEffects,
    ResourceValidationMissing,
    ResourceScopeMismatch,
    ReadRestricted,
    ReadEvidenceUnproven,
    ProcessIsolationMissing,
    NetworkNotAuditable,
    NetworkScopeMismatch,
    ConcreteReviewMissing,
    RemoteInferenceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Ask(Vec<Prompt>),
    Deny(Denial),
}
