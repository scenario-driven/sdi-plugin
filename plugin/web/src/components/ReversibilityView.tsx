import { useEffect, useState, useMemo } from 'react';
import type { Decision, ReversalPlan } from '../types/entities';
import { DaemonError, getJson, rollbackDecision } from '../lib/api';
import { toastError, toastSuccess } from '../lib/toast';
import { Badge, Button } from './ui';
import { cn } from '../lib/cn';

interface ReversibilityViewProps {
  planId: string;
  refreshKey: number;
  /** D28 — AutonomyPolicy.l5_threshold (0–10, default 5). The L5 unlock
   *  matrix reads against this; the daemon is still authoritative. */
  l5Threshold?: number;
  /** When provided, lets a parent open a full Decision detail drawer. */
  onSelectDecision?: (decisionId: string) => void;
}

export function ReversibilityView({
  planId,
  refreshKey,
  l5Threshold = 5,
  onSelectDecision,
}: ReversibilityViewProps) {
  const [decisions, setDecisions] = useState<Decision[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const rows = await getJson<Decision[]>(
          `/decisions?plan_id=${encodeURIComponent(planId)}`,
        );
        if (cancelled) return;
        setDecisions(rows);
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

  const sorted = useMemo(
    () => decisions.slice().sort((a, b) => (a.created_at < b.created_at ? 1 : -1)),
    [decisions],
  );

  return (
    <section className="space-y-3">
      <header className="flex items-center gap-2">
        <h3 className="text-headline-md text-foreground flex items-baseline gap-2">
          Reversibility
          <span className="text-xs text-muted font-normal">{decisions.length}</span>
        </h3>
        <span className="text-[10px] text-muted ml-auto">
          L5 threshold ≤ {l5Threshold}
        </span>
      </header>

      {error && <p className="text-xs text-danger px-2">{error}</p>}

      {sorted.length === 0 ? (
        <p className="text-xs italic text-muted px-2 py-6 text-center">
          No decisions on this plan yet.
        </p>
      ) : (
        <ul className="space-y-2">
          {sorted.map((d) => (
            <DecisionRow
              key={d.id}
              decision={d}
              l5Threshold={l5Threshold}
              onSelect={onSelectDecision}
            />
          ))}
        </ul>
      )}
    </section>
  );
}

interface DecisionRowProps {
  decision: Decision;
  l5Threshold: number;
  onSelect?: (decisionId: string) => void;
}

function DecisionRow({ decision, l5Threshold, onSelect }: DecisionRowProps) {
  const [busy, setBusy] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const score = decision.blast_radius_score ?? null;
  const plan = decision.reversal_plan ?? null;
  const isRollback = !!decision.reversal_of;

  async function doRollback(e: React.MouseEvent) {
    e.stopPropagation();
    if (!plan || isRollback) return;
    const reason = window.prompt(
      `Rollback ${decision.short_code}. Reason (optional):`,
      '',
    );
    setBusy(true);
    try {
      await rollbackDecision(decision.id, reason?.trim() || undefined);
      toastSuccess(`Rollback dispatched for ${decision.short_code}`);
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <li
      className={cn(
        'rounded-md border border-border bg-background px-3 py-2 space-y-2',
        onSelect && 'cursor-pointer hover:bg-surface-high',
      )}
      onClick={onSelect ? () => onSelect(decision.id) : undefined}
    >
      <div className="flex items-center gap-2 flex-wrap">
        <span className="text-[10px] font-mono text-muted">{decision.short_code}</span>
        <Badge size="sm" variant="primary">{decision.kind}</Badge>
        {isRollback && <Badge size="sm" variant="warning">rollback</Badge>}
        <span className="text-xs font-medium text-foreground truncate" title={decision.title}>
          {decision.title}
        </span>
        <span className="text-[10px] text-muted ml-auto">
          {new Date(decision.created_at).toLocaleString()}
        </span>
      </div>

      <BlastRadiusBar score={score} />

      <L5UnlockMatrix
        decision={decision}
        l5Threshold={l5Threshold}
      />

      <div className="flex items-center gap-2">
        {plan ? (
          <>
            <Badge size="sm" variant="success">{plan.type}</Badge>
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                setExpanded((v) => !v);
              }}
              className="text-[10px] text-primary hover:underline cursor-pointer"
            >
              {expanded ? 'hide plan' : 'show plan'}
            </button>
          </>
        ) : (
          <Badge size="sm" variant="default">no reversal_plan</Badge>
        )}
        <Button
          size="sm"
          variant="outline"
          onClick={doRollback}
          disabled={busy || !plan || isRollback}
          className="ml-auto"
          title={
            !plan
              ? 'Decision has no reversal_plan — rollback unavailable'
              : isRollback
                ? 'This row is itself a rollback'
                : 'Dispatch reversal-runner specialist'
          }
        >
          {busy ? '…' : '↺ Rollback'}
        </Button>
      </div>

      {expanded && plan && (
        <pre className="text-[10px] font-mono bg-surface-low rounded p-2 overflow-auto max-h-48 whitespace-pre-wrap break-all">
          {formatReversalPlan(plan)}
        </pre>
      )}
    </li>
  );
}

function BlastRadiusBar({ score }: { score: number | null }) {
  if (score === null || score === undefined) {
    return (
      <div className="text-[10px] text-muted italic">
        blast_radius_score not set
      </div>
    );
  }
  const clamped = Math.max(0, Math.min(10, score));
  const widthPct = (clamped / 10) * 100;
  const tone =
    clamped <= 3 ? 'bg-success' : clamped <= 6 ? 'bg-warning' : 'bg-danger';
  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between text-[10px]">
        <span className="text-muted">blast_radius_score</span>
        <span className="font-mono text-foreground">{clamped} / 10</span>
      </div>
      <div className="h-1.5 w-full rounded-full bg-surface-low overflow-hidden">
        <div
          className={cn('h-full rounded-full transition-all', tone)}
          style={{ width: `${widthPct}%` }}
        />
      </div>
    </div>
  );
}

interface UnlockCheck {
  label: string;
  ok: boolean;
  tip: string;
}

function L5UnlockMatrix({
  decision,
  l5Threshold,
}: {
  decision: Decision;
  l5Threshold: number;
}) {
  const score = decision.blast_radius_score ?? null;
  const plan = decision.reversal_plan ?? null;
  // Pattern shape validity is owned by the daemon — the UI reflects whether
  // the row carries a `produced_via_pattern_id` at all (D23 NOT NULL post
  // migration) and treats absence as a failed shape gate.
  const patternShapeOk = !!decision.produced_via_pattern_id;
  const reversalOk = !!plan && isReversalPlanWellFormed(plan);
  const blastOk = score !== null && score <= l5Threshold;
  const checks: UnlockCheck[] = [
    {
      label: 'pattern shape valid',
      ok: patternShapeOk,
      tip: 'D26/D27: produced_via_pattern_id must reference a row whose lifecycle reached active via shape validation.',
    },
    {
      label: 'reversal_plan present',
      ok: reversalOk,
      tip: 'D28: reversal_plan JSON must parse and carry the required fields for its type.',
    },
    {
      label: `blast ≤ ${l5Threshold}`,
      ok: blastOk,
      tip: 'D28: blast_radius_score must be ≤ AutonomyPolicy.l5_threshold for L5 auto-apply.',
    },
  ];
  return (
    <div className="rounded border border-border-muted bg-surface-low px-2 py-1.5 space-y-1">
      <div className="text-[10px] uppercase tracking-wide text-muted">
        L5 unlock
      </div>
      <ul className="grid grid-cols-3 gap-1">
        {checks.map((c) => (
          <li
            key={c.label}
            title={c.tip}
            className={cn(
              'flex items-center gap-1 text-[10px]',
              c.ok ? 'text-success' : 'text-danger',
            )}
          >
            <span aria-hidden>{c.ok ? '✓' : '✗'}</span>
            <span className="truncate">{c.label}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function isReversalPlanWellFormed(plan: ReversalPlan): boolean {
  switch (plan.type) {
    case 'migration_sql':
      return !!plan.sql;
    case 'git_revert':
      return !!plan.sha;
    case 'fs_snapshot':
      return !!plan.snapshot_ref;
    case 'compensating_action':
      return !!plan.action_spec;
    default:
      return false;
  }
}

function formatReversalPlan(plan: ReversalPlan): string {
  try {
    return JSON.stringify(plan, null, 2);
  } catch {
    return String(plan);
  }
}
