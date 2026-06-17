import { useEffect, useState } from 'react';
import type { NextStep, TaskBrief } from '../lib/api';
import { getNextStep, getTaskBrief } from '../lib/api';
import { Badge } from '../components/ui';
import { cn } from '../lib/cn';

interface NextViewProps {
  projectId: string;
  refreshKey: number;
}

/** #15 — `sdi next` + `sdi task brief` surfaced in the dashboard: the single
 *  mechanical next step, provisional decisions to revisit, and the in-progress
 *  task's delegation brief. The daemon computes everything; this is a view. */
export function NextView({ projectId, refreshKey }: NextViewProps) {
  const [next, setNext] = useState<NextStep | null>(null);
  const [brief, setBrief] = useState<TaskBrief | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const n = await getNextStep(projectId);
        if (cancelled) return;
        setNext(n);
        // If the next step targets an in-progress task brief, fetch it.
        const m = n.command.match(/sdi task brief (\S+)/);
        if (m) {
          const b = await getTaskBrief(m[1]).catch(() => null);
          if (!cancelled) setBrief(b);
        } else {
          setBrief(null);
        }
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [projectId, refreshKey]);

  return (
    <div className="h-full flex flex-col">
      <header className="px-6 pt-5 pb-3 border-b border-border">
        <h2 className="text-headline-lg text-foreground">Next</h2>
        <p className="text-sm text-muted">
          The daemon-computed next mechanical step, decisions to revisit, and the
          active task's delegation brief.
        </p>
      </header>

      <div className="flex-1 overflow-auto p-6 space-y-6">
        {error && <p className="text-sm text-danger">{error}</p>}
        {!error && !next && <p className="text-sm text-muted">Loading…</p>}

        {next && (
          <>
            <section className="rounded-lg border border-border bg-surface p-4 space-y-2">
              <div className="text-[10px] uppercase tracking-wide text-muted">Next step</div>
              <pre className="text-sm font-mono text-foreground whitespace-pre-wrap break-all bg-background rounded-md border border-border p-3">
                {next.command}
              </pre>
              <p className="text-sm text-muted">{next.reason}</p>
            </section>

            {next.provisional_decisions.length > 0 && (
              <section className="space-y-2">
                <div className="text-[10px] uppercase tracking-wide text-muted">
                  Provisional decisions to revisit
                </div>
                <ul className="space-y-2">
                  {next.provisional_decisions.map((d) => (
                    <li
                      key={d.id}
                      className="rounded-md border border-warning/40 bg-warning/5 px-3 py-2"
                    >
                      <div className="flex items-center gap-2 flex-wrap">
                        <Badge size="sm" variant="warning">provisional</Badge>
                        <span className="text-[10px] font-mono text-muted">{d.short_code}</span>
                        <span className="text-xs font-medium text-foreground">{d.title}</span>
                      </div>
                      {d.supersede_when && (
                        <div className="text-[11px] text-warning mt-1">
                          revisit when: {d.supersede_when}
                        </div>
                      )}
                    </li>
                  ))}
                </ul>
              </section>
            )}

            {brief && <BriefCard brief={brief} />}
          </>
        )}
      </div>
    </div>
  );
}

function BriefCard({ brief }: { brief: TaskBrief }) {
  return (
    <section className="rounded-lg border border-border bg-surface p-4 space-y-4">
      <div className="flex items-center gap-2 flex-wrap">
        <div className="text-[10px] uppercase tracking-wide text-muted">Task brief</div>
        <span className="text-[10px] font-mono text-muted">{brief.task.short_code}</span>
        <Badge size="sm" variant="default">{brief.task.status}</Badge>
      </div>
      <div className="text-sm text-foreground">{brief.task.description}</div>

      <div className="space-y-2">
        <div className="text-[10px] uppercase tracking-wide text-muted">
          Linked scenarios ({brief.scenarios.length})
        </div>
        {brief.scenarios.map((s) => (
          <div key={s.id} className="rounded-md border border-border bg-background p-3 space-y-1">
            <span className="text-[10px] font-mono text-muted">{s.short_code}</span>
            <Gwt label="Given" value={s.given} />
            <Gwt label="When" value={s.when} />
            <Gwt label="Then" value={s.then} />
          </div>
        ))}
      </div>

      {brief.baseline && (
        <Row label="Verification baseline">
          <code className="text-foreground">{brief.baseline}</code>
        </Row>
      )}
      <Row label="Evidence format">
        <code className="text-foreground break-all">{brief.evidence_format}</code>
      </Row>
      <Row label="Complete with">
        <code className="text-foreground break-all">{brief.complete_with}</code>
      </Row>
      <div className="space-y-1">
        <div className="text-[10px] uppercase tracking-wide text-muted">Prohibitions</div>
        <ul className="list-disc list-inside text-[11px] text-muted space-y-0.5">
          {brief.prohibitions.map((p, i) => (
            <li key={i}>{p}</li>
          ))}
        </ul>
      </div>
    </section>
  );
}

function Gwt({ label, value }: { label: string; value: string }) {
  return (
    <div className="text-xs">
      <span className="text-[10px] uppercase tracking-wide text-muted mr-1">{label}</span>
      <span className="text-foreground">{value}</span>
    </div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className={cn('text-[11px] text-muted')}>
      <span className="uppercase tracking-wide mr-2">{label}</span>
      {children}
    </div>
  );
}
