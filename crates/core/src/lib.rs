//! SDI core domain model.

pub mod agent_note;
pub mod agent_spec;
pub mod autonomy_policy;
pub mod collab;
pub mod decision;
pub mod disruption;
pub mod error;
pub mod ids;
pub mod knowledge;
pub mod pattern;
pub mod plan;
pub mod project;
pub mod requirement;
pub mod round;
pub mod run;
pub mod scenario;
pub mod task;
pub mod usage;

pub use agent_note::{AgentNote, AgentNoteId, AgentNoteKind, AgentNoteScope};
pub use agent_spec::{AgentSpec, AgentSpecId, AgentSpecStatus, STOCK_AGENTS, STOCK_META_AGENTS};
pub use autonomy_policy::{AutonomyMode, AutonomyPolicy, AutonomyPolicyId, AutonomyScopeKind};
pub use decision::{Decision, DecisionId, DecisionKind, DecisionStatus};
pub use disruption::{
    DisruptionResolution, DisruptionReview, DisruptionReviewId, DisruptionReviewStatus,
    DisruptionSource,
};
pub use error::{DomainError, DomainResult};
pub use ids::{new_ulid_id, now, Id, IdKind, Timestamp};
pub use knowledge::{Knowledge, KnowledgeId, KnowledgeScope};
pub use pattern::{
    validate_pattern_shape, validate_reversal_plan_json, AppliesTo, ClaimStatus,
    CollaborationPattern, CollaborationPatternId, PatternKind, PatternLifecycle, PatternStep,
    PeerLink, ReversalPlan, Reviewer, Stance,
};
pub use plan::{Plan, PlanId, PlanStatus};
pub use project::{Project, ProjectId};
pub use requirement::{Requirement, RequirementId};
pub use round::{DisruptionPolicy, InFlightPolicy, Round, RoundId, RoundMode, RoundStatus};
pub use scenario::{Scenario, ScenarioId, ScenarioResult, ScenarioStatus};
pub use task::{ScenarioEvidence, Task, TaskEvidence, TaskId, TaskStatus};
