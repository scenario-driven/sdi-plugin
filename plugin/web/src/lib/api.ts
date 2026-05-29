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
  PatternLifecycle,
  ScenarioClaim,
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

// ─── CollaborationPattern (D22, D26, D27) ────────────────────────────────────

/** List every pattern row scoped to a plan, oldest to newest by created_at. */
export function listPatterns(planId: string): Promise<CollaborationPattern[]> {
  return getJson<CollaborationPattern[]>(
    `/patterns?plan_id=${encodeURIComponent(planId)}`,
  );
}

/** List patterns whose lifecycle is `active` (used by the header badge). */
export function listActivePatterns(): Promise<CollaborationPattern[]> {
  return getJson<CollaborationPattern[]>('/patterns/active');
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
