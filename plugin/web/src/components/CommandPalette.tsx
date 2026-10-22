import { useEffect, useMemo, useRef, useState } from 'react';
import { cn } from '../lib/cn';

export interface Command {
  id: string;
  label: string;
  keywords?: string;
  run: () => void;
}

interface CommandPaletteProps {
  commands: Command[];
  onClose: () => void;
}

export function CommandPalette({ commands, onClose }: CommandPaletteProps) {
  const [query, setQuery] = useState('');
  const [active, setActive] = useState(0);
  const [lastQuery, setLastQuery] = useState('');
  const inputRef = useRef<HTMLInputElement>(null);

  const filtered = useMemo(() => {
    if (!query) return commands;
    const needle = query.toLowerCase();
    return commands.filter((c) =>
      `${c.label} ${c.keywords ?? ''}`.toLowerCase().includes(needle),
    );
  }, [commands, query]);

  // Render-time state adjustment: reset cursor when query changes.
  if (query !== lastQuery) {
    setLastQuery(query);
    setActive(0);
  }

  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  function handleKey(e: React.KeyboardEvent) {
    if (e.key === 'ArrowDown') {
      e.preventDefault();
      setActive((a) => Math.min(a + 1, Math.max(0, filtered.length - 1)));
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      setActive((a) => Math.max(a - 1, 0));
    } else if (e.key === 'Enter') {
      e.preventDefault();
      const c = filtered[active];
      if (c) c.run();
    }
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center bg-overlay pt-[15vh]"
      onClick={onClose}
    >
      <div
        className="w-[520px] rounded-lg border border-border bg-surface shadow-elevated"
        onClick={(e) => e.stopPropagation()}
      >
        <input
          ref={inputRef}
          type="text"
          value={query}
          placeholder="Type a command…"
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={handleKey}
          className="w-full bg-transparent px-4 py-3 text-sm text-foreground placeholder:text-muted border-b border-border focus:outline-none"
        />
        <ul className="max-h-[60vh] overflow-auto py-1">
          {filtered.length === 0 ? (
            <li className="px-4 py-3 text-sm text-muted text-center">No commands</li>
          ) : (
            filtered.map((c, i) => (
              <li key={c.id}>
                <button
                  type="button"
                  onClick={c.run}
                  onMouseEnter={() => setActive(i)}
                  className={cn(
                    'w-full text-left px-4 py-2 text-sm cursor-pointer',
                    i === active
                      ? 'bg-primary/15 text-foreground'
                      : 'text-foreground hover:bg-surface-hover',
                  )}
                >
                  {c.label}
                </button>
              </li>
            ))
          )}
        </ul>
        <div className="px-3 py-2 border-t border-border text-[10px] text-muted flex justify-between">
          <span>↑↓ navigate · ↵ select · esc close</span>
          <span>⌘K</span>
        </div>
      </div>
    </div>
  );
}
