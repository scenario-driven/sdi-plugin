import { useEffect, useState } from 'react';
import { onToast, type ToastPayload } from '../lib/toast';
import { cn } from '../lib/cn';

interface Visible extends ToastPayload {
  count: number;
}

const KIND_STYLES: Record<ToastPayload['kind'], string> = {
  info: 'border-primary/40 bg-primary/10 text-foreground',
  success: 'border-success/40 bg-success/10 text-foreground',
  warning: 'border-warning/40 bg-warning/10 text-foreground',
  error: 'border-danger/40 bg-danger/10 text-foreground',
};

const KIND_GLYPH: Record<ToastPayload['kind'], string> = {
  info: 'ℹ',
  success: '✓',
  warning: '⚠',
  error: '✕',
};

export function ToastContainer() {
  const [list, setList] = useState<Visible[]>([]);

  useEffect(() => {
    const unsub = onToast((p) => {
      setList((prev) => {
        const idx = p.hash ? prev.findIndex((x) => x.hash === p.hash) : -1;
        if (idx >= 0) {
          const next = prev.slice();
          next[idx] = { ...next[idx], count: next[idx].count + 1 };
          return next;
        }
        return [...prev, { ...p, count: 1 }];
      });
      if (p.duration && p.duration > 0) {
        window.setTimeout(() => {
          setList((prev) => prev.filter((x) => x.id !== p.id));
        }, p.duration);
      }
    });
    return unsub;
  }, []);

  function dismiss(id: string) {
    setList((prev) => prev.filter((x) => x.id !== id));
  }

  if (list.length === 0) return null;
  return (
    <div className="fixed bottom-4 right-4 z-[60] flex flex-col gap-2 max-w-sm">
      {list.map((t) => (
        <div
          key={t.id}
          className={cn(
            'rounded-md border px-3 py-2 shadow-elevated text-sm flex items-start gap-2',
            KIND_STYLES[t.kind],
          )}
        >
          <span className="mt-0.5 leading-none">{KIND_GLYPH[t.kind]}</span>
          <span className="flex-1 min-w-0 break-words">{t.message}</span>
          {t.count > 1 && (
            <span className="text-[10px] rounded bg-foreground/10 px-1.5 py-0.5 mt-0.5">
              ×{t.count}
            </span>
          )}
          <button
            type="button"
            aria-label="Dismiss"
            onClick={() => dismiss(t.id)}
            className="text-muted hover:text-foreground cursor-pointer leading-none"
          >
            ×
          </button>
        </div>
      ))}
    </div>
  );
}
