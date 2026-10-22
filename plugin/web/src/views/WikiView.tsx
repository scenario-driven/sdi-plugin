import { useEffect, useMemo, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import type { KnowledgeEntry } from '../types/entities';
import { getJson } from '../lib/api';
import { cn } from '../lib/cn';

interface WikiViewProps {
  projectId: string;
  refreshKey: number;
}

export function WikiView({ projectId, refreshKey }: WikiViewProps) {
  const [entries, setEntries] = useState<KnowledgeEntry[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await getJson<KnowledgeEntry[]>(
          `/knowledge?project_id=${projectId}&scope=rag`,
        ).catch(() => [] as KnowledgeEntry[]);
        if (cancelled) return;
        setEntries(list);
        if (list.length && !selectedId) setSelectedId(list[0].id);
      } catch (err) {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      }
    })();
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId, refreshKey]);

  const filtered = useMemo(() => {
    if (!query) return entries;
    const needle = query.toLowerCase();
    return entries.filter(
      (e) =>
        e.title.toLowerCase().includes(needle) ||
        e.body.toLowerCase().includes(needle) ||
        e.tags.some((t) => t.toLowerCase().includes(needle)),
    );
  }, [entries, query]);

  const selected = useMemo(
    () => entries.find((e) => e.id === selectedId) ?? null,
    [entries, selectedId],
  );

  if (error) return <div className="p-6 text-danger text-sm">{error}</div>;

  return (
    <div className="h-full flex">
      <aside className="w-72 shrink-0 border-r border-border flex flex-col">
        <div className="px-4 pt-4 pb-2 border-b border-border">
          <h2 className="text-headline-md text-foreground">Wiki</h2>
          <p className="text-xs text-muted">RAG knowledge for this project.</p>
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search…"
            className="mt-2 w-full rounded-md border border-border bg-background px-3 py-1.5 text-sm text-foreground placeholder:text-muted focus:outline-none focus:border-primary"
          />
        </div>
        <div className="flex-1 overflow-auto">
          {filtered.length === 0 ? (
            <p className="p-4 text-xs text-muted">No knowledge yet.</p>
          ) : (
            <ul className="divide-y divide-border">
              {filtered.map((e) => (
                <li key={e.id}>
                  <button
                    type="button"
                    onClick={() => setSelectedId(e.id)}
                    className={cn(
                      'w-full px-4 py-2 text-left hover:bg-surface-hover transition-colors cursor-pointer',
                      selectedId === e.id && 'bg-surface-high',
                    )}
                  >
                    <div className="text-sm text-foreground truncate">{e.title}</div>
                    <div className="text-[10px] uppercase tracking-wide text-muted mt-0.5">
                      {e.kind}
                    </div>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </aside>
      <div className="flex-1 min-w-0 overflow-auto">
        {selected ? (
          <article className="max-w-3xl mx-auto p-8">
            <header className="mb-6">
              <div className="text-[10px] uppercase tracking-wide text-muted">
                {selected.kind} · {selected.scope}
              </div>
              <h2 className="text-headline-lg text-foreground mt-1">{selected.title}</h2>
              {selected.tags.length > 0 && (
                <div className="mt-2 flex gap-1.5 flex-wrap">
                  {selected.tags.map((t) => (
                    <span
                      key={t}
                      className="text-[10px] uppercase tracking-wide rounded bg-surface-high text-muted px-2 py-0.5"
                    >
                      {t}
                    </span>
                  ))}
                </div>
              )}
            </header>
            <div className="prose prose-sm max-w-none text-foreground prose-headings:text-foreground prose-strong:text-foreground prose-code:text-foreground prose-a:text-primary">
              <ReactMarkdown remarkPlugins={[remarkGfm]}>{selected.body}</ReactMarkdown>
            </div>
          </article>
        ) : (
          <p className="p-10 text-center text-sm text-muted">
            Select a knowledge entry to view it.
          </p>
        )}
      </div>
    </div>
  );
}
