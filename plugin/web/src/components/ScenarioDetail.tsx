import { useEffect, useState } from 'react';
import type { Scenario } from '../types/entities';
import {
  getJson,
  postJson,
  retireScenario,
  unretireScenario,
  DaemonError,
} from '../lib/api';
import { toastError, toastSuccess } from '../lib/toast';
import { Badge, Button } from './ui';

interface ScenarioDetailProps {
  scenarioId: string;
  onClose: () => void;
  refreshKey: number;
}

export function ScenarioDetail({ scenarioId, onClose, refreshKey }: ScenarioDetailProps) {
  const [scenario, setScenario] = useState<Scenario | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const s = await getJson<Scenario>(`/scenarios/${scenarioId}`).catch(async () => {
          const list = await getJson<Scenario[]>(`/scenarios?id=${scenarioId}`).catch(() => [] as Scenario[]);
          return list[0] ?? null;
        });
        if (!s) throw new Error('Scenario not found');
        if (!cancelled) setScenario(s);
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [scenarioId, refreshKey]);

  async function confirm() {
    if (!scenario) return;
    setBusy(true);
    try {
      const fresh = await postJson<Scenario>(`/scenarios/${scenario.id}/confirm`, {});
      setScenario(fresh);
      toastSuccess('Scenario confirmed');
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function toggleRetired() {
    if (!scenario) return;
    const retiring = !scenario.retired_at;
    setBusy(true);
    try {
      const fresh = retiring
        ? await retireScenario(scenario.id)
        : await unretireScenario(scenario.id);
      setScenario(fresh);
      toastSuccess(retiring ? 'Scenario retired' : 'Scenario restored');
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (error) return <Wrapper onClose={onClose}><p className="text-danger text-sm">{error}</p></Wrapper>;
  if (!scenario) return <Wrapper onClose={onClose}><p className="text-sm text-muted">Loading…</p></Wrapper>;

  const retired = Boolean(scenario.retired_at);

  return (
    <Wrapper onClose={onClose}>
      <header className="space-y-2">
        <div className="flex items-center gap-2">
          <span className="text-xs font-mono text-muted">{scenario.short_code}</span>
          <Badge size="sm" variant={scenario.status === 'confirmed' ? 'success' : 'default'}>
            {scenario.status}
          </Badge>
          {retired && (
            <Badge size="sm" variant="warning">retired</Badge>
          )}
        </div>
        <h2 className="text-headline-md text-foreground">Scenario</h2>
        {retired && (
          <p className="text-[11px] text-muted">
            Excluded from verification, regression carry-over, and the approve
            count. History is preserved; restore to bring it back.
          </p>
        )}
      </header>

      <div className="flex items-center gap-2">
        {scenario.status === 'draft' && !retired && (
          <Button size="sm" onClick={confirm} disabled={busy}>
            {busy ? 'Confirming…' : 'Confirm scenario'}
          </Button>
        )}
        <Button size="sm" variant="ghost" onClick={toggleRetired} disabled={busy}>
          {retired ? 'Restore (un-retire)' : 'Retire'}
        </Button>
      </div>

      <section
        className={`rounded-md border border-border bg-background p-4 space-y-3 ${
          retired ? 'opacity-50' : ''
        }`}
      >
        <Field label="Given" value={scenario.given} />
        <Field label="When" value={scenario.when} />
        <Field label="Then" value={scenario.then} />
      </section>

      <section className="text-[11px] text-muted space-y-0.5">
        <div>Created {new Date(scenario.created_at).toLocaleString()}</div>
        <div>Updated {new Date(scenario.updated_at).toLocaleString()}</div>
        {retired && <div>Retired {new Date(scenario.retired_at as string).toLocaleString()}</div>}
      </section>
    </Wrapper>
  );
}

function Wrapper({ onClose, children }: { onClose: () => void; children: React.ReactNode }) {
  return (
    <div className="h-full flex flex-col">
      <div className="px-5 py-3 border-b border-border flex items-center justify-between sticky top-0 bg-surface z-10">
        <span className="text-[10px] uppercase tracking-wide text-muted">Scenario</span>
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

function Field({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="text-[10px] uppercase tracking-wide text-muted">{label}</div>
      <div className="text-sm text-foreground whitespace-pre-wrap mt-0.5">{value}</div>
    </div>
  );
}
