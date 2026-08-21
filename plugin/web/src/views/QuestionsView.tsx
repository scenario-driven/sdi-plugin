import { useCallback, useEffect, useMemo, useState } from 'react';
import type { DecisionQuestion } from '../types/entities';
import { answerQuestion, listDecisionQuestions } from '../lib/api';
import type { AnswerQuestionBody } from '../lib/api';
import { QuestionCard } from '../components/QuestionCard';
import { Badge, Button } from '../components/ui';
import { toastSuccess, toastError } from '../lib/toast';
import { cn } from '../lib/cn';

interface QuestionsViewProps {
  projectId: string;
  refreshKey: number;
}

type Mode = 'batch' | 'conversational';

/** D36 — the decision-question answering surface. Two modes:
 *  - **batch**: stage selections across every open question, then submit the
 *    whole set in one pass.
 *  - **conversational**: answer one question at a time, with the per-card LLM
 *    discussion panel, each submitted immediately on click.
 *  The fact/preference distinction and the SA-exam card itself live in
 *  `QuestionCard`; this view owns the list, the mode toggle, and the batch
 *  submit orchestration. */
export function QuestionsView({ projectId, refreshKey }: QuestionsViewProps) {
  const [questions, setQuestions] = useState<DecisionQuestion[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const loading = questions === null && error === null;
  const [mode, setMode] = useState<Mode>('batch');
  /** Local bump so a conversational-mode answer re-fetches without waiting for
   *  the SSE round-trip. */
  const [localSeq, setLocalSeq] = useState(0);
  /** Staged answers (batch mode): questionId → body. */
  const [staged, setStaged] = useState<Map<string, AnswerQuestionBody>>(new Map());
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    let cancelled = false;
    listDecisionQuestions(projectId)
      .then((rows) => {
        if (cancelled) return;
        setQuestions(rows);
        setError(null);
      })
      .catch((err) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [projectId, refreshKey, localSeq]);

  const open = useMemo(
    () => (questions ?? []).filter((q) => q.status === 'open'),
    [questions],
  );
  const resolved = useMemo(
    () => (questions ?? []).filter((q) => q.status !== 'open'),
    [questions],
  );

  const stage = useCallback(
    (questionId: string, body: AnswerQuestionBody | null) => {
      setStaged((prev) => {
        const next = new Map(prev);
        if (body === null) next.delete(questionId);
        else next.set(questionId, body);
        return next;
      });
    },
    [],
  );

  const submitBatch = async () => {
    if (staged.size === 0) return;
    setSubmitting(true);
    const entries = Array.from(staged.entries());
    let ok = 0;
    let failed = 0;
    for (const [questionId, body] of entries) {
      const q = open.find((x) => x.id === questionId);
      try {
        await answerQuestion(questionId, {
          ...body,
          auto: q?.qtype === 'fact',
        });
        ok += 1;
      } catch {
        failed += 1;
      }
    }
    setSubmitting(false);
    setStaged(new Map());
    if (ok > 0) toastSuccess(`${ok}개 답변 제출됨`);
    if (failed > 0) toastError(`${failed}개 제출 실패`);
    setLocalSeq((n) => n + 1);
  };

  return (
    <div className="h-full flex flex-col">
      <header className="px-6 pt-5 pb-3 border-b border-border">
        <div className="flex items-center gap-3 flex-wrap">
          <h2 className="text-headline-lg text-foreground">Questions</h2>
          <Badge size="sm" variant={open.length > 0 ? 'warning' : 'success'}>
            open {open.length}
          </Badge>
          {resolved.length > 0 && (
            <Badge size="sm" variant="default">
              resolved {resolved.length}
            </Badge>
          )}
          <div className="ml-auto flex items-center gap-1 rounded-md border border-border p-0.5">
            {(['batch', 'conversational'] as const).map((m) => (
              <button
                key={m}
                type="button"
                onClick={() => setMode(m)}
                data-active={mode === m || undefined}
                className={cn(
                  'rounded px-2.5 py-1 text-xs font-medium transition-colors cursor-pointer',
                  mode === m
                    ? 'bg-surface-high text-foreground'
                    : 'text-muted hover:text-foreground',
                )}
              >
                {m === 'batch' ? '일괄' : '대화'}
              </button>
            ))}
          </div>
        </div>
        <p className="text-sm text-muted mt-1">
          {mode === 'batch'
            ? 'SA 시험형 결정-질문: 보기를 모두 고른 뒤 한 번에 제출합니다.'
            : '한 문제씩, LLM과 토론(정답 확인하듯)한 뒤 즉시 제출합니다.'}
        </p>
      </header>

      <div className="flex-1 overflow-auto p-6 space-y-4">
        {error && <p className="text-sm text-danger">{error}</p>}
        {!error && loading && <p className="text-sm text-muted">로딩…</p>}
        {!error && !loading && open.length === 0 && (
          <div className="rounded-lg border border-success/40 bg-success/5 p-4">
            <p className="text-sm text-success">
              미답 질문 0 — 결정-질문 게이트 통과.
            </p>
          </div>
        )}

        {open.map((q) => (
          <QuestionCard
            key={q.id}
            question={q}
            refreshKey={refreshKey + localSeq}
            {...(mode === 'batch'
              ? { onStage: stage, staged: staged.get(q.id) ?? null }
              : { onAnswered: () => setLocalSeq((n) => n + 1) })}
          />
        ))}

        {resolved.length > 0 && (
          <details className="rounded-lg border border-border bg-surface">
            <summary className="cursor-pointer px-4 py-2.5 text-sm text-muted">
              해소된 질문 {resolved.length}개
            </summary>
            <div className="space-y-4 p-4 pt-0">
              {resolved.map((q) => (
                <QuestionCard key={q.id} question={q} refreshKey={refreshKey} />
              ))}
            </div>
          </details>
        )}
      </div>

      {mode === 'batch' && open.length > 0 && (
        <footer className="border-t border-border px-6 py-3 flex items-center gap-3">
          <span className="text-sm text-muted">
            {staged.size} / {open.length} staged
          </span>
          <Button
            className="ml-auto"
            onClick={submitBatch}
            disabled={submitting || staged.size === 0}
          >
            {submitting ? '제출 중…' : `일괄 제출 (${staged.size})`}
          </Button>
        </footer>
      )}
    </div>
  );
}
