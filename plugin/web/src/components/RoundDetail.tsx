import { useEffect, useState } from 'react';
import type { Round, Task, ScenarioResultRow } from '../types/entities';
import { getJson, postJson, DaemonError } from '../lib/api';
import { toastError, toastSuccess } from '../lib/toast';
import { Badge, Button } from './ui';

interface RoundDetailProps {
  roundId: string;
  onClose: () => void;
  refreshKey: number;
}

const STATUS_VARIANT: Record<Round['status'], 'default' | 'primary' | 'success'> = {
  planning: 'default',
  active: 'primary',
  completed: 'success',
};

const RESULT_VARIANT: Record<ScenarioResultRow['result'], 'success' | 'danger' | 'warning' | 'default'> = {
  passing: 'success',
  failing: 'danger',
  impacted: 'warning',
  retired: 'default',
};

export function RoundDetail({ roundId, onClose, refreshKey }: RoundDetailProps) {
  const [round, setRound] = useState<Round | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [results, setResults] = useState<ScenarioResultRow[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r = await getJson<Round>(`/rounds/${roundId}`).catch(async () => {
          const list = await getJson<Round[]>(`/rounds?id=${roundId}`).catch(() => [] as Round[]);
          return list[0] ?? null;
        });
        if (!r) throw new Error('Round not found');
        const [ts, rs] = await Promise.all([
          getJson<Task[]>(`/tasks?round_id=${roundId}`).catch(() => []),
          getJson<ScenarioResultRow[]>(`/rounds/${roundId}/results`).catch(() => []),
        ]);
        if (cancelled) return;
        setRound(r);
        setTasks(ts);
        setResults(rs);
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [roundId, refreshKey]);

  async function activate() {
    if (!round) return;
    setBusy(true);
    try {
      await postJson(`/rounds/${round.id}/activate`, {});
      toastSuccess('Round activated');
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function complete() {
    if (!round) return;
    setBusy(true);
    try {
      await postJson(`/rounds/${round.id}/complete`, {});
      toastSuccess('Round completed');
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (error) return <Wrapper onClose={onClose}><p className="text-danger text-sm">{error}</p></Wrapper>;
  if (!round) return <Wrapper onClose={onClose}><p className="text-sm text-muted">Loading…</p></Wrapper>;

  return (
    <Wrapper onClose={onClose}>
      <header className="space-y-2">
        <div className="flex items-center gap-2">
          <span className="text-xs font-mono text-muted">{round.short_code}</span>
          <Badge size="sm" variant={STATUS_VARIANT[round.status]}>
            {round.status}
          </Badge>
        </div>
        <h2 className="text-headline-md text-foreground">Round · {round.mode}</h2>
        <div className="text-[11px] text-muted">
          in-flight: {round.in_flight_policy} · disruption: {round.disruption_policy}
        </div>
      </header>

      <div className="flex gap-2">
        {round.status === 'planning' && (
          <Button size="sm" onClick={activate} disabled={busy}>Activate</Button>
        )}
        {round.status === 'active' && (
          <Button size="sm" variant="outline" onClick={complete} disabled={busy}>Complete</Button>
        )}
      </div>

      <section>
        <h3 className="text-headline-md text-foreground mb-2 flex items-baseline gap-2">
          Tasks <span className="text-xs text-muted font-normal">{tasks.length}</span>
        </h3>
        {tasks.length === 0 ? (
          <p className="text-xs italic text-muted">No tasks in this round.</p>
        ) : (
          <ul className="divide-y divide-border rounded-md border border-border bg-background">
            {tasks.map((t) => (
              <li key={t.id} className="px-3 py-2 flex items-center gap-2">
                <span className="text-[10px] font-mono text-muted">{t.short_code}</span>
                <span className="flex-1 min-w-0 truncate text-xs text-foreground">{t.description}</span>
                <Badge size="sm" variant={t.status === 'done' ? 'success' : t.status === 'in_progress' ? 'warning' : t.status === 'blocked' ? 'danger' : 'default'}>
                  {t.status}
                </Badge>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section>
        <h3 className="text-headline-md text-foreground mb-2 flex items-baseline gap-2">
          Scenario results <span className="text-xs text-muted font-normal">{results.length}</span>
        </h3>
        {results.length === 0 ? (
          <p className="text-xs italic text-muted">No scenario results recorded yet.</p>
        ) : (
          <ul className="divide-y divide-border rounded-md border border-border bg-background">
            {results.map((r) => (
              <li key={`${r.round_id}-${r.scenario_id}`} className="px-3 py-2 flex items-center gap-2 text-xs">
                <span className="font-mono text-muted truncate max-w-[160px]">{r.scenario_id}</span>
                <Badge size="sm" variant={RESULT_VARIANT[r.result]} className="ml-auto">
                  {r.result}
                </Badge>
                {r.evidence_ref && (
                  <span className="text-muted truncate max-w-[160px]">{r.evidence_ref}</span>
                )}
              </li>
            ))}
          </ul>
        )}
      </section>
    </Wrapper>
  );
}

function Wrapper({ onClose, children }: { onClose: () => void; children: React.ReactNode }) {
  return (
    <div className="h-full flex flex-col">
      <div className="px-5 py-3 border-b border-border flex items-center justify-between sticky top-0 bg-surface z-10">
        <span className="text-[10px] uppercase tracking-wide text-muted">Round</span>
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
