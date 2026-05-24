use thiserror::Error;

#[derive(Debug, Error)]
pub enum DomainError {
    #[error("invalid state transition: {entity} {from} -> {to}")]
    InvalidTransition {
        entity: String,
        from: String,
        to: String,
    },

    #[error("validation error: {0}")]
    Validation(String),

    #[error("entity not found: {0}")]
    NotFound(String),

    #[error("plan must be active before this operation: {0}")]
    PlanNotActive(String),

    #[error("scenarios required: a plan needs at least one confirmed scenario before approval")]
    ScenariosRequired,

    #[error("GWT field empty: {field}")]
    GwtEmpty { field: &'static str },

    #[error("evidence required for done transition")]
    EvidenceRequired,

    #[error("round mode invalid: {0}")]
    InvalidRoundMode(String),

    #[error("path invariant violated (LM-8): {0}")]
    PathInvariant(String),

    #[error("disruption pending — needs review")]
    DisruptionPending,

    #[error("in-flight task pause required")]
    InFlightTaskPause,

    #[error("snapshot-only violation: {0} (move history to a Decision via /decide)")]
    SnapshotViolation(String),

    #[error("conflict: {0}")]
    Conflict(String),

    // ─── v0.4 multi-agent governance ─────────────────────────────────────
    #[error("consensus stage requires ≥ 1 prior critique against proposal {proposal_id}")]
    ConsensusMissingCritique { proposal_id: String },

    #[error("dissensus escalated to human gate (mode-agnostic, per D20)")]
    DissensusEscalated,

    #[error("autonomy gate blocked: decision-kind {kind} requires mode {required} but policy is {actual}")]
    AutonomyGateBlocked {
        kind: String,
        required: String,
        actual: String,
    },

    #[error("circuit breaker active — all autonomy modes demoted to L3; explicit user approval required")]
    CircuitBreakerActive,
}

pub type DomainResult<T> = Result<T, DomainError>;

impl DomainError {
    pub fn code(&self) -> &'static str {
        match self {
            DomainError::InvalidTransition { .. } => "INVALID_TRANSITION",
            DomainError::Validation(_) => "VALIDATION",
            DomainError::NotFound(_) => "NOT_FOUND",
            DomainError::PlanNotActive(_) => "PLAN_NOT_ACTIVE",
            DomainError::ScenariosRequired => "SCENARIOS_REQUIRED",
            DomainError::GwtEmpty { .. } => "GWT_EMPTY",
            DomainError::EvidenceRequired => "EVIDENCE_REQUIRED",
            DomainError::InvalidRoundMode(_) => "INVALID_ROUND_MODE",
            DomainError::PathInvariant(_) => "PATH_INVARIANT_VIOLATION",
            DomainError::DisruptionPending => "DISRUPTION_PENDING",
            DomainError::InFlightTaskPause => "IN_FLIGHT_TASK_PAUSE",
            DomainError::SnapshotViolation(_) => "SNAPSHOT_VIOLATION",
            DomainError::Conflict(_) => "CONFLICT",
            DomainError::ConsensusMissingCritique { .. } => "CONSENSUS_MISSING_CRITIQUE",
            DomainError::DissensusEscalated => "DISSENSUS_ESCALATED",
            DomainError::AutonomyGateBlocked { .. } => "AUTONOMY_GATE_BLOCKED",
            DomainError::CircuitBreakerActive => "CIRCUIT_BREAKER_ACTIVE",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            DomainError::NotFound(_) => 404,
            DomainError::Conflict(_) => 409,
            DomainError::DissensusEscalated => 409,
            DomainError::AutonomyGateBlocked { .. } => 403,
            DomainError::CircuitBreakerActive => 423,
            _ => 400,
        }
    }
}
