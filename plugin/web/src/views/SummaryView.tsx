import { useEffect, useState } from 'react';
import type { Plan, Round, Task, Scenario } from '../types/entities';
import { getJson } from '../lib/api';
import { cn } from '../lib/cn';

interface SummaryViewProps {
  projectId: string;
  refreshKey: number;
}

interface Snapshot {
  plans: Plan[];
  activePlan: Plan | null;
  activeRound: Round | null;
  scenarios: Scenario[];
  tasks: Task[];
}

export function SummaryView({ projectId, refreshKey }: SummaryViewProps) {
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const plans = await getJson<Plan[]>(`/plans?project_id=${projectId}`);
        const activePlan = plans.find((p) => p.status === 'active') ?? null;
        let activeRound: Round | null = null;
        let scenarios: Scenario[] = [];
        let tasks: Task[] = [];
        if (activePlan) {
          const [rounds, scn] = await Promise.all([
            getJson<Round[]>(`/rounds?plan_id=${activePlan.id}`).catch(() => []),
            getJson<Scenario[]>(`/scenarios?plan_id=${activePlan.id}`).catch(() => []),
          ]);
          activeRound = rounds.find((r) => r.status === 'active') ?? null;
          scenarios = scn;
          if (activeRound) {
            tasks = await getJson<Task[]>(`/tasks?round_id=${activeRound.id}`).catch(() => []);
          }
        }
        if (!cancelled) setSnap({ plans, activePlan, activeRound, scenarios, tasks });
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [projectId, refreshKey]);

  if (error) return <div className="p-6 text-danger text-sm">{error}</div>;
  if (!snap) return <div className="p-6 text-muted text-sm">Loading…</div>;

  const taskCounts = countBy(snap.tasks, (t) => t.status);
  const scenarioCounts = countBy(snap.scenarios, (s) => s.status);

  return (
    <div className="p-6 space-y-6 max-w-5xl mx-auto">
      <header>
        <h2 className="text-headline-lg text-foreground">Summary</h2>
        <p className="text-sm text-muted">
          Project snapshot — plans, active round, scenarios, tasks.
        </p>
      </header>

      <section className="grid grid-cols-4 gap-3">
        <Card label="Plans" value={String(snap.plans.length)} accent="primary" />
        <Card
          label="Active plan"
          value={snap.activePlan ? snap.activePlan.short_code : '—'}
          subtitle={snap.activePlan?.title ?? 'No active plan'}
          accent={snap.activePlan ? 'success' : 'muted'}
        />
        <Card
          label="Active round"
          value={snap.activeRound ? snap.activeRound.short_code : '—'}
          subtitle={snap.activeRound ? snap.activeRound.mode : 'No active round'}
          accent={snap.activeRound ? 'warning' : 'muted'}
        />
        <Card
          label="Scenarios"
          value={String(snap.scenarios.length)}
          subtitle={`${scenarioCounts.get('confirmed') ?? 0} confirmed / ${scenarioCounts.get('draft') ?? 0} draft`}
          accent="secondary"
        />
      </section>

      <section>
        <h3 className="text-headline-md text-foreground mb-2">Round tasks</h3>
        {snap.activeRound ? (
          <div className="grid grid-cols-5 gap-2">
            <MiniStat label="Todo" value={taskCounts.get('todo') ?? 0} color="text-muted" />
            <MiniStat label="In progress" value={taskCounts.get('in_progress') ?? 0} color="text-warning" />
            <MiniStat label="Blocked" value={taskCounts.get('blocked') ?? 0} color="text-danger" />
            <MiniStat label="Done" value={taskCounts.get('done') ?? 0} color="text-success" />
            <MiniStat label="Cancelled" value={taskCounts.get('cancelled') ?? 0} color="text-muted" />
          </div>
        ) : (
          <div className="rounded-md border border-border bg-surface p-6 text-center text-sm text-muted">
            No active round — activate one to start tracking tasks.
          </div>
        )}
      </section>

      <section>
        <h3 className="text-headline-md text-foreground mb-2">Plans</h3>
        <ul className="divide-y divide-border rounded-md border border-border bg-surface">
          {snap.plans.length === 0 ? (
            <li className="px-4 py-3 text-sm text-muted">No plans yet.</li>
          ) : (
            snap.plans.map((p) => (
              <li
                key={p.id}
                className="flex items-center gap-3 px-4 py-2"
              >
                <span className="text-xs font-mono text-muted w-20 shrink-0">{p.short_code}</span>
                <span className="flex-1 min-w-0 truncate text-sm text-foreground">{p.title}</span>
                <span
                  className={cn(
                    'text-[10px] uppercase tracking-wide px-2 py-0.5 rounded',
                    p.status === 'active' && 'bg-primary/15 text-primary',
                    p.status === 'draft' && 'bg-muted/15 text-muted',
                    p.status === 'completed' && 'bg-success/15 text-success',
                  )}
                >
                  {p.status}
                </span>
              </li>
            ))
          )}
        </ul>
      </section>
    </div>
  );
}

interface CardProps {
  label: string;
  value: string;
  subtitle?: string;
  accent: 'primary' | 'success' | 'warning' | 'muted' | 'secondary';
}

function Card({ label, value, subtitle, accent }: CardProps) {
  const accentClass: Record<CardProps['accent'], string> = {
    primary: 'text-primary',
    success: 'text-success',
    warning: 'text-warning',
    muted: 'text-muted',
    secondary: 'text-secondary',
  };
  return (
    <div className="rounded-md border border-border bg-surface p-4">
      <div className="text-[10px] uppercase tracking-wide text-muted">{label}</div>
      <div className={cn('mt-1 text-headline-md font-semibold', accentClass[accent])}>
        {value}
      </div>
      {subtitle && (
        <div className="mt-1 text-xs text-muted truncate" title={subtitle}>
          {subtitle}
        </div>
      )}
    </div>
  );
}

function MiniStat({ label, value, color }: { label: string; value: number; color: string }) {
  return (
    <div className="rounded-md border border-border bg-surface p-3 text-center">
      <div className={cn('text-headline-md font-semibold', color)}>{value}</div>
      <div className="text-[10px] uppercase tracking-wide text-muted">{label}</div>
    </div>
  );
}

function countBy<T, K>(items: T[], key: (t: T) => K): Map<K, number> {
  const m = new Map<K, number>();
  for (const t of items) {
    const k = key(t);
    m.set(k, (m.get(k) ?? 0) + 1);
  }
  return m;
}
