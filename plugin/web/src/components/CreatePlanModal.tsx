import { useState } from 'react';
import type { Plan } from '../types/entities';
import { postJson, DaemonError } from '../lib/api';
import { toastError } from '../lib/toast';
import { Modal, Button, Input, Textarea, Label } from './ui';

interface CreatePlanModalProps {
  projectId: string;
  onClose: () => void;
  onCreated: (plan: Plan) => void;
}

export function CreatePlanModal({ projectId, onClose, onCreated }: CreatePlanModalProps) {
  const [shortCode, setShortCode] = useState('');
  const [title, setTitle] = useState('');
  const [body, setBody] = useState('');
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!title.trim() || !shortCode.trim()) return;
    setBusy(true);
    try {
      const plan = await postJson<Plan>('/plans', {
        project_id: projectId,
        short_code: shortCode.trim().toUpperCase(),
        title: title.trim(),
        body: body.trim() || '',
      });
      onCreated(plan);
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal.Overlay onClose={onClose}>
      <Modal.Content className="w-[560px]">
        <Modal.Header>New plan</Modal.Header>
        <form onSubmit={submit}>
          <Modal.Body>
            <div className="space-y-1.5">
              <Label htmlFor="plan-short">Short code</Label>
              <Input
                id="plan-short"
                autoFocus
                value={shortCode}
                onChange={(e) => setShortCode(e.target.value.toUpperCase())}
                placeholder="SD-AUTH"
                required
                pattern="[A-Z][A-Z0-9-]{1,32}"
                title="Uppercase letters, digits, hyphens. Project key prefix + slug."
              />
              <p className="text-[11px] text-muted">
                Ticket label (e.g. <span className="font-mono">SD-AUTH</span>).
              </p>
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="plan-title">Title</Label>
              <Input
                id="plan-title"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder="What's the goal of this plan?"
                required
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="plan-body">Body (markdown)</Label>
              <Textarea
                id="plan-body"
                value={body}
                onChange={(e) => setBody(e.target.value)}
                rows={10}
                placeholder="Context, motivation, scope…"
              />
              <p className="text-[11px] text-muted">
                Plans start in <span className="font-mono">draft</span>. Add scenarios then approve to activate.
              </p>
            </div>
          </Modal.Body>
          <div className="px-5 py-3 border-t border-border flex justify-end gap-2">
            <Button type="button" variant="ghost" onClick={onClose} disabled={busy}>
              Cancel
            </Button>
            <Button type="submit" disabled={busy || !title.trim() || !shortCode.trim()}>
              {busy ? 'Creating…' : 'Create plan'}
            </Button>
          </div>
        </form>
      </Modal.Content>
    </Modal.Overlay>
  );
}
