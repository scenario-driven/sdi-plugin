// Mirrors `sdi_core` JSON shapes that the daemon serializes. Kept narrow —
// the daemon is the source of truth, the dashboard adds no extra fields.

export type PlanStatus = 'draft' | 'active' | 'completed';
export type ScenarioStatus = 'proposed' | 'confirmed';
export type ScenarioResult = 'passing' | 'failing' | 'impacted' | 'retired';
export type RoundStatus = 'planning' | 'active' | 'completed';
export type RoundMode = 'strict-regression' | 'forward-only';
export type InFlightPolicy = 'pause' | 'abort' | 'continue-on-noimpact';
export type DisruptionPolicy = 'needs-review' | 'auto';
export type TaskStatus = 'todo' | 'in_progress' | 'blocked' | 'done' | 'cancelled';
/** D12 — append-only ADR status. Mirrors `sdi_core::decision::DecisionStatus`. */
export type DecisionStatus = 'proposed' | 'accepted' | 'superseded';
/** D20 — M3 four-stage negotiation classifier. */
export type DecisionKind = 'proposal' | 'critique' | 'consensus' | 'dissensus';
/** D14 — autonomy modes. L3 strictest (ask), L5 loosest (act-and-notify). */
export type AutonomyMode = 'L3' | 'L4' | 'L5';
/** D14 / D25 — autonomy scope. snake_case to match daemon serialization. */
export type AutonomyScopeKind = 'plan' | 'decision_kind' | 'pattern_kind' | 'global';
/** D22 — kind discriminator for a CollaborationPattern row. */
export type PatternKind = 'workflow' | 'graph' | 'swarm' | 'agents-as-tools' | 'direct';
/** D22 — lifecycle states a CollaborationPattern moves through. */
export type PatternLifecycle = 'pending' | 'active' | 'converged' | 'dissensus' | 'aborted';
/** D22 — which work entity a CollaborationPattern attaches to. */
export type AppliesTo = 'plan' | 'requirement' | 'scenario' | 'task' | 'decision' | 'round';
/** D22 §3.9 — reviewer stance carried on graph-kind patterns (sybil guard). */
export type Stance =
  | 'proposer'
  | 'devil_advocate'
  | 'schema_guardian'
  | 'performance_reviewer'
  | 'security_reviewer'
  | 'neutral';
/** D29 — claim state on a Scenario row. */
export type ClaimStatus = 'none' | 'requested' | 'active' | 'released';
/** D28 — discriminator for the `reversal_plan` JSON envelope. */
export type ReversalType =
  | 'migration_sql'
  | 'git_revert'
  | 'fs_snapshot'
  | 'compensating_action';
/** M1 — blackboard anchor; exactly one of plan/round/scenario/task is set. */
export type AgentNoteScope = 'plan' | 'round' | 'scenario' | 'task';
/** M1 — blackboard semantics. `handoff` requires a `to_agent`. */
export type AgentNoteKind =
  | 'hypothesis'
  | 'observation'
  | 'question'
  | 'dissent'
  | 'evidence'
  | 'handoff';

export interface Project {
  id: string;
  key: string;
  name: string;
  slug: string;
  /** Anchored working directories — entry to "active project" detection. */
  cwds: string[];
  /** Free-form description shown in the project settings drawer. Daemon
   *  omits the field when unset (Option<None>), so callers must treat
   *  missing and empty as the same "no description" state. */
  description?: string;
  /** Soft-disable flag (v0.3). When `false`, the hook layer skips every
   *  mutating gate for the project's anchored cwds — Claude Code drives
   *  the repo without SDI governance until the user re-enables. */
  enabled: boolean;
  /** Wiki tree roots (relative to project cwd, or absolute). Default is a
   *  single `docs` entry; the dashboard's WikiView renders one tree per
   *  path. */
  wiki_paths: string[];
  created_at: string;
  updated_at: string;
}

export interface Plan {
  id: string;
  project_id: string;
  short_code: string;
  title: string;
  body?: string;
  status: PlanStatus;
  approved_at?: string | null;
  completed_at?: string | null;
  created_at: string;
  updated_at: string;
}

export interface Scenario {
  id: string;
  plan_id: string;
  short_code: string;
  given: string;
  when: string;
  then: string;
  status: ScenarioStatus;
  /** D29 — path glob list (JSON-encoded string) of resources this scenario claims. */
  claimed_resources_json?: string | null;
  /** D29 — current claim state on this scenario row. */
  claim_status?: ClaimStatus;
  /** D23 — CollaborationPattern that produced this scenario (NOT NULL post-migration). */
  produced_via_pattern_id?: string | null;
  created_at: string;
  updated_at: string;
}

export interface Round {
  id: string;
  plan_id: string;
  short_code: string;
  status: RoundStatus;
  mode: RoundMode;
  in_flight_policy: InFlightPolicy;
  disruption_policy: DisruptionPolicy;
  activated_at?: string | null;
  completed_at?: string | null;
  created_at: string;
}

export interface ScenarioResultRow {
  round_id: string;
  scenario_id: string;
  result: ScenarioResult;
  note?: string | null;
  evidence_ref?: string | null;
  updated_at: string;
}

export interface ScenarioEvidence {
  scenario_id: string;
  result: ScenarioResult;
  evidence_ref: string;
  note?: string | null;
}

export interface TaskEvidence {
  scenarios: ScenarioEvidence[];
  summary?: string | null;
  extras?: Record<string, unknown> | null;
}

export interface Task {
  id: string;
  round_id: string;
  short_code: string;
  description: string;
  status: TaskStatus;
  parent_scenario_ids: string[];
  parent_requirement_ids: string[];
  evidence?: TaskEvidence | null;
  evidence_at?: string | null;
  created_at: string;
  updated_at: string;
}

export interface Requirement {
  id: string;
  plan_id: string;
  short_code: string;
  body: string;
  created_at: string;
  updated_at: string;
}

export interface Decision {
  id: string;
  plan_id: string;
  short_code: string;
  title: string;
  body: string;
  status: DecisionStatus;
  supersedes_id?: string | null;
  /** D20 — stage in the M3 four-stage flow. */
  kind: DecisionKind;
  /** D20 — set on critique/consensus/dissensus rows; null on proposal. */
  proposal_id?: string | null;
  /** D20 — Layer-2 specialist that emitted this row. */
  agent_name?: string | null;
  /** D20 — set when dissensus surfaces to the human user gate. */
  escalated_at?: string | null;
  /** D28 — JSON envelope of how to revert this decision (one of four type variants). */
  reversal_plan?: ReversalPlan | null;
  /** D28 — static-per-kind blast score (0–10) gating L5 unlock. */
  blast_radius_score?: number | null;
  /** D28 — when this decision is itself a rollback, ref to the original. */
  reversal_of?: string | null;
  /** D23 — CollaborationPattern that produced this decision. */
  produced_via_pattern_id?: string | null;
  created_at: string;
}

/** D14 / D25 / D28 / D29 — AutonomyPolicy row as serialized by `sdid`. */
export interface AutonomyPolicy {
  id: string;
  project_id: string;
  plan_id?: string | null;
  scope_kind: AutonomyScopeKind;
  decision_kind?: string | null;
  /** D25 — populated when `scope_kind === 'pattern_kind'`. */
  pattern_kind?: PatternKind | null;
  mode: AutonomyMode;
  /** D28 — blast_radius_score upper bound for L5 auto-apply (0–10, default 5). */
  l5_threshold?: number;
  /** D24 — max CollaborationPattern.depth (1–10, default 3). */
  pattern_depth_cap?: number;
  /** D29 — when true, a plan permits only one session to hold active claims. */
  plan_single_session_lock?: boolean;
  set_at: string;
  set_by: string;
  reason?: string | null;
  created_at: string;
  updated_at: string;
}

/** D22 §3.9 — CollaborationPattern row as serialized by `sdid`. */
export interface CollaborationPattern {
  id: string;
  short_code: string;
  plan_id: string;
  kind: PatternKind;
  applies_to: AppliesTo;
  scope_id: string;
  parent_pattern_id: string | null;
  depth: number;
  lifecycle: PatternLifecycle;
  /** workflow.kind shape: ordered step list (D26 shape gate: len ≥ 2). */
  steps?: Array<{ idx: number; agent: string; action: string }>;
  /** graph.kind shape: (name, stance) reviewers manifest (D26 shape gate: distinct ≥ 2). */
  reviewers?: Array<{ name: string; stance: Stance }>;
  /** swarm.kind shape: spawn target list (D26 shape gate: len ≥ 2). */
  fan_out?: string[];
  /** agents-as-tools.kind shape: caller→callee registration (D26 shape gate: len ≥ 1). */
  peer_registration?: Array<{ caller: string; callee: string }>;
  decided_at?: string;
  decided_reason?: string;
  created_at: string;
  updated_at: string;
}

/** D28 — reversal plan envelope. Exactly one variant per `type`. */
export interface ReversalPlan {
  type: ReversalType;
  /** `migration_sql` variant. */
  sql?: string;
  /** `migration_sql` variant — ordered upstream migration deps. */
  dependencies?: string[];
  /** `git_revert` variant — commit SHA to revert. */
  sha?: string;
  /** `fs_snapshot` variant — backend-specific snapshot identifier. */
  snapshot_ref?: string;
  /** `compensating_action` variant — free-form action description. */
  action_spec?: string;
}

/** D29 — per-scenario claim ledger row returned by `/scenarios/active-claims`. */
export interface ScenarioClaim {
  scenario_id: string;
  short_code: string;
  claimed_resources: string[];
  claim_status: ClaimStatus;
  holder_session_id?: string;
}

/** M1 — AgentNote row as serialized by `sdid`. */
export interface AgentNote {
  id: string;
  project_id: string;
  plan_id?: string | null;
  round_id?: string | null;
  scenario_id?: string | null;
  task_id?: string | null;
  scope_kind: AgentNoteScope;
  kind: AgentNoteKind;
  from_agent: string;
  to_agent?: string | null;
  body: string;
  payload: Record<string, unknown>;
  receipt_acknowledged_at?: string | null;
  retired_at?: string | null;
  retired_reason?: string | null;
  created_at: string;
}

export interface KnowledgeEntry {
  id: string;
  scope: 'rag' | 'reference' | 'archive';
  kind: string;
  title: string;
  body: string;
  tags: string[];
  created_at: string;
  updated_at: string;
}

export interface DisruptionReview {
  id: string;
  plan_id: string;
  source_kind: 'scenario' | 'requirement' | 'decision';
  source_id: string;
  impacted_scenario_ids: string[];
  status: 'pending' | 'resolved';
  resolution?: 'keep' | 'retire' | 'edit' | null;
  note?: string | null;
  created_at: string;
  resolved_at?: string | null;
}

export type EventKind =
  | 'plan.created'
  | 'plan.updated'
  | 'plan.approved'
  | 'plan.completed'
  | 'scenario.created'
  | 'scenario.updated'
  | 'scenario.confirmed'
  | 'round.created'
  | 'round.activated'
  | 'round.completed'
  | 'round.result.updated'
  | 'task.created'
  | 'task.updated'
  | 'task.completed'
  | 'requirement.created'
  | 'requirement.updated'
  | 'decision.created'
  | 'decision.superseded'
  | 'knowledge.created'
  | 'knowledge.updated'
  | 'knowledge.deleted'
  | 'disruption.created'
  | 'disruption.resolved'
  | 'autonomy.changed'
  | 'circuit_breaker.triggered'
  | 'agent_note.created'
  | 'agent_note.acknowledged'
  | 'agent_note.retired'
  | 'consensus.reached'
  | 'dissensus.escalated'
  | 'decision.status-changed'
  | 'pattern_created'
  | 'pattern_lifecycle'
  | 'pattern_aborted'
  | 'claim_status_changed'
  | 'rollback_initiated'
  | 'rollback_completed';

export interface SdiEvent<T = unknown> {
  kind: EventKind | string;
  entity_id?: string;
  payload: T;
}
