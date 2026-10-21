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

    // ─── verification-run integrity ──────────────────────────────────────
    #[error(
        "retry budget exhausted for scenario {scenario}: {count}/{cap} {class} failure(s) with signature {signature}. \
         Stop retrying — escalate by opening a decision (`sdi decision create <PLAN> <SHORT> \"<title>\" --body …`) \
         and release the budget with `sdi run release-budget --scenario {scenario} --decision <DEC-ID>`. \
         Cancelling, recreating or renaming tasks does not reset it"
    )]
    RetryBudgetExhausted {
        scenario: String,
        class: String,
        signature: String,
        count: i64,
        cap: i64,
    },

    #[error("run evidence rejected: {0}")]
    EvidenceRunInvalid(String),

    #[error("run evidence is stale: {0}")]
    EvidenceStale(String),

    #[error("round baseline is locked: {0}")]
    BaselineLocked(String),

    #[error("round completion blocked by unresolved harness/environment failures: {0}")]
    HarnessFailureUnresolved(String),
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
            DomainError::RetryBudgetExhausted { .. } => "RETRY_BUDGET_EXHAUSTED",
            DomainError::EvidenceRunInvalid(_) => "EVIDENCE_RUN_INVALID",
            DomainError::EvidenceStale(_) => "EVIDENCE_STALE",
            DomainError::BaselineLocked(_) => "BASELINE_LOCKED",
            DomainError::HarnessFailureUnresolved(_) => "HARNESS_FAILURE_UNRESOLVED",
        }
    }

    pub fn http_status(&self) -> u16 {
        match self {
            DomainError::NotFound(_) => 404,
            DomainError::Conflict(_) => 409,
            DomainError::DissensusEscalated => 409,
            DomainError::AutonomyGateBlocked { .. } => 403,
            DomainError::CircuitBreakerActive => 423,
            DomainError::RetryBudgetExhausted { .. } => 409,
            DomainError::EvidenceStale(_) => 409,
            DomainError::BaselineLocked(_) => 409,
            DomainError::HarnessFailureUnresolved(_) => 409,
            _ => 400,
        }
    }
}
