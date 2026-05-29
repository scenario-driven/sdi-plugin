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
//! - pattern  — D22/D23/D24/D26/D27 CollaborationPattern lifecycle + tree
//! - mcp      — stdio MCP server (PRD §5.4)

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "sdi",
    version,
    about = "Scenario-Driven Implementation — LLM-era successor to TDD/BDD."
)]
pub struct App {
    /// Output format: `json` (default) or `text`.
    /// `text` is reserved for future tabular output; commands fall back to
    /// `json` when `text` is not yet supported.
    #[arg(long, global = true, default_value = "json")]
    pub format: String,

    /// Quiet mode: print only the entity id on success.
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
    /// Environment diagnostics (XDG paths, database, daemon liveness). `--format` is ignored.
    // XDG path invariant lineage: Clawket LM-8.
    Doctor,
    /// Project: multi-project root entity.
    #[command(subcommand)]
    Project(ProjectCmd),
    /// Plan: lifecycle (draft → active → completed).
    // D1 (plan identity) / D8 (approve gate: ≥1 confirmed scenario).
    #[command(subcommand)]
    Plan(PlanCmd),
    /// Requirement: snapshot doc (overwrite in place, no versioning).
    // D12 SNAPSHOT semantics.
    #[command(subcommand, alias = "requirement")]
    Req(RequirementCmd),
    /// Scenario: strict Given/When/Then acceptance criterion.
    // D5 GWT-strict.
    #[command(subcommand)]
    Scenario(ScenarioCmd),
    /// Round: regression-defaulting iteration (R1, R2…).
    // D6 strict-regression default.
    #[command(subcommand)]
    Round(RoundCmd),
    /// Task: runtime work unit; `done` requires evidence.
    // D3 runtime artifact, PRD §6.6 evidence mandatory on complete.
    #[command(subcommand)]
    Task(TaskCmd),
    /// Decision: append-only ADR log with supersession chain.
    // D12 append-only.
    #[command(subcommand)]
    Decision(DecisionCmd),
    /// Knowledge: knowledge artifacts under `rag` / `reference` / `archive` scopes.
    // PRD §5.4 scope model.
    #[command(subcommand)]
    Knowledge(KnowledgeCmd),
    /// Comment: polymorphic anchor (plan / task / scenario / round).
    #[command(subcommand)]
    Comment(CommentCmd),
    /// Question: open / answered Q&A, anchored to a plan.
    #[command(subcommand)]
    Question(QuestionCmd),
    /// Run: a single execution attempt of a task (`start` → `finish`).
    #[command(subcommand)]
    Run(RunCmd),
    /// Token and tool-call accounting plus preflight cost estimate.
    #[command(subcommand)]
    Usage(UsageCmd),
    /// Aggregate dashboard (active plan + counts + recent activity).
    Dashboard(DashboardArgs),
    /// Project handoff bundle (hand a session over to another agent).
    Handoff(HandoffArgs),
    /// Activity timeline for a project.
    Timeline(TimelineArgs),
    /// Project-scoped board view (in-flight + backlog).
    Board(BoardArgs),
    /// Project-scoped wiki view (rag-scoped knowledge).
    Wiki(WikiArgs),
    /// Project summary (counts + active plan).
    Summary(SummaryArgs),
    /// Prometheus metrics dump (text exposition format). `--format` is ignored.
    Metrics,
    /// Replay recorded events from the persistent event log.
    Replay(ReplayArgs),
    /// JSON import/export of plan-scoped state.
    #[command(subcommand)]
    Export(ExportCmd),
    /// JSON import (matching `export` shape).
    #[command(subcommand)]
    Import(ImportCmd),
    /// Autonomy: per-scope L3 / L4 / L5 policy and circuit breaker.
    // D14 (entity) / D17 (mode defaults) / D18 (circuit breaker).
    #[command(subcommand)]
    Autonomy(AutonomyCmd),
    /// Agent note: shared blackboard + agent-to-agent hand-off receipts.
    // PRD §5 layer M1 (blackboard) / M2 (hand-off).
    #[command(subcommand, name = "agent-note")]
    AgentNote(AgentNoteCmd),
    /// Consensus: 4-stage proposal / critique / consensus / dissensus status.
    // PRD §5 layer M3, D20 unit of autonomy gate.
    #[command(subcommand)]
    Consensus(ConsensusCmd),
    /// Collaboration pattern: workflow / graph / swarm / agents-as-tools / direct.
    // D22 entity, D24 parent-child DAG, D26 shape gate at pending→active.
    #[command(subcommand)]
    Pattern(PatternCmd),
    /// MCP server (stdio JSON-RPC, exposes the rag scope only).
    Mcp,
    /// Initialise a project anchored to the current cwd (idempotent).
    Init(InitArgs),
    /// Back up the SQLite database to a target path.
    Backup(BackupArgs),
    /// Restore the SQLite database from a backup path (destructive).
    Restore(RestoreArgs),
    /// Print the resolved configuration: data paths and daemon liveness. `--format` is ignored.
    Config,
    /// Tail the daemon log.
    Log(LogArgs),
    /// Watch the SSE stream and print events.
    Watch(WatchArgs),
    /// Emit a shell completion script for the given shell.
    Completions(CompletionsArgs),
    /// Arm, disarm, or inspect the emergency hook-bypass marker.
    ///
    /// One armed marker unlocks every mutating PreToolUse gate (D21
    /// delegation, active-task, D29 claim overlap) for the next single
    /// tool invocation, then auto-consumes. The marker file lives at
    /// `~/.cache/sdi/bypass-once`. `sdi` is on the D21 read-only Bash
    /// whitelist, so the main session can run `sdi bypass arm` directly
    /// — no specialist delegation required.
    #[command(subcommand)]
    Bypass(BypassCmd),
}

#[derive(Debug, Subcommand)]
pub enum BypassCmd {
    /// Arm the marker so the next mutating tool call skips every gate.
    ///
    /// Recommended over the legacy env switches (`SDI_DELEGATION_BYPASS=1`,
    /// `SDI_BYPASS_HOOKS=1`, `SDI_HOOK_V05_DISABLE=1`) — those work only
    /// when exported from the shell that launched Claude Code, whereas
    /// the marker works mid-session. Routine use is a protocol violation;
    /// every arm + consumption is recorded in the hook audit log
    /// (`~/.local/state/sdi/hook.log`).
    Arm(BypassArmArgs),
    /// Remove the bypass marker (idempotent — succeeds whether one exists or not).
    Disarm,
    /// Report the marker state (`armed` | `expired` | `absent`).
    ///
    /// Output includes the TTL remainder, the reason recorded at `arm`
    /// time, and the on-disk marker path so you can confirm the hook is
    /// reading the same file.
    Status,
}

#[derive(Debug, Args)]
pub struct BypassArmArgs {
    /// Free-form reason recorded inside the marker.
    ///
    /// Surfaces in the hook's stderr warning + audit log so future
    /// spelunkers know why the bypass fired. Keep it short (e.g.
    /// "investigating CI flake", "emergency rollback").
    #[arg(long)]
    pub reason: Option<String>,
    /// Seconds before the marker self-expires. Default 60s.
    ///
    /// Short enough that a forgotten marker doesn't silently disarm every
    /// gate forever. Expired markers are cleaned up on the next hook hit
    /// but do NOT open the gate.
    #[arg(long, default_value_t = 60)]
    pub ttl: i64,
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
pub struct HandoffArgs {
    #[command(flatten)]
    pub selector: ProjectSelector,
}

#[derive(Debug, Args)]
pub struct BackupArgs {
    /// Destination file path for the SQLite backup copy.
    pub output: String,
}

#[derive(Debug, Args)]
pub struct RestoreArgs {
    /// Source file path of a prior backup. Replaces the current database.
    pub input: String,
}

#[derive(Debug, Args)]
pub struct CompletionsArgs {
    /// Target shell: `bash` | `zsh` | `fish` | `powershell` | `elvish`.
    pub shell: String,
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
    View {
        /// Project id.
        id: String,
    },
    /// Show the project owning a given working directory.
    ByCwd {
        /// Absolute working-directory path to look up.
        cwd: String,
    },
    /// PATCH-style update — every field is optional; only those passed are
    /// changed. Slug and key are immutable identifiers; to change them,
    /// create a new project.
    Update(ProjectUpdateArgs),
    /// Soft-disable the project. The hook layer (`projectByCwd` consumers
    /// in the plugin shell) skips every mutating gate for the project's
    /// anchored cwds while disabled, so Claude Code drives the repo
    /// without SDI governance. Idempotent.
    Disable {
        /// Project id.
        id: String,
    },
    /// Re-enable a previously disabled project. Idempotent.
    Enable {
        /// Project id.
        id: String,
    },
    /// Hard-delete the project and every PROJ-scoped row (plans,
    /// requirements, scenarios, rounds, tasks, decisions, comments,
    /// questions, knowledge, agent_notes, autonomy_policies,
    /// collaboration_patterns, activity events). Default behavior refuses
    /// deletion if any task under the project is `in_progress`; pass
    /// `--force` to delete anyway. Irreversible.
    Delete(ProjectDeleteArgs),
    /// Attach a working-directory to a project.
    CwdAttach {
        /// Project id.
        project_id: String,
        /// Absolute working-directory path to attach.
        cwd: String,
    },
    /// Detach a working-directory from a project.
    CwdDetach {
        /// Project id.
        project_id: String,
        /// Absolute working-directory path to detach.
        cwd: String,
    },
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
    /// Project id.
    pub id: String,
    /// New display name. Whitespace-only values are rejected.
    #[arg(long)]
    pub name: Option<String>,
    /// New free-form description. Pass an empty string to clear.
    #[arg(long)]
    pub description: Option<String>,
    /// Replace the wiki path roots (repeatable; relative to cwd or absolute).
    /// Passing `--wiki-path` zero times leaves the array untouched; pass it
    /// at least once to replace.
    #[arg(long = "wiki-path")]
    pub wiki_paths: Vec<String>,
}

#[derive(Debug, Args)]
pub struct ProjectDeleteArgs {
    /// Project id.
    pub id: String,
    /// Skip the in-flight task guard. Defaults to false (delete refuses
    /// when any task under the project is `in_progress`).
    #[arg(long, default_value_t = false)]
    pub force: bool,
}

#[derive(Debug, Subcommand)]
pub enum PlanCmd {
    /// Create a plan (status starts at `draft`).
    Create(PlanCreateArgs),
    /// List plans for a project.
    List {
        /// Project id.
        project_id: String,
    },
    /// Show a plan by id.
    View {
        /// Plan id.
        id: String,
    },
    /// Replace the title and body of a plan in place (snapshot-style).
    Update(PlanUpdateArgs),
    /// Approve a plan. Requires at least one confirmed scenario.
    // D8 approve gate.
    Approve {
        /// Plan id.
        id: String,
    },
    /// Complete a plan.
    Complete {
        /// Plan id.
        id: String,
    },
    /// Show the active plan for a project (404 if none).
    Active {
        /// Project id.
        project_id: String,
    },
    /// Composite snapshot: plan + scenarios + in-flight tasks + decisions.
    // Mirrors the MCP `get_plan_context` tool (PRD §5.4).
    Context {
        /// Plan id.
        id: String,
    },
}

#[derive(Debug, Args)]
pub struct PlanCreateArgs {
    /// Project id this plan belongs to.
    pub project_id: String,
    /// Short stable code (e.g. `SDI-1`).
    pub short_code: String,
    /// Title.
    pub title: String,
    /// Body markdown. Use `-` to read from stdin.
    #[arg(long, default_value = "")]
    pub body: String,
}

#[derive(Debug, Args)]
pub struct PlanUpdateArgs {
    /// Plan id.
    pub id: String,
    /// New title.
    #[arg(long)]
    pub title: Option<String>,
    /// New body markdown.
    #[arg(long)]
    pub body: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum RequirementCmd {
    /// Create a requirement under a plan.
    Create(RequirementCreateArgs),
    /// List requirements for a plan.
    List {
        /// Plan id.
        plan_id: String,
    },
    /// Show a requirement by id.
    View {
        /// Requirement id.
        id: String,
    },
    /// Overwrite a requirement in place (snapshot semantics).
    // D12 SNAPSHOT.
    Update(RequirementUpdateArgs),
    /// Delete a requirement.
    Delete {
        /// Requirement id.
        id: String,
    },
}

#[derive(Debug, Args)]
pub struct RequirementCreateArgs {
    /// Plan id this requirement belongs to.
    pub plan_id: String,
    /// Short stable code (e.g. `REQ-1`).
    pub short_code: String,
    /// Title.
    pub title: String,
    /// Body markdown.
    #[arg(long, default_value = "")]
    pub body: String,
    /// Free-form source reference (file:line, ticket URL, …).
    #[arg(long)]
    pub source: Option<String>,
}

#[derive(Debug, Args)]
pub struct RequirementUpdateArgs {
    /// Requirement id.
    pub id: String,
    /// New title.
    #[arg(long)]
    pub title: Option<String>,
    /// New body markdown.
    #[arg(long)]
    pub body: Option<String>,
    /// New source reference.
    #[arg(long)]
    pub source: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ScenarioCmd {
    /// Create a scenario. All three of `given` / `when` / `then` are required.
    // D5 GWT-strict.
    Create(ScenarioCreateArgs),
    /// List scenarios for a plan.
    List {
        /// Plan id.
        plan_id: String,
    },
    /// Show a scenario by id.
    View {
        /// Scenario id.
        id: String,
    },
    /// Replace given / when / then in place (snapshot).
    Update(ScenarioUpdateArgs),
    /// Mark a scenario as confirmed (counts toward the plan approve gate).
    // Advances the D8 approve gate.
    Confirm {
        /// Scenario id.
        id: String,
    },
    /// Full-text search across one plan's scenarios.
    // FTS5; mirrors the MCP `search_scenarios` tool (PRD §5.4).
    Search(ScenarioSearchArgs),
    /// Mark this scenario's resource claim as active before editing.
    // D29 multi-session claim ledger. Callers should pre-check
    // `GET /scenarios/active-claims` for overlap; the daemon does not yet
    // compute glob intersection itself.
    Claim {
        /// Scenario id.
        id: String,
    },
    /// Release this scenario's resource claim once editing is done.
    // D29.
    Release {
        /// Scenario id.
        id: String,
    },
}

#[derive(Debug, Args)]
pub struct ScenarioSearchArgs {
    /// Plan id whose scenarios to search.
    pub plan_id: String,
    /// FTS5 MATCH expression. Quote multi-word queries.
    pub query: String,
    /// Cap on results (max 50).
    #[arg(long, default_value_t = 10)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct ScenarioCreateArgs {
    /// Plan id this scenario belongs to.
    pub plan_id: String,
    /// Short stable code (e.g. `SCN-1`).
    pub short_code: String,
    /// Given clause (precondition).
    #[arg(long)]
    pub given: String,
    /// When clause (action/event).
    #[arg(long, name = "when")]
    pub when_: String,
    /// Then clause (expected outcome).
    #[arg(long, name = "then")]
    pub then_: String,
    /// Mark as confirmed immediately (default: draft).
    #[arg(long)]
    pub confirmed: bool,
}

#[derive(Debug, Args)]
pub struct ScenarioUpdateArgs {
    /// Scenario id.
    pub id: String,
    /// New Given clause.
    #[arg(long)]
    pub given: Option<String>,
    /// New When clause.
    #[arg(long, name = "when")]
    pub when_: Option<String>,
    /// New Then clause.
    #[arg(long, name = "then")]
    pub then_: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum RoundCmd {
    /// Create a round (defaults: strict-regression, needs-review, pause-in-flight).
    Create(RoundCreateArgs),
    /// List rounds for a plan.
    List {
        /// Plan id.
        plan_id: String,
    },
    /// Show a round by id.
    View {
        /// Round id.
        id: String,
    },
    /// Activate a round. From R2 onwards, prior verdicts auto-carry under strict-regression mode.
    Activate {
        /// Round id.
        id: String,
    },
    /// Complete a round.
    Complete {
        /// Round id.
        id: String,
    },
    /// Record a per-scenario verdict.
    Result(RoundResultArgs),
    /// List all results for a round.
    Results {
        /// Round id.
        id: String,
    },
    /// Show the active round for a plan.
    Active {
        /// Plan id.
        plan_id: String,
    },
}

#[derive(Debug, Args)]
pub struct RoundCreateArgs {
    /// Plan id this round belongs to.
    pub plan_id: String,
    /// Short stable code (e.g. `R1`).
    pub short_code: String,
    /// Regression mode: `strict-regression` (default) | `additive` | `disruption`.
    #[arg(long)]
    pub mode: Option<String>,
    /// In-flight task policy: `pause` (default) | `abort` | `continue-on-noimpact`.
    #[arg(long)]
    pub in_flight: Option<String>,
    /// Disruption resolution policy: `needs-review` (default) | `auto`. `auto`
    /// only changes how the LLM proposes resolutions; human-confirm is universal.
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
    /// Result: `passing` | `failing` | `impacted` | `retired`.
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
    List {
        /// Round id.
        round_id: String,
    },
    /// Show a task by id.
    View {
        /// Task id.
        id: String,
    },
    /// Pick up a task (`todo` → `in_progress`).
    Start {
        /// Task id.
        id: String,
    },
    /// Block on an external dependency.
    Block {
        /// Task id.
        id: String,
    },
    /// Cancel a task.
    Cancel {
        /// Task id.
        id: String,
    },
    /// Complete a task. `--evidence` is mandatory (one per parent scenario).
    // PRD §6.6 evidence gate.
    Complete(TaskCompleteArgs),
    /// Decompose a task into subtasks (parent relations created automatically).
    Decompose(TaskDecomposeArgs),
    /// Show the ancestor chain for a task.
    Ancestors {
        /// Task id.
        id: String,
    },
    /// Show the descendant chain for a task.
    Descendants {
        /// Task id.
        id: String,
    },
    /// Show the subtree (root + descendants) for a task.
    Subtree {
        /// Task id.
        id: String,
    },
    /// List non-parent relations on a task.
    Relations {
        /// Task id.
        id: String,
    },
    /// Add a non-parent relation between two tasks.
    Relate(TaskRelateArgs),
    /// Remove a relation by id.
    Unrelate {
        /// Relation id.
        relation_id: String,
    },
    /// Acquire a lease on a task (single-writer mutex).
    Lease(TaskLeaseArgs),
    /// Heartbeat an existing lease to extend its TTL.
    Heartbeat(TaskLeaseArgs),
    /// Release a lease on a task.
    Release(TaskLeaseReleaseArgs),
    /// Show the current lease on a task (if any).
    LeaseInfo {
        /// Task id.
        id: String,
    },
    /// Task status histogram for one project (default: current directory).
    Stats(ProjectSelector),
    /// Preflight: estimate cost for a task using prior tier history.
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
    /// Child task id (the relation's "to" side).
    #[arg(long)]
    pub child: String,
    /// Relation kind: `blocks` | `depends-on` | `duplicates` | `related` (default).
    #[arg(long, default_value = "related")]
    pub kind: String,
}

#[derive(Debug, Args)]
pub struct TaskLeaseArgs {
    /// Task id.
    pub id: String,
    /// Holder label (agent name or session id).
    #[arg(long)]
    pub holder: String,
    /// Lease TTL in seconds (default 120).
    #[arg(long, default_value_t = 120)]
    pub ttl_seconds: i64,
}

#[derive(Debug, Args)]
pub struct TaskLeaseReleaseArgs {
    /// Task id.
    pub id: String,
    /// Holder label (must match the current lease holder).
    #[arg(long)]
    pub holder: String,
}

#[derive(Debug, Args)]
pub struct TaskPreflightArgs {
    /// Task id.
    pub id: String,
    /// Override the task's own tier (`low` | `med` | `high`).
    #[arg(long)]
    pub tier: Option<String>,
}

#[derive(Debug, Args)]
pub struct TaskCreateArgs {
    /// Round id this task belongs to.
    pub round_id: String,
    /// Short stable code (e.g. `TASK-1`).
    pub short_code: String,
    /// One-line description.
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
    /// Task id.
    pub id: String,
    /// Per-scenario verdict (repeatable). Format: `SCN-…=passing@file:line`.
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
    List {
        /// Plan id.
        plan_id: String,
    },
    /// Show a decision by id.
    View {
        /// Decision id.
        id: String,
    },
    /// Supersede an existing decision. Creates a new accepted decision and
    /// flips the predecessor to `superseded`.
    Supersede(DecisionSupersedeArgs),
    /// Append a rollback decision that reverses an earlier one (append-only;
    /// the original is never mutated).
    // D28 reversibility: kind=consensus, reversal_of=<id>.
    Rollback(DecisionRollbackArgs),
}

#[derive(Debug, Args)]
pub struct DecisionCreateArgs {
    /// Plan id this decision belongs to.
    pub plan_id: String,
    /// Short stable code (e.g. `DEC-1`).
    pub short_code: String,
    /// Title.
    pub title: String,
    /// Body markdown.
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
    /// Short stable code for the new decision row.
    #[arg(long)]
    pub short_code: String,
    /// Title of the new decision row.
    #[arg(long)]
    pub title: String,
    /// Body markdown of the new decision row.
    #[arg(long, default_value = "")]
    pub body: String,
}

#[derive(Debug, Args)]
pub struct DecisionRollbackArgs {
    /// Id of the original decision being rolled back.
    pub id: String,
    /// Short code for the rollback decision row.
    #[arg(long)]
    pub short_code: String,
    /// Title for the rollback decision row.
    #[arg(long)]
    pub title: String,
    /// Body for the rollback decision row.
    #[arg(long, default_value = "")]
    pub body: String,
    /// The rollback's own reversal_plan JSON (in case the reversal itself
    /// needs to be reversed). Daemon re-validates the JSON. Use
    /// `--reversal-plan-from-file path` to read it from disk instead.
    // D28 reversibility.
    #[arg(long, conflicts_with = "reversal_plan_from_file")]
    pub reversal_plan: Option<String>,
    /// Read the reversal_plan JSON from the given file path.
    #[arg(long)]
    pub reversal_plan_from_file: Option<String>,
    /// Blast-radius score for the rollback row (default 5).
    // D28 reversibility scoring.
    #[arg(long)]
    pub blast_radius_score: Option<i32>,
    /// Optional `produced_via_pattern_id` for the rollback row; the daemon
    /// writes a `direct` pattern row if absent.
    #[arg(long)]
    pub produced_via_pattern_id: Option<String>,
    /// Reason / agent label (default: `reversal-runner`).
    #[arg(long, default_value = "reversal-runner")]
    pub agent_name: String,
}

#[derive(Debug, Subcommand)]
pub enum KnowledgeCmd {
    /// Create a knowledge artifact in one of the rag / reference / archive scopes.
    // PRD §5.4 scope model.
    Create(KnowledgeCreateArgs),
    /// List knowledge for a project.
    List(KnowledgeListArgs),
    /// Show a knowledge artifact.
    View {
        /// Knowledge artifact id.
        id: String,
    },
    /// Update title / body / tags.
    Update(KnowledgeUpdateArgs),
    /// Delete a knowledge artifact.
    Delete {
        /// Knowledge artifact id.
        id: String,
    },
    /// Full-text search across knowledge artifacts.
    Search(KnowledgeSearchArgs),
}

#[derive(Debug, Args)]
pub struct KnowledgeCreateArgs {
    /// Project id this artifact belongs to.
    pub project_id: String,
    /// Visibility scope: `rag` (default) | `reference` | `archive`.
    #[arg(long, default_value = "rag")]
    pub scope: String,
    /// Free-form kind tag (e.g. `decision` | `runbook` | `note`).
    #[arg(long)]
    pub kind: String,
    /// Title.
    pub title: String,
    /// Body markdown.
    #[arg(long, default_value = "")]
    pub body: String,
    /// Repeatable tag.
    #[arg(long = "tag")]
    pub tags: Vec<String>,
    /// Free-form source reference (URL, file:line, …).
    #[arg(long)]
    pub source: Option<String>,
}

#[derive(Debug, Args)]
pub struct KnowledgeListArgs {
    /// Project id.
    pub project_id: String,
    /// Optional scope filter (`rag` | `reference` | `archive`).
    #[arg(long)]
    pub scope: Option<String>,
}

#[derive(Debug, Args)]
pub struct KnowledgeUpdateArgs {
    /// Knowledge artifact id.
    pub id: String,
    /// New title.
    #[arg(long)]
    pub title: String,
    /// New body markdown.
    #[arg(long)]
    pub body: String,
    /// Replacement tag set (repeatable).
    #[arg(long = "tag")]
    pub tags: Vec<String>,
}

#[derive(Debug, Args)]
pub struct KnowledgeSearchArgs {
    /// Project id.
    pub project_id: String,
    /// Query string (FTS5 MATCH expression).
    pub q: String,
    /// Optional scope filter (`rag` | `reference` | `archive`).
    #[arg(long)]
    pub scope: Option<String>,
    /// Maximum results to return.
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
    View {
        /// Comment id.
        id: String,
    },
    /// Edit the body (snapshot semantics).
    Update(CommentUpdateArgs),
    /// Delete a comment.
    Delete {
        /// Comment id.
        id: String,
    },
}

#[derive(Debug, Args)]
pub struct CommentCreateArgs {
    /// Project id.
    pub project_id: String,
    /// Author label.
    pub author: String,
    /// Anchor: `plan=PLAN-…` | `task=TASK-…` | `scenario=SCN-…` | `round=ROUND-…`.
    #[arg(long)]
    pub on: String,
    /// Comment body markdown.
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
    /// Comment id.
    pub id: String,
    /// New body markdown.
    #[arg(long)]
    pub body: String,
}

// ---------------------------------------------------------------------------
// Question
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum QuestionCmd {
    /// Post a new question under a plan (status starts at `open`).
    Create(QuestionCreateArgs),
    /// List questions for a plan, optionally filtered by status.
    List(QuestionListArgs),
    /// Show a question by id.
    View(QuestionViewArgs),
    /// Answer an open question (transitions status to `answered`).
    Answer(QuestionAnswerArgs),
}

#[derive(Debug, Args)]
pub struct QuestionCreateArgs {
    /// Project id the plan belongs to.
    pub project_id: String,
    /// Plan id this question is anchored to.
    pub plan_id: String,
    /// Author label (e.g. agent name or human handle).
    pub author: String,
    /// Question body (markdown supported).
    pub body: String,
}

#[derive(Debug, Args)]
pub struct QuestionListArgs {
    /// Plan id whose questions to list.
    pub plan_id: String,
    /// Filter: `open` | `answered` (default: all).
    #[arg(long)]
    pub status: Option<String>,
}

#[derive(Debug, Args)]
pub struct QuestionViewArgs {
    /// Question id.
    pub id: String,
}

#[derive(Debug, Args)]
pub struct QuestionAnswerArgs {
    /// Question id being answered.
    pub id: String,
    /// Author label of the responder.
    pub author: String,
    /// Answer body (markdown supported).
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
    /// Show a run by id.
    View {
        /// Run id.
        id: String,
    },
    /// List runs by task or session.
    List(RunListArgs),
}

#[derive(Debug, Args)]
pub struct RunStartArgs {
    /// Task id this run targets.
    pub task_id: String,
    /// Actor label (default: `agent`).
    #[arg(long, default_value = "agent")]
    pub actor: String,
    /// Optional session id to group related runs.
    #[arg(long)]
    pub session_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct RunFinishArgs {
    /// Run id.
    pub id: String,
    /// Terminal result: `success` | `failure` | `aborted`.
    #[arg(long)]
    pub result: String,
    /// Optional free-form notes.
    #[arg(long)]
    pub notes: Option<String>,
}

#[derive(Debug, Args)]
pub struct RunListArgs {
    /// Filter by task id.
    #[arg(long)]
    pub task_id: Option<String>,
    /// Filter by session id.
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
    /// Aggregated usage rollup for a plan.
    #[command(alias = "plan")]
    Rollup {
        /// Plan id.
        plan_id: String,
    },
}

#[derive(Debug, Args)]
pub struct UsageRecordArgs {
    /// Project id this usage row belongs to.
    pub project_id: String,
    /// Optional plan id.
    #[arg(long)]
    pub plan_id: Option<String>,
    /// Optional task id.
    #[arg(long)]
    pub task_id: Option<String>,
    /// Optional run id (a single execution attempt of a task).
    #[arg(long)]
    pub run_id: Option<String>,
    /// Model name (e.g. `claude-opus-4-7`). Blank if unknown.
    #[arg(long, default_value = "", hide_default_value = true)]
    pub model: String,
    /// Cost tier (`low` | `med` | `high`).
    #[arg(long)]
    pub tier: Option<String>,
    /// Input tokens consumed.
    #[arg(long, default_value_t = 0)]
    pub input_tokens: i64,
    /// Output tokens produced.
    #[arg(long, default_value_t = 0)]
    pub output_tokens: i64,
    /// Prompt-cache read tokens.
    #[arg(long, default_value_t = 0)]
    pub cache_read: i64,
    /// Prompt-cache write tokens.
    #[arg(long, default_value_t = 0)]
    pub cache_write: i64,
    /// Tool-call count.
    #[arg(long, default_value_t = 0)]
    pub tool_calls: i64,
    /// Cost in USD.
    #[arg(long, default_value_t = 0.0)]
    pub cost_usd: f64,
}

#[derive(Debug, Args)]
pub struct UsageListArgs {
    /// Filter by project id.
    #[arg(long)]
    pub project_id: Option<String>,
    /// Filter by plan id.
    #[arg(long)]
    pub plan_id: Option<String>,
    /// Filter by task id.
    #[arg(long)]
    pub task_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Aggregate views
// ---------------------------------------------------------------------------

/// Shared project selector for every project-scoped command. A project may be
/// named three interchangeable ways: a positional id/key, `--project <id|key>`,
/// or `--cwd <path>`. When none is given, the current working directory is used.
#[derive(Debug, Args)]
pub struct ProjectSelector {
    /// Project id or key. Omit to use `--project`, `--cwd`, or the current directory.
    pub project_id: Option<String>,
    /// Resolve project explicitly by id or key (alternative to the positional).
    #[arg(long, conflicts_with = "project_id")]
    pub project: Option<String>,
    /// Resolve project by path (default: current working directory).
    #[arg(long, conflicts_with_all = ["project_id", "project"])]
    pub cwd: Option<String>,
}

impl ProjectSelector {
    /// The explicit id/key form, taken from the positional or `--project`
    /// (mutually exclusive). `None` means resolve by `--cwd` / current dir.
    pub fn explicit(&self) -> Option<&str> {
        self.project_id.as_deref().or(self.project.as_deref())
    }
}

#[derive(Debug, Args)]
pub struct DashboardArgs {
    #[command(flatten)]
    pub selector: ProjectSelector,
}

#[derive(Debug, Args)]
pub struct TimelineArgs {
    #[command(flatten)]
    pub selector: ProjectSelector,
    /// Maximum number of events to return.
    #[arg(long, default_value_t = 50)]
    pub limit: usize,
}

#[derive(Debug, Args)]
pub struct BoardArgs {
    #[command(flatten)]
    pub selector: ProjectSelector,
}

#[derive(Debug, Args)]
pub struct WikiArgs {
    #[command(flatten)]
    pub selector: ProjectSelector,
}

#[derive(Debug, Args)]
pub struct SummaryArgs {
    #[command(flatten)]
    pub selector: ProjectSelector,
}

#[derive(Debug, Args)]
pub struct ReplayArgs {
    /// Project id filter (default: all projects).
    #[arg(long)]
    pub project_id: Option<String>,
    /// RFC3339 timestamp lower bound.
    #[arg(long)]
    pub since: Option<String>,
    /// Maximum number of events to return.
    #[arg(long, default_value_t = 200)]
    pub limit: usize,
}

// ---------------------------------------------------------------------------
// Import / export
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum ExportCmd {
    /// Export all plan-scoped state for a project as JSON.
    Plans {
        /// Project id.
        project_id: String,
    },
    /// Export knowledge artifacts (optionally filtered by scope).
    Knowledge(ExportKnowledgeArgs),
}

#[derive(Debug, Args)]
pub struct ExportKnowledgeArgs {
    /// Project id.
    pub project_id: String,
    /// Optional scope filter (`rag` | `reference` | `archive`).
    #[arg(long)]
    pub scope: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ImportCmd {
    /// Import plans from a JSON file (or `-` for stdin).
    Plans(ImportPlansArgs),
    /// Import knowledge artifacts from a JSON file (or `-` for stdin).
    Knowledge(ImportKnowledgeArgs),
}

#[derive(Debug, Args)]
pub struct ImportPlansArgs {
    /// Input file path, or `-` for stdin.
    pub input: String,
    /// Override every plan's `project_id` during import.
    #[arg(long)]
    pub project_id: Option<String>,
}

#[derive(Debug, Args)]
pub struct ImportKnowledgeArgs {
    /// Input file path, or `-` for stdin.
    pub input: String,
    /// Override every artifact's `project_id` during import.
    #[arg(long)]
    pub project_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Autonomy (D14 / D17 / D18)
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum AutonomyCmd {
    /// Set a policy at a scope (`global` | `plan` | `decision_kind`). Upsert semantics.
    Set(AutonomySetArgs),
    /// Resolve the effective policy. Specificity wins: plan > decision_kind > global.
    Get(AutonomyGetArgs),
    /// List every policy on a project.
    List {
        /// Project id.
        project_id: String,
    },
    /// Circuit breaker: demote every policy in a project to L3 (always-ask).
    // D18 panic switch.
    CircuitBreaker(AutonomyCircuitBreakerArgs),
}

#[derive(Debug, Args)]
pub struct AutonomySetArgs {
    /// Project id.
    pub project_id: String,
    /// Scope: `global` | `plan` | `decision_kind`.
    #[arg(long)]
    pub scope: String,
    /// Autonomy mode: `L3` | `L4` | `L5`.
    #[arg(long)]
    pub mode: String,
    /// Required when `--scope plan`.
    #[arg(long)]
    pub plan_id: Option<String>,
    /// Required when `--scope decision_kind` (e.g. `architecture` | `schema` | `naming-canonical`).
    #[arg(long)]
    pub decision_kind: Option<String>,
    /// Who set it (default: `agent`).
    #[arg(long, default_value = "agent")]
    pub set_by: String,
    /// Free-form rationale recorded with the policy.
    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Debug, Args)]
pub struct AutonomyGetArgs {
    /// Project id.
    pub project_id: String,
    /// Resolve the policy effective at this plan scope.
    #[arg(long)]
    pub plan_id: Option<String>,
    /// Resolve the policy effective at this decision_kind scope.
    #[arg(long)]
    pub decision_kind: Option<String>,
}

#[derive(Debug, Args)]
pub struct AutonomyCircuitBreakerArgs {
    /// Project id.
    pub project_id: String,
    /// Required — the panic-switch always records a reason.
    #[arg(long)]
    pub reason: String,
    /// Actor label (default: `user`).
    #[arg(long, default_value = "user")]
    pub actor: String,
}

// ---------------------------------------------------------------------------
// AgentNote (M1 blackboard / M2 hand-off)
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum AgentNoteCmd {
    /// Append a note to the shared blackboard. `--kind handoff` plus `--to`
    /// turns the note into a hand-off receipt addressed to another agent.
    Append(AgentNoteAppendArgs),
    /// List active notes anchored at a scope.
    List(AgentNoteListArgs),
    /// List pending hand-offs addressed to an agent.
    Handoffs {
        /// Recipient agent name (matches the `--to` field on `append`).
        to_agent: String,
    },
    /// Acknowledge a hand-off (clears it from the pending queue).
    Ack {
        /// Note id of the hand-off being acknowledged.
        id: String,
    },
    /// Retire a note (non-destructive; reason required).
    Retire(AgentNoteRetireArgs),
}

#[derive(Debug, Args)]
pub struct AgentNoteAppendArgs {
    /// Project id.
    pub project_id: String,
    /// Anchor scope: `plan` | `round` | `scenario` | `task` | `global`.
    #[arg(long)]
    pub scope: String,
    /// Note kind: `handoff` | `observation` | `question` | `answer` | `warning` | `summary`.
    #[arg(long)]
    pub kind: String,
    /// Name of the agent emitting the note.
    #[arg(long)]
    pub from: String,
    /// Anchor plan id (set when `--scope plan`).
    #[arg(long)]
    pub plan_id: Option<String>,
    /// Anchor round id (set when `--scope round`).
    #[arg(long)]
    pub round_id: Option<String>,
    /// Anchor scenario id (set when `--scope scenario`).
    #[arg(long)]
    pub scenario_id: Option<String>,
    /// Anchor task id (set when `--scope task`).
    #[arg(long)]
    pub task_id: Option<String>,
    /// Target agent (required when `--kind handoff`).
    #[arg(long)]
    pub to: Option<String>,
    /// Note body markdown.
    pub body: String,
}

#[derive(Debug, Args)]
pub struct AgentNoteListArgs {
    /// Anchor scope: `plan` | `round` | `scenario` | `task` | `global`.
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
    /// Note id.
    pub id: String,
    /// Free-form reason for retiring.
    #[arg(long)]
    pub reason: String,
}

// ---------------------------------------------------------------------------
// Consensus (D20 — M3 4-stage)
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum ConsensusCmd {
    /// Group a plan's decisions by proposal and report 4-stage progress
    /// (proposal → critique → consensus → dissensus).
    Status(ConsensusStatusArgs),
}

#[derive(Debug, Args)]
pub struct ConsensusStatusArgs {
    /// Plan id whose decisions to group.
    pub plan_id: String,
    /// Filter to a single proposal id.
    #[arg(long)]
    pub proposal_id: Option<String>,
}

// ---------------------------------------------------------------------------
// CollaborationPattern (D22) — pattern CRUD + lifecycle + tree
// ---------------------------------------------------------------------------

#[derive(Debug, Subcommand)]
pub enum PatternCmd {
    /// Create a collaboration pattern row (lifecycle starts at `pending`).
    // PatternCreateArgs is Box<>'d because it's ~5x the next variant
    // (four `*-json` / `*-from-file` shape flag pairs).
    Create(Box<PatternCreateArgs>),
    /// List patterns for a plan, or `--active` to list every active row across plans.
    List(PatternListArgs),
    /// Show a pattern by id.
    Show {
        /// Pattern id.
        id: String,
    },
    /// Transition the pattern lifecycle. `pending → active` runs the shape
    /// gate; terminal targets (`converged` / `dissensus` / `aborted`) stamp
    /// `decided_at` automatically.
    // D26 shape gate at pending→active.
    Transition(PatternTransitionArgs),
    /// Sugar for `transition --to aborted`.
    Abort(PatternAbortArgs),
    /// Render the plan's pattern parent→child tree.
    // D24 DAG.
    Tree(PatternTreeArgs),
}

#[derive(Debug, Args)]
pub struct PatternCreateArgs {
    /// Plan id the pattern belongs to.
    #[arg(long)]
    pub plan: String,
    /// Short stable code (e.g. `PAT-1`). Unique per plan.
    #[arg(long)]
    pub short_code: String,
    /// `workflow` | `graph` | `swarm` | `agents-as-tools` | `direct`.
    #[arg(long)]
    pub kind: String,
    /// Entity kind the pattern produces (must match `--scope-id`).
    /// One of: `plan` | `requirement` | `scenario` | `task` | `decision` | `round`.
    #[arg(long = "applies-to")]
    pub applies_to: String,
    /// Id of the work entity (must match the kind in `--applies-to`).
    #[arg(long = "scope-id")]
    pub scope_id: String,
    /// Optional parent pattern id; daemon rejects cycles and enforces depth cap.
    // D24 DAG enforcement.
    #[arg(long = "parent")]
    pub parent_pattern_id: Option<String>,
    /// Workflow `steps` JSON (array of {idx, agent, action}). Required when
    /// `--kind workflow`. Conflicts with `--steps-from-file`.
    #[arg(long = "steps-json", conflicts_with = "steps_from_file")]
    pub steps_json: Option<String>,
    /// Read the workflow `steps` JSON from the given file path.
    #[arg(long = "steps-from-file")]
    pub steps_from_file: Option<String>,
    /// Graph `reviewers` JSON (array of {name, stance}). Required when
    /// `--kind graph`. Conflicts with `--reviewers-from-file`.
    #[arg(long = "reviewers-json", conflicts_with = "reviewers_from_file")]
    pub reviewers_json: Option<String>,
    /// Read the graph `reviewers` JSON from the given file path.
    #[arg(long = "reviewers-from-file")]
    pub reviewers_from_file: Option<String>,
    /// Swarm `fan_out` JSON (array of agent names). Required when
    /// `--kind swarm`. Conflicts with `--fan-out-from-file`.
    #[arg(long = "fan-out-json", conflicts_with = "fan_out_from_file")]
    pub fan_out_json: Option<String>,
    /// Read the swarm `fan_out` JSON from the given file path.
    #[arg(long = "fan-out-from-file")]
    pub fan_out_from_file: Option<String>,
    /// Agents-as-tools `peer_registration` JSON (array of {caller, callee}).
    /// Required when `--kind agents-as-tools`. Conflicts with
    /// `--peers-from-file`.
    #[arg(long = "peers-json", conflicts_with = "peers_from_file")]
    pub peers_json: Option<String>,
    /// Read the agents-as-tools `peer_registration` JSON from the given path.
    #[arg(long = "peers-from-file")]
    pub peers_from_file: Option<String>,
}

#[derive(Debug, Args)]
pub struct PatternListArgs {
    /// Plan id (required when `--active` is absent).
    #[arg(long)]
    pub plan: Option<String>,
    /// List every active pattern across the daemon (cross-plan view).
    #[arg(long, default_value_t = false)]
    pub active: bool,
    /// Optional kind filter (`workflow` | `graph` | `swarm` |
    /// `agents-as-tools` | `direct`).
    #[arg(long)]
    pub kind: Option<String>,
}

#[derive(Debug, Args)]
pub struct PatternTransitionArgs {
    /// Pattern id.
    pub id: String,
    /// Target lifecycle state: `pending` | `active` | `converged` | `dissensus` | `aborted`.
    #[arg(long = "to")]
    pub to: String,
    /// Free-form reason recorded in `decided_reason`.
    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Debug, Args)]
pub struct PatternAbortArgs {
    /// Pattern id.
    pub id: String,
    /// Free-form reason recorded in `decided_reason`.
    #[arg(long)]
    pub reason: Option<String>,
}

#[derive(Debug, Args)]
pub struct PatternTreeArgs {
    /// Plan id whose pattern DAG to render.
    #[arg(long)]
    pub plan: String,
}
