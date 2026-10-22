import { useEffect, useState, useCallback } from 'react';
import type { AgentNote, AgentNoteKind, AgentNoteScope } from '../types/entities';
import { getJson, postJson, DaemonError } from '../lib/api';
import { toastError, toastSuccess } from '../lib/toast';
import { Badge, Button, Input, Select, Textarea } from './ui';
import { cn } from '../lib/cn';

interface AgentNotesPanelProps {
  projectId: string;
  scopeKind: AgentNoteScope;
  anchorId: string;
  refreshKey: number;
}

interface ListResponse {
  notes: AgentNote[];
}

const KIND_VARIANT: Record<AgentNoteKind, 'default' | 'primary' | 'success' | 'warning' | 'danger' | 'info'> = {
  hypothesis: 'info',
  observation: 'default',
  question: 'primary',
  dissent: 'danger',
  evidence: 'success',
  handoff: 'warning',
};

const KIND_OPTIONS: AgentNoteKind[] = [
  'observation',
  'hypothesis',
  'question',
  'dissent',
  'evidence',
  'handoff',
];

export function AgentNotesPanel({ projectId, scopeKind, anchorId, refreshKey }: AgentNotesPanelProps) {
  const [notes, setNotes] = useState<AgentNote[]>([]);
  const [kind, setKind] = useState<AgentNoteKind>('observation');
  const [from, setFrom] = useState('main');
  const [to, setTo] = useState('');
  const [body, setBody] = useState('');
  const [busy, setBusy] = useState(false);
  const [reloadTick, setReloadTick] = useState(0);

  const triggerReload = useCallback(() => setReloadTick((n) => n + 1), []);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await getJson<ListResponse | AgentNote[]>(
          `/agent_notes?scope_kind=${encodeURIComponent(scopeKind)}&anchor_id=${encodeURIComponent(anchorId)}`,
        );
        if (cancelled) return;
        const rows = Array.isArray(list) ? list : list.notes;
        setNotes(rows);
      } catch (err) {
        if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [scopeKind, anchorId, refreshKey, reloadTick]);

  async function append() {
    if (!body.trim() || !from.trim()) return;
    if (kind === 'handoff' && !to.trim()) {
      toastError('handoff requires to_agent');
      return;
    }
    setBusy(true);
    try {
      const anchor =
        scopeKind === 'plan' ? { plan_id: anchorId } :
        scopeKind === 'round' ? { round_id: anchorId } :
        scopeKind === 'scenario' ? { scenario_id: anchorId } :
        { task_id: anchorId };
      await postJson('/agent_notes', {
        project_id: projectId,
        scope_kind: scopeKind,
        kind,
        from_agent: from.trim(),
        to_agent: to.trim() || null,
        body: body.trim(),
        ...anchor,
      });
      toastSuccess('Note appended');
      setBody('');
      setTo('');
      triggerReload();
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function ack(id: string) {
    setBusy(true);
    try {
      await postJson(`/agent_notes/${id}/ack`);
      toastSuccess('Handoff acked');
      triggerReload();
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function retire(id: string) {
    const reason = window.prompt('Retire note. Reason:');
    if (!reason || !reason.trim()) return;
    setBusy(true);
    try {
      await postJson(`/agent_notes/${id}/retire`, { reason: reason.trim() });
      toastSuccess('Note retired');
      triggerReload();
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="space-y-3">
      <h3 className="text-headline-md text-foreground flex items-baseline gap-2">
        Agent notes
        <span className="text-xs text-muted font-normal">{notes.length}</span>
        <span className="text-[10px] text-muted ml-auto">M1 blackboard · live via SSE</span>
      </h3>

      {notes.length === 0 ? (
        <p className="text-xs italic text-muted px-2 py-2">No notes on this anchor yet.</p>
      ) : (
        <ul className="space-y-2">
          {notes.map((n) => (
            <li
              key={n.id}
              className={cn(
                'rounded-md border border-border bg-background px-3 py-2 space-y-1',
                n.retired_at && 'opacity-50',
              )}
            >
              <div className="flex items-center gap-2 flex-wrap">
                <Badge size="sm" variant={KIND_VARIANT[n.kind]}>{n.kind}</Badge>
                <span className="font-mono text-[10px] text-muted">{n.from_agent}</span>
                {n.to_agent && (
                  <>
                    <span className="text-[10px] text-muted">→</span>
                    <span className="font-mono text-[10px] text-foreground">{n.to_agent}</span>
                  </>
                )}
                {n.receipt_acknowledged_at && (
                  <Badge size="sm" variant="success">acked</Badge>
                )}
                <span className="text-[10px] text-muted ml-auto">
                  {new Date(n.created_at).toLocaleString()}
                </span>
              </div>
              <p className="text-xs text-foreground whitespace-pre-wrap">{n.body}</p>
              {!n.retired_at && (
                <div className="flex items-center gap-2 pt-1">
                  {n.kind === 'handoff' && !n.receipt_acknowledged_at && (
                    <button
                      type="button"
                      onClick={() => ack(n.id)}
                      disabled={busy}
                      className="text-[10px] text-primary hover:underline cursor-pointer"
                    >
                      ack →
                    </button>
                  )}
                  <button
                    type="button"
                    onClick={() => retire(n.id)}
                    disabled={busy}
                    className="text-[10px] text-muted hover:text-danger cursor-pointer ml-auto"
                  >
                    retire
                  </button>
                </div>
              )}
              {n.retired_at && n.retired_reason && (
                <p className="text-[10px] italic text-muted">
                  retired: {n.retired_reason}
                </p>
              )}
            </li>
          ))}
        </ul>
      )}

      <div className="rounded-md border border-border bg-background p-3 space-y-2">
        <div className="text-[10px] uppercase tracking-wide text-muted">Append note</div>
        <div className="flex items-center gap-2 flex-wrap">
          <Select
            size="sm"
            value={kind}
            onChange={(e) => setKind(e.target.value as AgentNoteKind)}
            disabled={busy}
            className="max-w-[140px]"
          >
            {KIND_OPTIONS.map((k) => (
              <option key={k} value={k}>{k}</option>
            ))}
          </Select>
          <Input
            size="sm"
            placeholder="from_agent"
            value={from}
            onChange={(e) => setFrom(e.target.value)}
            disabled={busy}
            className="max-w-[140px] font-mono text-xs"
          />
          {kind === 'handoff' && (
            <Input
              size="sm"
              placeholder="to_agent"
              value={to}
              onChange={(e) => setTo(e.target.value)}
              disabled={busy}
              className="max-w-[140px] font-mono text-xs"
            />
          )}
        </div>
        <Textarea
          rows={2}
          placeholder="body"
          value={body}
          onChange={(e) => setBody(e.target.value)}
          disabled={busy}
        />
        <Button
          size="sm"
          onClick={append}
          disabled={busy || !body.trim() || !from.trim() || (kind === 'handoff' && !to.trim())}
        >
          Append
        </Button>
      </div>
    </section>
  );
}
