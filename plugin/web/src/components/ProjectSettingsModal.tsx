// Modal wrapper around ProjectSettings — opened from the project switcher
// gear icon so the user can edit metadata, toggle SDI governance, or
// delete the project without leaving the current route.
//
// Listens for Escape to close (parity with the Clawket reference modal).
// Delete-cascade hand-off: when ProjectSettings invokes onDeleted, the
// modal closes and the host clears its selection so the next render picks
// a different active project.

import { useEffect } from 'react';
import type { Project } from '../types/entities';
import { Modal } from './ui';
import { ProjectSettings } from './ProjectSettings';

export interface ProjectSettingsModalProps {
  project: Project;
  onClose: () => void;
  /** Re-fetch the project list after any mutation succeeds. */
  onProjectChange: () => Promise<unknown>;
  /** Fired after the project is hard-deleted. Host should clear its
   *  cached project + navigate to a project-less route. */
  onDeleted?: () => void;
}

export function ProjectSettingsModal({
  project,
  onClose,
  onProjectChange,
  onDeleted,
}: ProjectSettingsModalProps) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);

  return (
    <Modal.Overlay onClose={onClose}>
      <Modal.Content className="w-[560px] max-w-[calc(100vw-2rem)]">
        <div
          role="dialog"
          aria-modal="true"
          aria-label="Project settings"
          data-testid="project-settings-modal"
        >
          <Modal.Header>
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0 flex flex-col">
                <span>Project settings</span>
                <span
                  data-testid="project-settings-id"
                  className="font-mono text-xs text-muted truncate"
                >
                  {project.id}
                </span>
              </div>
              <button
                type="button"
                aria-label="Close"
                data-testid="project-settings-close"
                onClick={onClose}
                className="rounded p-1 text-muted hover:text-foreground cursor-pointer"
              >
                ✕
              </button>
            </div>
          </Modal.Header>
          <Modal.Body className="p-5">
            <ProjectSettings
              project={project}
              onProjectChange={onProjectChange}
              onDeleted={() => {
                if (onDeleted) onDeleted();
                onClose();
              }}
            />
          </Modal.Body>
        </div>
      </Modal.Content>
    </Modal.Overlay>
  );
}

export default ProjectSettingsModal;
