import { useEffect, useState } from 'react';
import type { Plan, Round, Task, Scenario, Decision } from '../types/entities';
import { getJson } from '../lib/api';
import { cn } from '../lib/cn';

interface TimelineViewProps {
  projectId: string;
  refreshKey: number;
}

interface TimelineEntry {
  ts: string;
  kind: 'plan' | 'round' | 'task' | 'scenario' | 'decision';
  label: string;
  code: string;
  detail: string;
}

const KIND_COLOR: Record<TimelineEntry['kind'], string> = {
  plan: 'bg-primary',
  round: 'bg-warning',
  task: 'bg-success',
  scenario: 'bg-secondary',
  decision: 'bg-muted',
};

export function TimelineView({ projectId, refreshKey }: TimelineViewProps) {
  const [entries, setEntries] = useState<TimelineEntry[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const plans = await getJson<Plan[]>(`/plans?project_id=${projectId}`);
        const all: TimelineEntry[] = [];
        for (const p of plans) {
          all.push({
            ts: p.created_at,
            kind: 'plan',
            label: 'plan created',
            code: p.short_code,
            detail: p.title,
          });
          if (p.approved_at)
            all.push({
              ts: p.approved_at,
              kind: 'plan',
              label: 'plan approved',
              code: p.short_code,
              detail: p.title,
            });
          if (p.completed_at)
            all.push({
              ts: p.completed_at,
              kind: 'plan',
              label: 'plan completed',
              code: p.short_code,
              detail: p.title,
            });
          const [rounds, scenarios, decisions] = await Promise.all([
            getJson<Round[]>(`/rounds?plan_id=${p.id}`).catch(() => []),
            getJson<Scenario[]>(`/scenarios?plan_id=${p.id}`).catch(() => []),
            getJson<Decision[]>(`/decisions?plan_id=${p.id}`).catch(() => []),
          ]);
          for (const r of rounds) {
            all.push({
              ts: r.created_at,
              kind: 'round',
              label: 'round created',
              code: r.short_code,
              detail: `${p.short_code} · ${r.mode}`,
            });
            if (r.activated_at)
              all.push({
                ts: r.activated_at,
                kind: 'round',
                label: 'round activated',
                code: r.short_code,
                detail: `${p.short_code} · ${r.mode}`,
              });
            if (r.completed_at)
              all.push({
                ts: r.completed_at,
                kind: 'round',
                label: 'round completed',
                code: r.short_code,
                detail: `${p.short_code} · ${r.mode}`,
              });
            const tasks = await getJson<Task[]>(`/tasks?round_id=${r.id}`).catch(() => []);
            for (const t of tasks) {
              all.push({
                ts: t.created_at,
                kind: 'task',
                label: 'task created',
                code: t.short_code,
                detail: t.description.slice(0, 80),
              });
              if (t.evidence_at)
                all.push({
                  ts: t.evidence_at,
                  kind: 'task',
                  label: 'task evidence recorded',
                  code: t.short_code,
                  detail: t.description.slice(0, 80),
                });
            }
          }
          for (const s of scenarios) {
            all.push({
              ts: s.created_at,
              kind: 'scenario',
              label: 'scenario added',
              code: s.short_code,
              detail: s.when,
            });
          }
          for (const d of decisions) {
            all.push({
              ts: d.created_at,
              kind: 'decision',
              label: d.status === 'superseded' ? 'decision superseded' : 'decision recorded',
              code: d.short_code,
              detail: d.title,
            });
          }
        }
        all.sort((a, b) => (a.ts < b.ts ? 1 : a.ts > b.ts ? -1 : 0));
        if (!cancelled) setEntries(all);
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [projectId, refreshKey]);

  if (error) return <div className="p-6 text-danger text-sm">{error}</div>;

  return (
    <div className="h-full flex flex-col">
      <header className="px-6 pt-5 pb-3 border-b border-border">
        <h2 className="text-headline-lg text-foreground">Timeline</h2>
        <p className="text-sm text-muted">Project activity, newest first.</p>
      </header>
      <div className="flex-1 overflow-auto p-6">
        {entries.length === 0 ? (
          <p className="text-sm text-muted text-center py-12">No activity yet.</p>
        ) : (
          <ol className="relative border-l border-border ml-3 space-y-3">
            {entries.map((e, i) => (
              <li key={`${e.ts}-${i}`} className="ml-4">
                <span className={cn('absolute -left-1.5 w-3 h-3 rounded-full mt-1.5', KIND_COLOR[e.kind])} />
                <time className="text-[10px] uppercase tracking-wide text-muted block">
                  {new Date(e.ts).toLocaleString()}
                </time>
                <p className="text-sm text-foreground">
                  <span className="font-mono text-xs text-muted mr-2">{e.code}</span>
                  {e.label}
                  <span className="text-muted"> · {e.detail}</span>
                </p>
              </li>
            ))}
          </ol>
        )}
      </div>
    </div>
  );
}
