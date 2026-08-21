import { useEffect, useMemo, useState } from 'react';
import type {
  CollaborationPattern,
  PatternKind,
  PatternLifecycle,
} from '../types/entities';
import { listPatterns } from '../lib/api';
import { DaemonError } from '../lib/api';
import { toastError } from '../lib/toast';
import { Badge, Select } from './ui';
import { cn } from '../lib/cn';

interface PatternTimelineProps {
  planId: string;
  refreshKey: number;
  onSelect?: (pattern: CollaborationPattern) => void;
}

const ALL_KINDS: ReadonlyArray<PatternKind> = [
  'workflow',
  'graph',
  'swarm',
  'agents-as-tools',
  'direct',
];

const ALL_LIFECYCLES: ReadonlyArray<PatternLifecycle> = [
  'pending',
  'active',
  'converged',
  'dissensus',
  'aborted',
];

const LIFECYCLE_VARIANT: Record<PatternLifecycle, 'default' | 'primary' | 'success' | 'warning' | 'danger'> = {
  pending: 'default',
  active: 'primary',
  converged: 'success',
  dissensus: 'warning',
  aborted: 'danger',
};

const LIFECYCLE_DOT: Record<PatternLifecycle, string> = {
  pending: 'bg-muted',
  active: 'bg-primary',
  converged: 'bg-success',
  dissensus: 'bg-warning',
  aborted: 'bg-danger',
};

const KIND_VARIANT: Record<PatternKind, 'default' | 'primary' | 'success' | 'warning' | 'danger' | 'secondary' | 'info'> = {
  workflow: 'primary',
  graph: 'secondary',
  swarm: 'info',
  'agents-as-tools': 'success',
  direct: 'danger',
};

export function PatternTimeline({ planId, refreshKey, onSelect }: PatternTimelineProps) {
  const [patterns, setPatterns] = useState<CollaborationPattern[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [kindFilter, setKindFilter] = useState<PatternKind | ''>('');
  const [lifecycleFilter, setLifecycleFilter] = useState<PatternLifecycle | ''>('');

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const rows = await listPatterns(planId);
        if (cancelled) return;
        setPatterns(rows);
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

  const filtered = useMemo(() => {
    const rows = patterns
      .filter((p) => (kindFilter ? p.kind === kindFilter : true))
      .filter((p) => (lifecycleFilter ? p.lifecycle === lifecycleFilter : true));
    // Newest first — chronological timeline reads top-down.
    return rows.slice().sort((a, b) => (a.created_at < b.created_at ? 1 : -1));
  }, [patterns, kindFilter, lifecycleFilter]);

  return (
    <section className="space-y-3">
      <header className="flex items-center gap-2 flex-wrap">
        <h3 className="text-headline-md text-foreground flex items-baseline gap-2">
          Pattern timeline
          <span className="text-xs text-muted font-normal">{patterns.length}</span>
        </h3>
        <div className="ml-auto flex items-center gap-2">
          <Select
            size="sm"
            value={kindFilter}
            onChange={(e) => setKindFilter(e.target.value as PatternKind | '')}
            className="max-w-[160px]"
            aria-label="Filter by pattern kind"
          >
            <option value="">all kinds</option>
            {ALL_KINDS.map((k) => (
              <option key={k} value={k}>{k}</option>
            ))}
          </Select>
          <Select
            size="sm"
            value={lifecycleFilter}
            onChange={(e) => setLifecycleFilter(e.target.value as PatternLifecycle | '')}
            className="max-w-[160px]"
            aria-label="Filter by lifecycle"
          >
            <option value="">all lifecycles</option>
            {ALL_LIFECYCLES.map((l) => (
              <option key={l} value={l}>{l}</option>
            ))}
          </Select>
        </div>
      </header>

      {error && <p className="text-xs text-danger px-2">{error}</p>}

      {patterns.length === 0 ? (
        <div className="rounded-md border border-dashed border-border px-4 py-6 text-xs text-muted space-y-2">
          <p className="text-foreground font-medium">
            No collaboration patterns recorded for this plan yet.
          </p>
          <p>
            A CollaborationPattern is logged whenever work runs under an
            orchestration — run{' '}
            <code className="font-mono text-foreground">/pattern</code> to
            activate a <span className="font-mono">workflow</span>,{' '}
            <span className="font-mono">graph</span>,{' '}
            <span className="font-mono">swarm</span>, or{' '}
            <span className="font-mono">agents-as-tools</span> pattern.
          </p>
          <p>
            Work done solo is back-filled as a{' '}
            <span className="font-mono text-danger">direct</span> anti-pattern
            marker, so a fully populated timeline still reads honestly when no
            multi-agent collaboration was used.
          </p>
        </div>
      ) : filtered.length === 0 ? (
        <p className="text-xs italic text-muted px-2 py-6 text-center">
          No CollaborationPattern rows match the current filters.
        </p>
      ) : (
        <ol className="relative border-l border-border ml-3 space-y-2">
          {filtered.map((p) => {
            const isDirect = p.kind === 'direct';
            return (
              <li
                key={p.id}
                className={cn(
                  'ml-4 rounded-md border border-border bg-background px-3 py-2',
                  'transition-colors',
                  onSelect && 'cursor-pointer hover:bg-surface-high',
                  isDirect && 'border-l-4 border-l-danger',
                )}
                onClick={onSelect ? () => onSelect(p) : undefined}
              >
                <span
                  className={cn(
                    'absolute -left-1.5 w-3 h-3 rounded-full mt-1.5',
                    LIFECYCLE_DOT[p.lifecycle],
                  )}
                  aria-hidden
                />
                <div className="flex items-center gap-2 flex-wrap">
                  <span className="text-[10px] font-mono text-muted">{p.short_code}</span>
                  <Badge size="sm" variant={KIND_VARIANT[p.kind]}>{p.kind}</Badge>
                  <span className="text-[10px] text-muted">
                    <span className="font-mono">{p.applies_to}</span>
                    <span> · </span>
                    <span className="font-mono">{p.scope_id.slice(-8)}</span>
                  </span>
                  <span className="text-[10px] text-muted">depth={p.depth}</span>
                  <Badge size="sm" variant={LIFECYCLE_VARIANT[p.lifecycle]}>{p.lifecycle}</Badge>
                  {isDirect && (
                    <Badge size="sm" variant="danger">ANTI-PATTERN</Badge>
                  )}
                  <span className="text-[10px] text-muted ml-auto">
                    {new Date(p.created_at).toLocaleString()}
                  </span>
                </div>
                {p.decided_reason && (
                  <p className="text-xs text-foreground mt-1 italic">
                    {p.decided_reason}
                  </p>
                )}
              </li>
            );
          })}
        </ol>
      )}
    </section>
  );
}
