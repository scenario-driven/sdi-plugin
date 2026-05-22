//! SDI core domain model.

pub mod ids;
pub mod error;
pub mod project;
pub mod plan;
pub mod requirement;
pub mod decision;
pub mod scenario;
pub mod round;
pub mod task;
pub mod knowledge;
pub mod disruption;
pub mod collab;
pub mod run;
pub mod usage;
pub mod autonomy_policy;
pub mod agent_note;
pub mod agent_spec;

pub use error::{DomainError, DomainResult};
pub use ids::{Id, IdKind, Timestamp, new_ulid_id, now};
pub use project::{Project, ProjectId};
pub use plan::{Plan, PlanId, PlanStatus};
pub use requirement::{Requirement, RequirementId};
pub use decision::{Decision, DecisionId, DecisionKind, DecisionStatus};
pub use scenario::{Scenario, ScenarioId, ScenarioStatus, ScenarioResult};
pub use round::{Round, RoundId, RoundStatus, RoundMode, InFlightPolicy, DisruptionPolicy};
pub use task::{Task, TaskId, TaskStatus, TaskEvidence, ScenarioEvidence};
pub use knowledge::{Knowledge, KnowledgeId, KnowledgeScope};
pub use disruption::{
    DisruptionResolution, DisruptionReview, DisruptionReviewId, DisruptionReviewStatus,
    DisruptionSource,
};
pub use autonomy_policy::{
    AutonomyMode, AutonomyPolicy, AutonomyPolicyId, AutonomyScopeKind,
};
pub use agent_note::{AgentNote, AgentNoteId, AgentNoteKind, AgentNoteScope};
pub use agent_spec::{AgentSpec, AgentSpecId, STOCK_AGENTS};
