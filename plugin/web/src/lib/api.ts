// Thin fetch wrapper. Daemon calls go to bare paths; the dev proxy in
// vite.config.ts forwards them to the local daemon port, and in production
// the same SPA is served by the daemon itself on the same origin.
//
// Every request carries (1) the session cookie via `credentials: 'include'`
// and (2) the optional `X-Sdi-Token` header from `authHeaders()`. The two
// channels are independent: the daemon accepts whichever is present, and
// `SDI_REQUIRE_TOKEN=1` mode falls back to the header when cookies don't
// flow across the dev cross-port boundary.
//
// The daemon emits structured errors as
// `{ error: { code: string, message: string } }`. We surface that as
// `DaemonError` so views can react to specific codes (DISRUPTION_PENDING,
// EVIDENCE_REQUIRED, SCENARIOS_REQUIRED, etc.).

import { authHeaders } from './auth';
import type {
  CollaborationPattern,
  Decision,
  DecisionQuestion,
  OracleVerify,
  PatternLifecycle,
  Plan,
  Project,
  QuestionOption,
  Round,
  Scenario,
  ScenarioClaim,
  SsotNode,
  Task,
  UserFlow,
} from '../types/entities';

export interface DaemonErrorBody {
  error: { code: string; message: string };
}

export class DaemonError extends Error {
  code: string;
  status: number;

  constructor(code: string, message: string, status: number) {
    super(message);
    this.code = code;
    this.status = status;
    this.name = 'DaemonError';
  }
}

async function parseError(res: Response): Promise<DaemonError> {
  try {
    const body = (await res.json()) as DaemonErrorBody;
    if (body?.error?.code) {
      return new DaemonError(body.error.code, body.error.message, res.status);
    }
  } catch {
    // Fall through to generic error.
  }
  return new DaemonError('UNKNOWN', `${res.status} ${res.statusText}`, res.status);
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(path, {
    credentials: 'include',
    ...init,
    headers: {
      accept: 'application/json',
      ...authHeaders(),
      ...(init?.headers ?? {}),
    },
  });
  if (!res.ok) throw await parseError(res);
  if (res.status === 204) return undefined as T;
  const body = (await res.json()) as unknown;
  return unwrapListEnvelope(body) as T;
}

// Daemon list endpoints wrap results: `{ "<entity_plural>": [...] }`. The web
// client treats lists as bare arrays, so we unwrap any single-key envelope whose
// value is an array. Non-list responses (entity DTOs) carry many scalar fields
// and are returned untouched.
function unwrapListEnvelope(body: unknown): unknown {
  if (body && typeof body === 'object' && !Array.isArray(body)) {
    const keys = Object.keys(body as Record<string, unknown>);
    if (keys.length === 1) {
      const value = (body as Record<string, unknown>)[keys[0]];
      if (Array.isArray(value)) return value;
    }
  }
  return body;
}

export function getJson<T>(path: string): Promise<T> {
  return request<T>(path);
}

export function postJson<T>(path: string, body?: unknown): Promise<T> {
  return request<T>(path, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
}

export function putJson<T>(path: string, body: unknown): Promise<T> {
  return request<T>(path, {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
}

export function deleteJson<T>(path: string): Promise<T> {
  return request<T>(path, { method: 'DELETE' });
}

export function patchJson<T>(path: string, body: unknown): Promise<T> {
  return request<T>(path, {
    method: 'PATCH',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
}

// ─── Project lifecycle (v0.3) ────────────────────────────────────────────────

/** PATCH-style update body. Each field is optional; only the keys present in
 *  the patch are sent to the daemon, so callers can mix-and-match without
 *  reading back the current state first. `description: null` clears the
 *  field (distinct from omitting the key); `wiki_paths` replaces the array
 *  in full. */
export interface UpdateProjectPatch {
  name?: string;
  description?: string | null;
  enabled?: boolean;
  wiki_paths?: string[];
}

export function updateProject(
  id: string,
  patch: UpdateProjectPatch,
): Promise<Project> {
  return putJson<Project>(`/projects/${encodeURIComponent(id)}`, patch);
}

/** Soft-disable the project. Idempotent; daemon returns the fresh project
 *  row with `enabled: false`. The hook layer (`projectByCwd` consumers in
 *  the plugin shell) will skip every mutating gate for the project's
 *  anchored cwds until `enableProject` flips it back. */
export function disableProject(id: string): Promise<Project> {
  return postJson<Project>(`/projects/${encodeURIComponent(id)}/disable`);
}

/** Re-enable a previously disabled project. Idempotent. */
export function enableProject(id: string): Promise<Project> {
  return postJson<Project>(`/projects/${encodeURIComponent(id)}/enable`);
}

/** Hard-delete the project + every PROJ-scoped row across the schema.
 *  Default behaviour refuses deletion when any task under the project is
 *  `in_progress`; pass `force: true` to override. */
export function deleteProject(
  id: string,
  opts: { force?: boolean } = {},
): Promise<{ ok: true; deleted: string; forced: boolean }> {
  const qs = opts.force ? '?force=true' : '';
  return deleteJson<{ ok: true; deleted: string; forced: boolean }>(
    `/projects/${encodeURIComponent(id)}${qs}`,
  );
}

/** Attach a working directory to the project. */
export function addProjectCwd(id: string, cwd: string): Promise<{ cwds: string[] }> {
  return postJson<{ cwds: string[] }>(
    `/projects/${encodeURIComponent(id)}/cwds`,
    { cwd },
  );
}

/** Detach a working directory from the project. */
export function removeProjectCwd(
  id: string,
  cwd: string,
): Promise<{ cwds: string[]; removed: boolean }> {
  return request<{ cwds: string[]; removed: boolean }>(
    `/projects/${encodeURIComponent(id)}/cwds`,
    {
      method: 'DELETE',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ cwd }),
    },
  );
}

// ─── CollaborationPattern (D22, D26, D27) ────────────────────────────────────

/** List every pattern row scoped to a plan, oldest to newest by created_at. */
export function listPatterns(planId: string): Promise<CollaborationPattern[]> {
  return getJson<CollaborationPattern[]>(
    `/patterns?plan_id=${encodeURIComponent(planId)}`,
  );
}

/** List patterns whose lifecycle is `active` (used by the header badge).
 *  Pass `projectId` to scope through plans — the badge must agree with the
 *  current project's Patterns view, not count rows from every project. */
export function listActivePatterns(
  projectId?: string,
): Promise<CollaborationPattern[]> {
  const suffix = projectId
    ? `?project_id=${encodeURIComponent(projectId)}`
    : '';
  return getJson<CollaborationPattern[]>(`/patterns/active${suffix}`);
}

export function getPattern(id: string): Promise<CollaborationPattern> {
  return getJson<CollaborationPattern>(`/patterns/${encodeURIComponent(id)}`);
}

/** Move a pattern through its lifecycle. Daemon validates D26 shape on
 *  `pending → active` and emits a `pattern_lifecycle` SSE event on success. */
export function transitionPattern(
  id: string,
  lifecycle: PatternLifecycle,
  reason?: string,
): Promise<CollaborationPattern> {
  return patchJson<CollaborationPattern>(
    `/patterns/${encodeURIComponent(id)}/lifecycle`,
    { lifecycle, reason },
  );
}

// ─── Reversibility (D28) ─────────────────────────────────────────────────────

/** Dispatch the reversal-runner specialist. Daemon appends a new Decision
 *  row with `kind='consensus'` + `reversal_of=<id>` (D12 SNAPSHOT-ONLY: the
 *  original decision is never mutated) and emits `rollback_completed` on
 *  success or `dissensus.escalated` on failure. */
export function rollbackDecision(id: string, reason?: string): Promise<Decision> {
  return postJson<Decision>(`/decisions/${encodeURIComponent(id)}/rollback`, {
    reason,
  });
}

// ─── Resource claims (D29) ───────────────────────────────────────────────────

export interface ActiveClaimsQuery {
  planId?: string;
  path?: string;
}

/** Snapshot of scenarios currently holding `requested` or `active` claims.
 *  Daemon authoritatively detects glob overlap; the UI surfaces conflicts
 *  but does not arbitrate. */
export function getActiveClaims(
  query: ActiveClaimsQuery = {},
): Promise<ScenarioClaim[]> {
  const qs = new URLSearchParams();
  if (query.planId) qs.set('plan_id', query.planId);
  if (query.path) qs.set('path', query.path);
  const suffix = qs.toString() ? `?${qs}` : '';
  return getJson<ScenarioClaim[]>(`/scenarios/active-claims${suffix}`);
}

// ─── Scenario retirement (#8) ────────────────────────────────────────────────

/** Retire a scenario (reversible). Excludes it from verification / regression /
 *  approve count; history is preserved and the draft/confirmed status is kept. */
export function retireScenario(id: string): Promise<Scenario> {
  return postJson<Scenario>(`/scenarios/${encodeURIComponent(id)}/retire`);
}

/** Restore a retired scenario to its preserved status. */
export function unretireScenario(id: string): Promise<Scenario> {
  return postJson<Scenario>(`/scenarios/${encodeURIComponent(id)}/unretire`);
}

// ─── Autonomous-run surfaces (#15) ───────────────────────────────────────────

export interface NextStep {
  project: Project;
  active_plan: Plan | null;
  command: string;
  reason: string;
  provisional_decisions: Decision[];
}

/** The single mechanical next step for a project's active plan, computed by the
 *  daemon (`sdi next`), plus any provisional decisions to revisit. */
export function getNextStep(projectId: string): Promise<NextStep> {
  return getJson<NextStep>(`/projects/${encodeURIComponent(projectId)}/next`);
}

export interface TaskBrief {
  task: Task;
  round: Round;
  baseline: string | null;
  scenarios: Scenario[];
  evidence_format: string;
  report_schema: Record<string, string>;
  prohibitions: string[];
  complete_with: string;
}

/** A delegation brief for one task (`sdi task brief`): linked scenarios' GWT
 *  inline + round baseline + the SDI-universal evidence/report/prohibition
 *  template. */
export function getTaskBrief(taskId: string): Promise<TaskBrief> {
  return getJson<TaskBrief>(`/tasks/${encodeURIComponent(taskId)}/brief`);
}

// ─── Oracle (PRD-v2 D32/D33/D34/D35) ─────────────────────────────────────────

/** D34 — deterministic completeness verdict (L0 facet/link, L1 coverage,
 *  open-question count, L2 enforced flag, oracle_complete). The verdict object
 *  is a scalar DTO, so `request` returns it untouched (not a list envelope). */
export function getOracleVerify(projectId: string): Promise<OracleVerify> {
  return getJson<OracleVerify>(
    `/projects/${encodeURIComponent(projectId)}/oracle/verify`,
  );
}

/** L0 — SSoT graph nodes for the project. */
export function listSsotNodes(projectId: string): Promise<SsotNode[]> {
  return getJson<SsotNode[]>(
    `/projects/${encodeURIComponent(projectId)}/ssot-nodes`,
  );
}

/** L1 — UserFlow rows for the project. */
export function listUserFlows(projectId: string): Promise<UserFlow[]> {
  return getJson<UserFlow[]>(
    `/projects/${encodeURIComponent(projectId)}/user-flows`,
  );
}

/** D35 — every decision question for the project (open / answered / auto_decided). */
export function listDecisionQuestions(
  projectId: string,
): Promise<DecisionQuestion[]> {
  return getJson<DecisionQuestion[]>(
    `/projects/${encodeURIComponent(projectId)}/decision-questions`,
  );
}

/** D35 — the options of one question (the SA-exam choices). */
export function listQuestionOptions(
  questionId: string,
): Promise<QuestionOption[]> {
  return getJson<QuestionOption[]>(
    `/decision-questions/${encodeURIComponent(questionId)}/options`,
  );
}

/** D35 — answer payload. The daemon records the answer, flips question status,
 *  and (when `apply_*` is present) deterministically compiles the decision into
 *  the scoped SSoT node — closing an OPEN marker and/or replacing facets. */
export interface AnswerQuestionBody {
  chosen_option_id?: string;
  free_text?: string;
  answered_by?: string;
  /** `true` for a fact-type 1-survivor auto-decision → status `auto_decided`. */
  auto?: boolean;
  apply_node_id?: string;
  resolve_marker_id?: string;
  apply_facets_json?: string;
}

/** D35 — submit an answer to a decision question. Returns the recorded answer +
 *  the question in its new status. */
export function answerQuestion(
  questionId: string,
  body: AnswerQuestionBody,
): Promise<{ answer: unknown; question: DecisionQuestion }> {
  return postJson<{ answer: unknown; question: DecisionQuestion }>(
    `/decision-questions/${encodeURIComponent(questionId)}/answer`,
    body,
  );
}
