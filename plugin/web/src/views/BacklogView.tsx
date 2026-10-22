import { useEffect, useMemo, useState } from 'react';
import type { Plan, Task } from '../types/entities';
import { getJson } from '../lib/api';
import { cn } from '../lib/cn';

interface BacklogViewProps {
  projectId: string;
  refreshKey: number;
  onSelectTask: (id: string) => void;
}

interface Row {
  plan: Plan;
  tasks: Task[];
}

const STATUS_GLYPH: Record<Task['status'], string> = {
  todo: '○',
  in_progress: '◐',
  blocked: '⊘',
  done: '●',
  cancelled: '✕',
};

const STATUS_COLOR: Record<Task['status'], string> = {
  todo: 'text-muted',
  in_progress: 'text-warning',
  blocked: 'text-danger',
  done: 'text-success',
  cancelled: 'text-muted',
};

export function BacklogView({ projectId, refreshKey, onSelectTask }: BacklogViewProps) {
  const [rows, setRows] = useState<Row[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<'all' | Task['status']>('all');
  const [query, setQuery] = useState('');

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const plans = await getJson<Plan[]>(`/plans?project_id=${projectId}`);
        const result: Row[] = await Promise.all(
          plans.map(async (plan) => {
            // Backlog = all non-terminal tasks across rounds (todo + in_progress +
            // blocked). Distinct from `/tasks/in-flight` which is in_progress-only
            // for round disruption decisions. Fall back to per-round listing when
            // the daemon predates the backlog endpoint.
            const tasks = await getJson<Task[]>(`/plans/${plan.id}/tasks/backlog`).catch(
              async () => {
                const rounds = await getJson<{ id: string }[]>(`/rounds?plan_id=${plan.id}`).catch(() => []);
                const allTasks = await Promise.all(
                  rounds.map((r) =>
                    getJson<Task[]>(`/tasks?round_id=${r.id}`).catch(() => []),
                  ),
                );
                return allTasks
                  .flat()
                  .filter((t) => t.status !== 'done' && t.status !== 'cancelled');
              },
            );
            return { plan, tasks };
          }),
        );
        if (!cancelled) setRows(result);
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [projectId, refreshKey]);

  const filtered = useMemo(() => {
    return rows.map((r) => ({
      plan: r.plan,
      tasks: r.tasks.filter((t) => {
        if (filter !== 'all' && t.status !== filter) return false;
        if (query) {
          const needle = query.toLowerCase();
          if (!t.description.toLowerCase().includes(needle) && !t.short_code.toLowerCase().includes(needle)) return false;
        }
        return true;
      }),
    }));
  }, [rows, filter, query]);

  if (error) return <div className="p-6 text-danger text-sm">{error}</div>;

  return (
    <div className="h-full flex flex-col">
      <header className="px-6 pt-5 pb-3 border-b border-border flex items-baseline gap-3">
        <h2 className="text-headline-lg text-foreground">Backlog</h2>
        <input
          type="search"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Search tasks…"
          className="ml-auto w-72 rounded-md border border-border bg-background px-3 py-1.5 text-sm text-foreground placeholder:text-muted focus:outline-none focus:border-primary"
        />
        <select
          value={filter}
          onChange={(e) => setFilter(e.target.value as typeof filter)}
          className="rounded-md border border-border bg-background px-3 py-1.5 text-sm text-foreground focus:outline-none focus:border-primary"
        >
          <option value="all">All</option>
          <option value="todo">Todo</option>
          <option value="in_progress">In progress</option>
          <option value="blocked">Blocked</option>
          <option value="done">Done</option>
          <option value="cancelled">Cancelled</option>
        </select>
      </header>
      <div className="flex-1 overflow-auto p-6 space-y-6">
        {filtered.map((row) => (
          <section key={row.plan.id}>
            <header className="flex items-baseline gap-3 mb-2">
              <span className="text-xs font-mono text-muted">{row.plan.short_code}</span>
              <h3 className="text-headline-md text-foreground">{row.plan.title}</h3>
              <span className="text-xs text-muted">{row.tasks.length} tasks</span>
            </header>
            {row.tasks.length === 0 ? (
              <p className="text-xs text-muted italic px-2">No tasks matching the current filter.</p>
            ) : (
              <ul className="divide-y divide-border rounded-md border border-border bg-surface">
                {row.tasks.map((t) => (
                  <li key={t.id}>
                    <button
                      type="button"
                      onClick={() => onSelectTask(t.id)}
                      className="w-full flex items-center gap-3 px-4 py-2 text-left hover:bg-surface-high transition-colors cursor-pointer"
                    >
                      <span className={cn('w-3 text-center', STATUS_COLOR[t.status])}>
                        {STATUS_GLYPH[t.status]}
                      </span>
                      <span className="text-xs font-mono text-muted w-20 shrink-0">{t.short_code}</span>
                      <span className="flex-1 min-w-0 truncate text-sm text-foreground">{t.description}</span>
                      <span className="text-[10px] uppercase tracking-wide text-muted">{t.status}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </section>
        ))}
        {filtered.length === 0 && (
          <p className="text-sm text-muted text-center py-12">No plans in this project.</p>
        )}
      </div>
    </div>
  );
}
