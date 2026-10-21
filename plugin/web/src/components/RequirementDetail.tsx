import { useEffect, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { Requirement } from '../types/entities';
import { getJson } from '../lib/api';

interface RequirementDetailProps {
  requirementId: string;
  onClose: () => void;
  refreshKey: number;
}

export function RequirementDetail({ requirementId, onClose, refreshKey }: RequirementDetailProps) {
  const [req, setReq] = useState<Requirement | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const r = await getJson<Requirement>(`/requirements/${requirementId}`).catch(async () => {
          const list = await getJson<Requirement[]>(`/requirements?id=${requirementId}`).catch(() => [] as Requirement[]);
          return list[0] ?? null;
        });
        if (!r) throw new Error('Requirement not found');
        if (!cancelled) setReq(r);
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [requirementId, refreshKey]);

  if (error) return <Wrapper onClose={onClose}><p className="text-danger text-sm">{error}</p></Wrapper>;
  if (!req) return <Wrapper onClose={onClose}><p className="text-sm text-muted">Loading…</p></Wrapper>;

  return (
    <Wrapper onClose={onClose}>
      <header className="space-y-2">
        <span className="text-xs font-mono text-muted">{req.short_code}</span>
        <h2 className="text-headline-md text-foreground">Requirement</h2>
      </header>
      <div className="rounded-md border border-border bg-background p-4">
        <div className="prose prose-sm max-w-none text-foreground prose-headings:text-foreground prose-strong:text-foreground prose-code:text-foreground prose-a:text-primary">
          <ReactMarkdown remarkPlugins={[remarkGfm]}>{req.body}</ReactMarkdown>
        </div>
      </div>
      <section className="text-[11px] text-muted space-y-0.5">
        <div>Created {new Date(req.created_at).toLocaleString()}</div>
        <div>Updated {new Date(req.updated_at).toLocaleString()}</div>
      </section>
    </Wrapper>
  );
}

function Wrapper({ onClose, children }: { onClose: () => void; children: React.ReactNode }) {
  return (
    <div className="h-full flex flex-col">
      <div className="px-5 py-3 border-b border-border flex items-center justify-between sticky top-0 bg-surface z-10">
        <span className="text-[10px] uppercase tracking-wide text-muted">Requirement</span>
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
