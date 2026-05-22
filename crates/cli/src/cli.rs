//! Top-level clap App for `sdi`.
//!
//! Subcommand families (PRD §5.4 / §6):
//! - daemon   — lifecycle (start/stop/status)
//! - doctor   — environment diagnostics
//! - project  — multi-project root (CRUD + cwd attach)
//! - plan     — D1/D8 plan lifecycle
//! - req      — D12 SNAPSHOT requirement editor
//! - scenario — D5 GWT-strict scenarios
//! - round    — D6 round/regression
//! - task     — D3 runtime tasks + D7 evidence gate
//! - decision — D12 append-only ADR log
//! - knowledge — RAG / reference / archive scopes
//! - autonomy — D14/D17/D18 per-scope policy (L3 / L4 / L5)
//! - agent-note — M1 blackboard + M2 hand-off receipts
//! - consensus — M3 4-stage negotiation status (proposal/critique/consensus/dissensus)
//! - mcp      — stdio MCP server (PRD §5.4)

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "sdi",
    version,
    about = "Scenario-Driven Implementation — LLM-era successor to TDD/BDD."
)]
pub struct App {
    /// Output format hint (json | text). JSON is the default for entity-emitting
    /// commands; text is a future affordance reserved for tables/summaries.
    #[arg(long, global = true, default_value = "json")]
    pub format: String,

    /// Quiet mode: entity-emitting commands print only the id.
    #[arg(long, short = 'q', global = true, default_value_t = false)]
    pub quiet: bool,

    #[command(subcommand)]
    pub cmd: Cmd,
}

#[derive(Debug, Subcommand)]
pub enum Cmd {
    /// Daemon lifecycle (`start` / `stop` / `status`).
    #[command(subcommand)]
    Daemon(DaemonCmd),
    /// Environment diagnostics (paths, LM-8, db, daemon liveness).
    Doctor,
    /// Project: multi-project root entity.
    #[command(subcommand)]
    Project(ProjectCmd),
    /// Plan: D1/D8 lifecycle (draft → active → completed).
    #[command(subcommand)]
    Plan(PlanCmd),
    /// Requirement: D12 SNAPSHOT (overwrite-in-place, no versioning).
    #[command(subcommand)]
    Req(RequirementCmd),
    /// Scenario: D5 GWT-strict acceptance criteria.
    #[command(subcommand)]
    Scenario(ScenarioCmd),
    /// Round: D6 regression-defaulting iteration (R1, R2…).
    #[command(subcommand)]
    Round(RoundCmd),
    /// Task: D3 runtime artifact, requires evidence on `done` (PRD §6.6).
    #[command(subcommand)]
    Task(TaskCmd),
    /// Decision: D12 append-only ADR log with supersession chain.
    #[command(subcommand)]
    Decision(DecisionCmd),
    /// Knowledge: rag / reference / archive scopes (PRD §5.4).
    #[command(subcommand)]
    Knowledge(KnowledgeCmd),
    /// Comment: polymorphic anchor (plan / task / scenario / round).
    #[command(subcommand)]
    Comment(CommentCmd),
    /// Question: open/answered Q&A (anchored to plan).
    #[command(subcommand)]
    Question(QuestionCmd),
    /// Run: task execution attempt (start / finish / list).
    #[command(subcommand)]
    Run(RunCmd),
    /// Usage: token/tool-call accounting + preflight estimate.
    #[command(subcommand)]
    Usage(UsageCmd),
    /// Aggregate dashboard (active plan + counts + recent activity).
    Dashboard(DashboardArgs),
    /// Project handoff bundle (session pickup).
    Handoff { project_id: String },
    /// Activity timeline for a project.
    Timeline(TimelineArgs),
    /// Project-scoped board view (in-flight + backlog).
    Board(BoardArgs),
    /// Project-scoped wiki view (rag-scoped knowledge).
    Wiki(WikiArgs),
    /// Project summary (counts + active plan).
    Summary(SummaryArgs),
    /// Prometheus metrics dump.
    Metrics,
    /// Replay events from the durable log.
    Replay(ReplayArgs),
    /// JSON import/export of plan-scoped state.
    #[command(subcommand)]
    Export(ExportCmd),
    /// JSON import (matching `export` shape).
    #[command(subcommand)]
    Import(ImportCmd),
    /// Autonomy: D14/D17/D18 per-scope L3/L4/L5 policy.
    #[command(subcommand)]
    Autonomy(AutonomyCmd),
    /// AgentNote: M1 blackboard + M2 hand-off receipts.
    #[command(subcommand, name = "agent-note")]
    AgentNote(AgentNoteCmd),
    /// Consensus: M3 4-stage negotiation status (D20).
    #[command(subcommand)]
    Consensus(ConsensusCmd),
    /// MCP server (stdio JSON-RPC, exposes scope=rag only).
    Mcp,
    /// Initialise a project anchored to the current cwd (idempotent).
    Init(InitArgs),
    /// Backup the SQLite DB to a target path.
    Backup { output: String },
    /// Restore the SQLite DB from a backup path (destructive).
    Restore { input: String },
    /// Print effective config (paths + daemon liveness).
    Config,
    /// Tail the daemon's log file.
    Log(LogArgs),
    /// Watch the SSE stream and print events.
    Watch(WatchArgs),
    /// Emit shell completion script for the given shell.
    Completions { shell: String },
}

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Project key (2–8 uppercase). Defaults to slugified cwd basename.
    #[arg(long)]
    pub key: Option<String>,
    /// Display name. Defaults to cwd basename.
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Debug, Args)]
pub struct LogArgs {
    /// Follow (tail -f). Default: print the tail and exit.
    #[arg(long, short = 'f')]
    pub follow: bool,
    /// Number of lines to print at startup.
    #[arg(long, short = 'n', default_value_t = 80)]
    pub lines: usize,
}

#[derive(Debug, Args)]
pub struct WatchArgs {
    /// Filter event kind by prefix (e.g. "task." for all task.* events).
    #[arg(long)]
    pub kind: Option<String>,
    /// Stop after N events. 0 = unbounded.
    #[arg(long, default_value_t = 0)]
    pub count: usize,
}

#[derive(Debug, Subcommand)]
pub enum DaemonCmd {
    /// Start the daemon (detached). Idempotent if already running.
    Start,
    /// Stop the daemon (SIGTERM, waits up to 5s).
    Stop,
    /// Report running / pid / port.
    Status,
}

#[derive(Debug, Subcommand)]
pub enum ProjectCmd {
    /// Create a project.
    Create(ProjectCreateArgs),
    /// List all projects.
    List,
    /// Show a project by id.
    View { id: String },
    /// Show the project owning a given cwd.
    ByCwd { cwd: String },
    /// Update a project's name. Slug and key are immutable identifiers — to
    /// change them, create a new project.
    Update(ProjectUpdateArgs),
    /// Attach a working-directory to a project.
    CwdAttach { project_id: String, cwd: String },
    /// Detach a working-directory.
    CwdDetach { project_id: String, cwd: String },
}

#[derive(Debug, Args)]
pub struct ProjectCreateArgs {
    /// Short stable key (e.g. "SDI"). 2–8 uppercase chars.
    pub key: String,
    /// Display name.
    pub name: String,
    /// Slug (kebab-case). If omitted, derived from name.
    #[arg(long)]
    pub slug: Option<String>,
    /// Working directories to seed (`--cwd /abs/path`, repeatable).
    #[arg(long = "cwd")]
    pub cwds: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ProjectUpdateArgs {
    pub id: String,
    #[arg(long)]
    pub name: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum PlanCmd {
    /// Create a plan (status starts at `draft`).
    Create(PlanCreateArgs),
    /// List plans for a project.
    List { project_id: String },
    /// Show a plan by id.
    View { id: String },
    /// Replace title/body of a plan in place (SNAPSHOT-style).
    Update(PlanUpdateArgs),
    /// Approve a plan (D8: requires ≥1 confirmed scenario).
    Approve { id: String },
    /// Complete a plan.
    Complete { id: String },
    /// Show the active plan for a project (404 if none).
    Active { project_id: String },
    /// Composite snapshot: plan + scenarios + in-flight tasks + decisions.
    /// Mirrors the MCP `get_plan_context` tool (PRD §5.4).
    Context { id: String },
}

#[derive(Debug, Args)]
pub struct PlanCreateArgs {
    pub project_id: String,
    /// Short code (e.g. "SDI-1").
    pub short_code: String,
    /// Title.
    pub title: String,
    /// Body markdown. Use "-" to read from stdin.
    #[arg(long, default_value = "")]
    pub body: String,
}

#[derive(Debug, Args)]
pub struct PlanUpdateArgs {
    pub id: String,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub body: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum RequirementCmd {
    /// Create a requirement under a plan.
    Create(RequirementCreateArgs),
    /// List requirements for a plan.
    List { plan_id: String },
    /// Show a requirement by id.
    View { id: String },
    /// Overwrite a requirement in place (SNAPSHOT semantics, D12).
    Update(RequirementUpdateArgs),
    /// Delete a requirement.
    Delete { id: String },
}

#[derive(Debug, Args)]
pub struct RequirementCreateArgs {
    pub plan_id: String,
    pub short_code: String,
    pub title: String,
    #[arg(long, default_value = "")]
    pub body: String,
    /// Free-form source reference (file:line, ticket URL, …).
    #[arg(long)]
    pub source: Option<String>,
}

#[derive(Debug, Args)]
pub struct RequirementUpdateArgs {
    pub id: String,
    #[arg(long)]
    pub title: Option<String>,
    #[arg(long)]
    pub body: Option<String>,
    #[arg(long)]
    pub source: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ScenarioCmd {
    /// Create a scenario (D5 GWT-strict).
    Create(ScenarioCreateArgs),
    /// List scenarios for a plan.
    List { plan_id: String },
    /// Show a scenario by id.
    View { id: String },
    /// Replace given/when/then in place (SNAPSHOT).
    Update(ScenarioUpdateArgs),
    /// Mark a scenario as confirmed (advances the D8 approve gate).
    Confirm { id: String },
    /// FTS5 search across one plan's scenarios. Mirrors the MCP
    /// `search_scenarios` tool (PRD §5.4).
    Search(ScenarioSearchArgs),
}

#[derive(Debug, Args)]
pub struct ScenarioSearchArgs {
    pub plan_id: String,
    /// FTS5 MATCH expression. Quote multi-word queries.
    pub query: String,
    /// Cap on results (max 50).
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct ScenarioCreateArgs {
    pub plan_id: String,
    pub short_code: String,
    #[arg(long)]
    pub given: String,
    #[arg(long, name = "when")]
    pub when_: String,
    #[arg(long, name = "then")]
    pub then_: String,
    /// Mark as confirmed immediately (default: draft).
    #[arg(long)]
    pub confirmed: bool,
}

#[derive(Debug, Args)]
pub struct ScenarioUpdateArgs {
    pub id: String,
    #[arg(long)]
    pub given: Option<String>,
    #[arg(long, name = "when")]
    pub when_: Option<String>,
    #[arg(long, name = "then")]
    pub then_: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum RoundCmd {
    /// Create a round (defaults: strict-regression, needs-review, pause-in-flight).
    Create(RoundCreateArgs),
    /// List rounds for a plan.
    List { plan_id: String },
    /// Show a round by id.
    View { id: String },
    /// Activate a round (R2+ auto-carries verdicts under strict-regression).
    Activate { id: String },
    /// Complete a round.
    Complete { id: String },
    /// Record a per-scenario verdict.
    Result(RoundResultArgs),
    /// List all results for a round.
    Results { id: String },
    /// Show the active round for a plan.
    Active { plan_id: String },
}

#[derive(Debug, Args)]
pub struct RoundCreateArgs {
    pub plan_id: String,
    pub short_code: String,
    /// Mode: strict-regression (default) | additive | disruption (D6).
    #[arg(long)]
    pub mode: Option<String>,
    /// In-flight policy: pause (default) | abort | continue-on-noimpact (D10).
    #[arg(long)]
    pub in_flight: Option<String>,
    /// Disruption policy: needs-review (default) | auto (D9). `auto` only
    /// changes how the LLM proposes resolutions; human-confirm is universal.
    #[arg(long)]
    pub disruption: Option<String>,
}

#[derive(Debug, Args)]
pub struct RoundResultArgs {
    /// Round id.
    pub id: String,
    /// Scenario id.
    #[arg(long)]
    pub scenario: String,
    /// Result: passing | failing | impacted | retired (matches ScenarioResult).
    #[arg(long)]
    pub result: String,
    /// Evidence reference (file:line, log path, etc.).
    #[arg(long)]
    pub evidence: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum TaskCmd {
    /// Create a task tied to a round.
    Create(TaskCreateArgs),
    /// List tasks for a round.
    List { round_id: String },
    /// Show a task by id.
    View { id: String },
    /// Pick up a task (todo → in_progress).
    Start { id: String },
    /// Block on external dependency.
    Block { id: String },
    /// Cancel a task.
    Cancel { id: String },
    /// Complete a task with evidence (PRD §6.6 — `--evidence` mandatory).
    Complete(TaskCompleteArgs),
    /// Decompose a task into subtasks (parent relations created automatically).
    Decompose(TaskDecomposeArgs),
    /// Show ancestor chain.
    Ancestors { id: String },
    /// Show descendant chain.
    Descendants { id: String },
    /// Show subtree (root + descendants).
    Subtree { id: String },
    /// List non-parent relations on a task.
    Relations { id: String },
    /// Add a non-parent relation.
    Relate(TaskRelateArgs),
    /// Remove a relation by id.
    Unrelate { relation_id: String },
    /// Acquire a lease (single-writer mutex).
    Lease(TaskLeaseArgs),
    /// Heartbeat an existing lease.
    Heartbeat(TaskLeaseArgs),
    /// Release a lease.
    Release(TaskLeaseReleaseArgs),
    /// Show current lease (if any).
    LeaseInfo { id: String },
    /// Status histogram across all tasks.
    Stats,
    /// Preflight: estimate cost for a task (uses tier history).
    Preflight(TaskPreflightArgs),
}

#[derive(Debug, Args)]
pub struct TaskDecomposeArgs {
    /// Parent task id.
    pub id: String,
    /// Subtask spec: `SHORTCODE::description` (repeatable).
    #[arg(long = "subtask", required = true)]
    pub subtasks: Vec<String>,
    /// Optional round id override (default: parent's round).
    #[arg(long)]
    pub round_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct TaskRelateArgs {
    /// Parent task id (the relation's "from" side).
    pub id: String,
    /// Child task id.
    #[arg(long)]
    pub child: String,
    /// blocks | depends-on | duplicates | related (default: related).
    #[arg(long, default_value = "related")]
    pub kind: String,
}

#[derive(Debug, Args)]
pub struct TaskLeaseArgs {
    pub id: String,
    #[arg(long)]
    pub holder: String,
    #[arg(long, default_value_t = 120)]
    pub ttl_seconds: i64,
}

#[derive(Debug, Args)]
pub struct TaskLeaseReleaseArgs {
    pub id: String,
    #[arg(long)]
    pub holder: String,
}

#[derive(Debug, Args)]
pub struct TaskPreflightArgs {
    pub id: String,
    /// Override the task's own tier (low | med | high).
    #[arg(long)]
    pub tier: Option<String>,
}

#[derive(Debug, Args)]
pub struct TaskCreateArgs {
    pub round_id: String,
    pub short_code: String,
    pub description: String,
    /// Parent scenario ids (repeatable). Required to evidence at completion.
    #[arg(long = "scenario")]
    pub scenarios: Vec<String>,
    /// Parent requirement ids (repeatable, optional).
    #[arg(long = "req")]
    pub requirements: Vec<String>,
}

#[derive(Debug, Args)]
pub struct TaskCompleteArgs {
    pub id: String,
    /// Scenario verdict: `--evidence SCN-…=passing@file:line` (repeatable).
    #[arg(long = "evidence", required = true)]
    pub evidence: Vec<String>,
    /// Optional summary line.
    #[arg(long)]
    pub summary: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum DecisionCmd {
    /// Capture an ADR (defaults status=accepted).
    Create(DecisionCreateArgs),
    /// List decisions for a plan.
    List { plan_id: String },
    /// Show a decision by id.
    View { id: String },
    /// Supersede an existing decision (creates a new accepted decision and
    /// flips the predecessor to `superseded`).
    Supersede(DecisionSupersedeArgs),
}

#[derive(Debug, Args)]
pub struct DecisionCreateArgs {
    pub plan_id: String,
    pub short_code: String,
    pub title: String,
    #[arg(long, default_value = "")]
    pub body: String,
}

#[derive(Debug, Args)]
pub struct DecisionSupersedeArgs {
    /// Id of the prior decision being superseded.
    pub prior_id: String,
    /// Plan id for the new decision.
    #[arg(long)]
    pub plan_id: String,
    #[arg(long)]
    pub short_code: String,
    #[arg(long)]
    pub title: String,
    #[arg(long, default_value = "")]
    pub body: String,
}

#[derive(Debug, Subcommand)]
pub enum KnowledgeCmd {
    /// Create a knowledge artifact (PRD §5.4 scope-aware).
    Create(KnowledgeCreateArgs),
    /// List knowledge for a project.
    List(KnowledgeListArgs),
    /// Show a knowledge artifact.
    View { id: String },
    /// Update title/body/tags.
    Update(KnowledgeUpdateArgs),
    /// Delete a knowledge artifact.
    Delete { id: String },
    /// Full-text search.
    Search(KnowledgeSearchArgs),
}

#[derive(Debug, Args)]
pub struct KnowledgeCreateArgs {
    pub project_id: String,
    /// rag | reference | archive (default: rag).
    #[arg(long, default_value = "rag")]
    pub scope: String,
    /// Free-form kind (decision | runbook | note | …).
    #[arg(long)]
    pub kind: String,
    pub title: String,
    #[arg(long, default_value = "")]
    pub body: String,
    /// Repeatable tag flag.
    #[arg(long = "tag")]
    pub tags: Vec<String>,
    #[arg(long)]
    pub source: Option<String>,
}

#[derive(Debug, Args)]
pub struct KnowledgeListArgs {
    pub project_id: String,
    /// Optional scope filter (rag | reference | archive).
    #[arg(long)]
    pub scope: Option<String>,
}

#[derive(Debug, Args)]
pub struct KnowledgeUpdateArgs {
    pub id: String,
    #[arg(long)]
    pub title: String,
    #[arg(long)]
    pub body: String,
    #[arg(long = "tag")]
    pub tags: Vec<String>,
}

#[derive(Debug, Args)]
pub struct KnowledgeSearchArgs {
    pub project_id: String,
    /// Query string (FTS5 MATCH).
    pub q: String,
    #[arg(long)]
    pub scope: Option<String>,
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

// ---------------------------------------------------------------------------
// Comment
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum CommentCmd {
    /// Create a comment anchored to exactly one of plan/task/scenario/round.
    Create(CommentCreateArgs),
    /// List comments by anchor (`--on plan=PLAN-…`) or by project.
    List(CommentListArgs),
    /// Show a comment by id.
    View { id: String },
    /// Edit the body (SNAPSHOT semantics).
    Update(CommentUpdateArgs),
    /// Delete a comment.
    Delete { id: String },
}

#[derive(Debug, Args)]
pub struct CommentCreateArgs {
    pub project_id: String,
    pub author: String,
    /// Anchor: `plan=PLAN-…` or `task=TASK-…` or `scenario=SCN-…` or `round=ROUND-…`.
    #[arg(long)]
    pub on: String,
    pub body: String,
}

#[derive(Debug, Args)]
pub struct CommentListArgs {
    /// Filter by anchor (e.g. `--on task=TASK-…`).
    #[arg(long)]
    pub on: Option<String>,
    /// Or list all comments for a project.
    #[arg(long)]
    pub project_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct CommentUpdateArgs {
    pub id: String,
    #[arg(long)]
    pub body: String,
}

// ---------------------------------------------------------------------------
// Question
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum QuestionCmd {
    Create(QuestionCreateArgs),
    List(QuestionListArgs),
    View { id: String },
    Answer(QuestionAnswerArgs),
}

#[derive(Debug, Args)]
pub struct QuestionCreateArgs {
    pub project_id: String,
    pub plan_id: String,
    pub author: String,
    pub body: String,
}

#[derive(Debug, Args)]
pub struct QuestionListArgs {
    pub plan_id: String,
    /// Filter: open | answered (default: all).
    #[arg(long)]
    pub status: Option<String>,
}

#[derive(Debug, Args)]
pub struct QuestionAnswerArgs {
    pub id: String,
    pub author: String,
    pub answer: String,
}

// ---------------------------------------------------------------------------
// Run
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum RunCmd {
    /// Start a run against a task.
    Start(RunStartArgs),
    /// Mark a run terminal.
    Finish(RunFinishArgs),
    /// Show a run.
    View { id: String },
    /// List runs by task or session.
    List(RunListArgs),
}

#[derive(Debug, Args)]
pub struct RunStartArgs {
    pub task_id: String,
    #[arg(long, default_value = "agent")]
    pub actor: String,
    #[arg(long)]
    pub session_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct RunFinishArgs {
    pub id: String,
    /// success | failure | aborted.
    #[arg(long)]
    pub result: String,
    #[arg(long)]
    pub notes: Option<String>,
}

#[derive(Debug, Args)]
pub struct RunListArgs {
    #[arg(long)]
    pub task_id: Option<String>,
    #[arg(long)]
    pub session_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Usage
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum UsageCmd {
    /// Record a usage row.
    Record(UsageRecordArgs),
    /// List usage rows by scope.
    List(UsageListArgs),
    /// Plan rollup.
    Plan { plan_id: String },
}

#[derive(Debug, Args)]
pub struct UsageRecordArgs {
    pub project_id: String,
    #[arg(long)]
    pub plan_id: Option<String>,
    #[arg(long)]
    pub task_id: Option<String>,
    #[arg(long)]
    pub run_id: Option<String>,
    #[arg(long, default_value = "")]
    pub model: String,
    #[arg(long)]
    pub tier: Option<String>,
    #[arg(long, default_value_t = 0)]
    pub input_tokens: i64,
    #[arg(long, default_value_t = 0)]
    pub output_tokens: i64,
    #[arg(long, default_value_t = 0)]
    pub cache_read: i64,
    #[arg(long, default_value_t = 0)]
    pub cache_write: i64,
    #[arg(long, default_value_t = 0)]
    pub tool_calls: i64,
    #[arg(long, default_value_t = 0.0)]
    pub cost_usd: f64,
}

#[derive(Debug, Args)]
pub struct UsageListArgs {
    #[arg(long)]
    pub project_id: Option<String>,
    #[arg(long)]
    pub plan_id: Option<String>,
    #[arg(long)]
    pub task_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Aggregate views
// ---------------------------------------------------------------------------

#[derive(Debug, Args)]
pub struct DashboardArgs {
    /// Resolve project by cwd (default: current working directory).
    #[arg(long)]
    pub cwd: Option<String>,
    /// Resolve project explicitly by id or key.
    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct TimelineArgs {
    pub project_id: String,
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct BoardArgs {
    pub project_id: String,
}

#[derive(Debug, Args)]
pub struct WikiArgs {
    pub project_id: String,
}

#[derive(Debug, Args)]
pub struct SummaryArgs {
    #[arg(long)]
    pub cwd: Option<String>,
    #[arg(long)]
    pub project: Option<String>,
}

#[derive(Debug, Args)]
pub struct ReplayArgs {
    #[arg(long)]
    pub project_id: Option<String>,
    /// RFC3339 timestamp lower bound.
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long, default_value_t = 200)]
    pub limit: usize,
}

// ---------------------------------------------------------------------------
// Import / export
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum ExportCmd {
    /// Export all plan-scoped state for a project as JSON.
    Plans { project_id: String },
    /// Export knowledge (optionally filtered by scope).
    Knowledge(ExportKnowledgeArgs),
}

#[derive(Debug, Args)]
pub struct ExportKnowledgeArgs {
    pub project_id: String,
    #[arg(long)]
    pub scope: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ImportCmd {
    /// Import plans (input path or "-" for stdin).
    Plans(ImportPlansArgs),
    /// Import knowledge.
    Knowledge(ImportKnowledgeArgs),
}

#[derive(Debug, Args)]
pub struct ImportPlansArgs {
    pub input: String,
    /// Override every plan's project_id during import.
    #[arg(long)]
    pub project_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct ImportKnowledgeArgs {
    pub input: String,
    #[arg(long)]
    pub project_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Autonomy (D14 / D17 / D18)
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum AutonomyCmd {
    /// Set a policy (scope = global | plan | decision_kind). Upsert semantics.
    Set(AutonomySetArgs),
    /// Resolve the effective policy at a scope (plan > decision_kind > global).
    Get(AutonomyGetArgs),
    /// List every policy on a project.
    List { project_id: String },
    /// D18 circuit breaker — demote every policy in a project to L3.
    CircuitBreaker(AutonomyCircuitBreakerArgs),
}

#[derive(Debug, Args)]
pub struct AutonomySetArgs {
    pub project_id: String,
    /// global | plan | decision_kind.
    #[arg(long)]
    pub scope: String,
    /// L3 | L4 | L5.
    #[arg(long)]
    pub mode: String,
    /// Required when scope=plan.
    #[arg(long)]
    pub plan_id: Option<String>,
    /// Required when scope=decision_kind (architecture | schema | naming-canonical | …).
    #[arg(long)]
    pub decision_kind: Option<String>,
    /// Who set it (default: agent).
    #[arg(long, default_value = "agent")]
    pub set_by: String,
    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Debug, Args)]
pub struct AutonomyGetArgs {
    pub project_id: String,
    #[arg(long)]
    pub plan_id: Option<String>,
    #[arg(long)]
    pub decision_kind: Option<String>,
}

#[derive(Debug, Args)]
pub struct AutonomyCircuitBreakerArgs {
    pub project_id: String,
    /// Required — the panic-switch always records a reason.
    #[arg(long)]
    pub reason: String,
    #[arg(long, default_value = "user")]
    pub actor: String,
}

// ---------------------------------------------------------------------------
// AgentNote (M1 blackboard / M2 hand-off)
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum AgentNoteCmd {
    /// Append a note to the blackboard. `--kind handoff` plus `--to` triggers
    /// the hand-off receipt path.
    Append(AgentNoteAppendArgs),
    /// List active notes anchored at a scope.
    List(AgentNoteListArgs),
    /// List pending hand-offs addressed to an agent.
    Handoffs { to_agent: String },
    /// Acknowledge a hand-off (clears it from pending queue).
    Ack { id: String },
    /// Retire a note (non-destructive; reason required).
    Retire(AgentNoteRetireArgs),
}

#[derive(Debug, Args)]
pub struct AgentNoteAppendArgs {
    pub project_id: String,
    /// Scope of the note (plan | round | scenario | task | global).
    #[arg(long)]
    pub scope: String,
    /// Note kind (handoff | observation | question | answer | warning | summary).
    #[arg(long)]
    pub kind: String,
    /// Agent emitting the note.
    #[arg(long)]
    pub from: String,
    /// Anchor ids — set whichever matches the scope.
    #[arg(long)]
    pub plan_id: Option<String>,
    #[arg(long)]
    pub round_id: Option<String>,
    #[arg(long)]
    pub scenario_id: Option<String>,
    #[arg(long)]
    pub task_id: Option<String>,
    /// Target agent (required when kind=handoff).
    #[arg(long)]
    pub to: Option<String>,
    pub body: String,
}

#[derive(Debug, Args)]
pub struct AgentNoteListArgs {
    /// Scope (plan | round | scenario | task | global).
    #[arg(long)]
    pub scope: String,
    /// Anchor id matching the scope.
    #[arg(long)]
    pub anchor: String,
    /// Optional kind filter.
    #[arg(long)]
    pub kind: Option<String>,
}

#[derive(Debug, Args)]
pub struct AgentNoteRetireArgs {
    pub id: String,
    #[arg(long)]
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Consensus (D20 — M3 4-stage)
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum ConsensusCmd {
    /// Group a plan's decisions by proposal and report 4-stage progress.
    Status(ConsensusStatusArgs),
}

#[derive(Debug, Args)]
pub struct ConsensusStatusArgs {
    pub plan_id: String,
    /// Filter to a single proposal id.
    #[arg(long)]
    pub proposal_id: Option<String>,
}
