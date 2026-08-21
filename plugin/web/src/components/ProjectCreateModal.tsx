import { useState } from 'react';
import type { Project } from '../types/entities';
import { postJson, DaemonError } from '../lib/api';
import { toastError } from '../lib/toast';
import { Modal, Button, Input, Label } from './ui';

interface ProjectCreateModalProps {
  onClose: () => void;
  onCreated: (project: Project) => void;
}

export function ProjectCreateModal({ onClose, onCreated }: ProjectCreateModalProps) {
  const [name, setName] = useState('');
  const [key, setKey] = useState('');
  const [cwd, setCwd] = useState('');
  const [busy, setBusy] = useState(false);

  async function submit(e: React.FormEvent) {
    e.preventDefault();
    if (!name.trim() || !key.trim()) return;
    setBusy(true);
    try {
      const project = await postJson<Project>('/projects', {
        name: name.trim(),
        key: key.trim().toUpperCase(),
        cwds: cwd.trim() ? [cwd.trim()] : [],
      });
      onCreated(project);
    } catch (err) {
      if (err instanceof DaemonError) toastError(`${err.code}: ${err.message}`);
      else toastError(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Modal.Overlay onClose={onClose}>
      <Modal.Content className="w-[480px]">
        <Modal.Header>Create project</Modal.Header>
        <form onSubmit={submit}>
          <Modal.Body>
            <div className="space-y-1.5">
              <Label htmlFor="proj-name">Name</Label>
              <Input
                id="proj-name"
                autoFocus
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="My Project"
                required
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="proj-key">Key</Label>
              <Input
                id="proj-key"
                value={key}
                onChange={(e) => setKey(e.target.value.toUpperCase())}
                placeholder="MP"
                required
                pattern="[A-Z][A-Z0-9-]{0,9}"
                title="Uppercase letters, digits, hyphens. Starts with a letter."
              />
              <p className="text-[11px] text-muted">
                Short prefix for task codes (e.g. <span className="font-mono">MP-1</span>).
              </p>
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="proj-cwd">Working directory (optional)</Label>
              <Input
                id="proj-cwd"
                value={cwd}
                onChange={(e) => setCwd(e.target.value)}
                placeholder="/Users/you/repo"
              />
              <p className="text-[11px] text-muted">
                Anchored cwd for "active project" detection.
              </p>
            </div>
          </Modal.Body>
          <div className="px-5 py-3 border-t border-border flex justify-end gap-2">
            <Button type="button" variant="ghost" onClick={onClose} disabled={busy}>
              Cancel
            </Button>
            <Button type="submit" disabled={busy || !name.trim() || !key.trim()}>
              {busy ? 'Creating…' : 'Create'}
            </Button>
          </div>
        </form>
      </Modal.Content>
    </Modal.Overlay>
  );
}
