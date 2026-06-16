import { useEffect, useState } from 'react';
import type { Task, TaskStatus, Scenario, ScenarioResult, ScenarioEvidence } from '../types/entities';
import { getJson, postJson, DaemonError } from '../lib/api';
import { toastError, toastSuccess } from '../lib/toast';
import { Badge, Button, Input, Textarea, Label, Select } from './ui';
import { cn } from '../lib/cn';

interface TaskDetailProps {
  taskId: string;
  onClose: () => void;
  refreshKey: number;
}

const STATUS_VARIANT: Record<TaskStatus, 'default' | 'primary' | 'warning' | 'danger' | 'success'> = {
  todo: 'default',
  in_progress: 'warning',
  blocked: 'danger',
  done: 'success',
  cancelled: 'default',
};

const STATUSES: TaskStatus[] = ['todo', 'in_progress', 'blocked', 'done', 'cancelled'];
const RESULTS: ScenarioResult[] = ['passing', 'failing', 'impacted', 'retired'];

interface EvidenceRow {
  scenarioId: string;
  result: ScenarioResult;
  evidenceRef: string;
  note: string;
}

export function TaskDetail({ taskId, onClose, refreshKey }: TaskDetailProps) {
  const [task, setTask] = useState<Task | null>(null);
  const [scenarios, setScenarios] = useState<Scenario[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showEvidence, setShowEvidence] = useState(false);
  const [evidenceRows, setEvidenceRows] = useState<EvidenceRow[]>([]);
  const [summary, setSummary] = useState('');

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const t = await getJson<Task>(`/tasks/${taskId}`).catch(async () => {
          // Some daemons only expose list endpoints — fallback to filtered list.
          const list = await getJson<Task[]>(`/tasks?id=${taskId}`).catch(() => [] as Task[]);
          return list[0] ?? null;
        });
        if (!t) throw new Error('Task not found');
        // Resolve each parent scenario by id. The daemon `/scenarios` list
        // endpoint only accepts `plan_id`, so a per-id fetch against the
        // canonical `/scenarios/:id` route is the contract that exists. A
        // single missing scenario degrades to a placeholder row instead of
        // blanking the whole panel.
        const allScenarios = (
          await Promise.all(
            t.parent_scenario_ids.map((sid) =>
              getJson<Scenario>(`/scenarios/${encodeURIComponent(sid)}`).catch(
                () => null,
              ),
            ),
          )
        ).filter((s): s is Scenario => s !== null);
        if (cancelled) return;
        setTask(t);
        setScenarios(allScenarios);
        setEvidenceRows(
          t.parent_scenario_ids.map((sid) => ({
            scenarioId: sid,
            result: 'passing',
            evidenceRef: '',
            note: '',
          })),
        );
        if (t.evidence?.summary) setSummary(t.evidence.summary);
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [taskId, refreshKey]);

  async function transition(status: TaskStatus) {
    if (!task) return;
    setBusy(true);
    try {
      await postJson(`/tasks/${task.id}/status`, { status });
      toastSuccess(`Status → ${status}`);
    } catch (err) {
      if (err instanceof DaemonError) {
        if (err.code === 'EVIDENCE_REQUIRED') {
          setShowEvidence(true);
          toastError('Evidence required to complete this task.');
        } else toastError(`${err.code}: ${err.message}`);
      } else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function submitEvidence(e: React.FormEvent) {
    e.preventDefault();
    if (!task) return;
    if (evidenceRows.some((r) => !r.evidenceRef.trim())) {
      toastError('Every scenario needs an evidence ref.');
      return;
    }
    setBusy(true);
    try {
      const evidence = {
        scenarios: evidenceRows.map<ScenarioEvidence>((r) => ({
          scenario_id: r.scenarioId,
          result: r.result,
          evidence_ref: r.evidenceRef.trim(),
          note: r.note.trim() || null,
        })),
        summary: summary.trim() || null,
      };
      await postJson(`/tasks/${task.id}/complete`, { evidence });
      toastSuccess('Task completed with evidence');
      setShowEvidence(false);
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (error) return <Wrapper onClose={onClose}><p className="text-danger text-sm">{error}</p></Wrapper>;
  if (!task) return <Wrapper onClose={onClose}><p className="text-sm text-muted">Loading…</p></Wrapper>;

  return (
    <Wrapper onClose={onClose}>
      <header className="space-y-2">
        <div className="flex items-center gap-2">
          <span className="text-xs font-mono text-muted">{task.short_code}</span>
          <Badge variant={STATUS_VARIANT[task.status]} size="sm">
            {task.status}
          </Badge>
        </div>
        <p className="text-sm text-foreground whitespace-pre-wrap">{task.description}</p>
      </header>

      <section>
        <Label>Status</Label>
        <div className="mt-1.5 flex flex-wrap gap-1.5">
          {STATUSES.map((s) => (
            <button
              key={s}
              type="button"
              onClick={() => (s === 'done' ? setShowEvidence(true) : transition(s))}
              disabled={busy || s === task.status}
              className={cn(
                'rounded-md border px-2.5 py-1 text-xs cursor-pointer',
                task.status === s
                  ? 'border-primary text-primary bg-primary/10'
                  : 'border-border text-muted hover:text-foreground hover:bg-surface-hover',
                busy && 'opacity-50 cursor-wait',
              )}
            >
              {s}
            </button>
          ))}
        </div>
      </section>

      <section>
        <h3 className="text-headline-md text-foreground mb-2">Parent scenarios</h3>
        {task.parent_scenario_ids.length === 0 ? (
          <p className="text-xs text-muted italic">No parent scenarios.</p>
        ) : (
          <ul className="divide-y divide-border rounded-md border border-border bg-background">
            {task.parent_scenario_ids.map((sid) => {
              const s = scenarios.find((x) => x.id === sid);
              return (
                <li key={sid} className="px-3 py-2 text-xs space-y-1.5">
                  <span className="font-mono text-muted">{s?.short_code ?? sid}</span>
                  {s ? (
                    <dl className="space-y-1">
                      {(
                        [
                          ['Given', s.given],
                          ['When', s.when],
                          ['Then', s.then],
                        ] as const
                      ).map(([label, value]) => (
                        <div key={label} className="flex gap-2">
                          <dt className="w-12 shrink-0 text-[10px] uppercase tracking-wide text-muted pt-px">
                            {label}
                          </dt>
                          <dd className="flex-1 text-foreground whitespace-pre-wrap">
                            {value}
                          </dd>
                        </div>
                      ))}
                    </dl>
                  ) : (
                    <p className="text-muted italic">Scenario detail unavailable.</p>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </section>

      {(showEvidence || task.evidence) && (
        <section>
          <h3 className="text-headline-md text-foreground mb-2">Evidence</h3>
          {task.evidence ? (
            <div className="rounded-md border border-border bg-background p-3 text-xs space-y-2">
              {task.evidence.summary && (
                <div className="text-foreground">{task.evidence.summary}</div>
              )}
              <ul className="space-y-1">
                {task.evidence.scenarios.map((s) => (
                  <li key={s.scenario_id} className="flex items-center gap-2">
                    <Badge size="sm" variant={s.result === 'passing' ? 'success' : s.result === 'failing' ? 'danger' : 'warning'}>
                      {s.result}
                    </Badge>
                    <span className="font-mono text-muted">{s.scenario_id.slice(0, 12)}…</span>
                    <span className="text-muted">{s.evidence_ref}</span>
                  </li>
                ))}
              </ul>
            </div>
          ) : (
            <form onSubmit={submitEvidence} className="space-y-3 rounded-md border border-border bg-background p-3">
              {evidenceRows.map((row, idx) => {
                const s = scenarios.find((x) => x.id === row.scenarioId);
                return (
                  <div key={row.scenarioId} className="space-y-1.5 pb-3 border-b border-border last:border-b-0">
                    <div className="text-[10px] font-mono text-muted">{s?.short_code ?? row.scenarioId}</div>
                    <div className="grid grid-cols-2 gap-2">
                      <Select
                        value={row.result}
                        onChange={(e) =>
                          setEvidenceRows((rows) => rows.map((r, i) => (i === idx ? { ...r, result: e.target.value as ScenarioResult } : r)))
                        }
                      >
                        {RESULTS.map((r) => (
                          <option key={r} value={r}>{r}</option>
                        ))}
                      </Select>
                      <Input
                        value={row.evidenceRef}
                        onChange={(e) =>
                          setEvidenceRows((rows) => rows.map((r, i) => (i === idx ? { ...r, evidenceRef: e.target.value } : r)))
                        }
                        placeholder="evidence ref (URL, test id…)"
                      />
                    </div>
                    <Input
                      value={row.note}
                      onChange={(e) =>
                        setEvidenceRows((rows) => rows.map((r, i) => (i === idx ? { ...r, note: e.target.value } : r)))
                      }
                      placeholder="note (optional)"
                    />
                  </div>
                );
              })}
              <Textarea
                value={summary}
                onChange={(e) => setSummary(e.target.value)}
                placeholder="Summary (optional)"
                rows={3}
              />
              <div className="flex justify-end gap-2">
                <Button type="button" size="sm" variant="ghost" onClick={() => setShowEvidence(false)} disabled={busy}>
                  Cancel
                </Button>
                <Button type="submit" size="sm" disabled={busy}>
                  {busy ? 'Submitting…' : 'Complete with evidence'}
                </Button>
              </div>
            </form>
          )}
        </section>
      )}
    </Wrapper>
  );
}

function Wrapper({ onClose, children }: { onClose: () => void; children: React.ReactNode }) {
  return (
    <div className="h-full flex flex-col">
      <div className="px-5 py-3 border-b border-border flex items-center justify-between sticky top-0 bg-surface z-10">
        <span className="text-[10px] uppercase tracking-wide text-muted">Task</span>
        <button
          type="button"
          onClick={onClose}
          className="text-muted hover:text-foreground text-xl leading-none cursor-pointer"
          aria-label="Close"
        >
          ×
        </button>
      </div>
      <div className="flex-1 overflow-auto p-5 space-y-5">{children}</div>
    </div>
  );
}
