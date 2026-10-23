import { useEffect, useMemo, useState } from 'react';
import type { ClaimStatus, ScenarioClaim } from '../types/entities';
import { DaemonError, getActiveClaims } from '../lib/api';
import { toastError } from '../lib/toast';
import { Badge, Select } from './ui';
import { cn } from '../lib/cn';

interface ClaimsLedgerProps {
  /** When provided, scopes the ledger to a single plan. Omit to see all
   *  plans the daemon is currently tracking. */
  planId?: string;
  refreshKey: number;
}

const STATUS_VARIANT: Record<ClaimStatus, 'default' | 'primary' | 'success' | 'warning' | 'danger'> = {
  none: 'default',
  requested: 'warning',
  active: 'success',
  released: 'default',
};

const ALL_STATUSES: ReadonlyArray<ClaimStatus> = [
  'none',
  'requested',
  'active',
  'released',
];

export function ClaimsLedger({ planId, refreshKey }: ClaimsLedgerProps) {
  const [claims, setClaims] = useState<ScenarioClaim[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [statusFilter, setStatusFilter] = useState<ClaimStatus | ''>('');

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const rows = await getActiveClaims({ planId });
        if (cancelled) return;
        setClaims(rows);
      } catch (err) {
        if (cancelled) return;
        if (err instanceof DaemonError) {
          toastError(`${err.code}: ${err.message}`);
          setError(err.message);
        } else {
          setError(err instanceof Error ? err.message : String(err));
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [planId, refreshKey]);

  const filtered = useMemo(
    () =>
      claims
        .filter((c) => (statusFilter ? c.claim_status === statusFilter : true))
        .slice()
        .sort((a, b) => a.short_code.localeCompare(b.short_code)),
    [claims, statusFilter],
  );

  // Defensive overlap detection: daemon already prevents two `active` claims
  // overlapping on the same resource glob, but if a malformed payload sneaks
  // through (or a future bug regresses this invariant) the UI flags it.
  const overlapByResource = useMemo(() => detectOverlap(claims), [claims]);
  const conflicts = useMemo(
    () =>
      Array.from(overlapByResource.entries())
        .filter(([, scenarios]) => scenarios.length > 1)
        .map(([resource, scenarios]) => ({ resource, scenarios })),
    [overlapByResource],
  );

  return (
    <section className="space-y-3">
      <header className="flex items-center gap-2 flex-wrap">
        <h3 className="text-headline-md text-foreground flex items-baseline gap-2">
          Claims ledger
          <span className="text-xs text-muted font-normal">{claims.length}</span>
        </h3>
        <div className="ml-auto flex items-center gap-2">
          <Select
            size="sm"
            value={statusFilter}
            onChange={(e) => setStatusFilter(e.target.value as ClaimStatus | '')}
            className="max-w-[160px]"
            aria-label="Filter by claim status"
          >
            <option value="">all statuses</option>
            {ALL_STATUSES.map((s) => (
              <option key={s} value={s}>{s}</option>
            ))}
          </Select>
        </div>
      </header>

      {error && <p className="text-xs text-danger px-2">{error}</p>}

      {conflicts.length > 0 && (
        <div className="rounded-md border border-danger/40 bg-danger/5 px-3 py-2 space-y-1">
          <div className="text-[11px] font-semibold text-danger uppercase tracking-wide">
            Cross-claim overlap detected ({conflicts.length})
          </div>
          <ul className="space-y-0.5">
            {conflicts.map((c) => (
              <li key={c.resource} className="text-[11px] text-foreground">
                <span className="font-mono">{c.resource}</span>{' '}
                <span className="text-muted">
                  claimed by {c.scenarios.map((s) => s.short_code).join(', ')}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {filtered.length === 0 ? (
        <p className="text-xs italic text-muted px-2 py-6 text-center">
          No active claims{statusFilter ? ` with status ${statusFilter}` : ''}.
        </p>
      ) : (
        <div className="overflow-auto rounded-md border border-border">
          <table className="w-full text-xs">
            <thead className="bg-surface-low text-muted text-[10px] uppercase tracking-wide">
              <tr>
                <th className="text-left px-3 py-2 font-medium">Scenario</th>
                <th className="text-left px-3 py-2 font-medium">Status</th>
                <th className="text-left px-3 py-2 font-medium">Resources</th>
                <th className="text-left px-3 py-2 font-medium">Holder session</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-border">
              {filtered.map((c) => {
                const rowConflict = c.claimed_resources.some((r) =>
                  (overlapByResource.get(r)?.length ?? 0) > 1,
                );
                return (
                  <tr
                    key={c.scenario_id}
                    className={cn(rowConflict && 'bg-danger/5')}
                  >
                    <td className="px-3 py-2 font-mono text-foreground">
                      {c.short_code}
                    </td>
                    <td className="px-3 py-2">
                      <Badge size="sm" variant={STATUS_VARIANT[c.claim_status]}>
                        {c.claim_status}
                      </Badge>
                    </td>
                    <td className="px-3 py-2">
                      <div className="flex flex-wrap gap-1">
                        {c.claimed_resources.length === 0 ? (
                          <span className="text-muted italic text-[10px]">
                            (none)
                          </span>
                        ) : (
                          c.claimed_resources.map((r) => (
                            <span
                              key={r}
                              className="font-mono text-[10px] bg-surface-low border border-border rounded px-1.5 py-0.5"
                              title={r}
                            >
                              {r}
                            </span>
                          ))
                        )}
                      </div>
                    </td>
                    <td className="px-3 py-2 font-mono text-[10px] text-muted">
                      {c.holder_session_id ?? '—'}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

/** Map every claimed resource to the list of scenarios that hold it in an
 *  effectful (`requested` | `active`) state. Multiple scenarios on the same
 *  resource indicates daemon invariant violation worth surfacing. */
function detectOverlap(claims: ScenarioClaim[]): Map<string, ScenarioClaim[]> {
  const map = new Map<string, ScenarioClaim[]>();
  for (const c of claims) {
    if (c.claim_status !== 'requested' && c.claim_status !== 'active') continue;
    for (const r of c.claimed_resources) {
      const list = map.get(r);
      if (list) list.push(c);
      else map.set(r, [c]);
    }
  }
  return map;
}
