// Project settings view (v0.3) — standalone editor for the seven mutable
// dimensions of a Project row:
//   name, description, cwds, wiki_paths, enabled, plus the project id and
//   the danger-zone delete action.
//
// Mirrors the Clawket reference (web/src/components/ProjectSettings.tsx)
// adapted to SDI's daemon contract (`enabled: boolean`, PATCH-style PUT
// /projects/:id, dedicated /disable + /enable + DELETE endpoints). The
// modal wrapper at ./ProjectSettingsModal lifts this view into a Dialog
// so the project switcher can edit without leaving the current route.

import { useCallback, useState } from 'react';
import type { Project } from '../types/entities';
import {
  addProjectCwd,
  deleteProject,
  disableProject,
  enableProject,
  removeProjectCwd,
  updateProject,
} from '../lib/api';
import { toastError, toastSuccess } from '../lib/toast';
import { DaemonError } from '../lib/api';

export interface ProjectSettingsProps {
  project: Project;
  /** Re-fetch project data after a mutation succeeds. Sidebar / switcher
   *  uses this to refresh the cached row + propagate the new enabled flag
   *  through the hook layer's next `projectByCwd` call. */
  onProjectChange: () => Promise<unknown>;
  /** Called after a successful delete so the host can navigate away. */
  onDeleted?: () => void;
}

function toastDaemonError(e: unknown, fallback: string) {
  if (e instanceof DaemonError) {
    toastError(`${e.code}: ${e.message}`);
  } else {
    toastError(`${fallback}: ${(e as Error)?.message ?? String(e)}`);
  }
}

export function ProjectSettings({
  project,
  onProjectChange,
  onDeleted,
}: ProjectSettingsProps) {
  const [editingName, setEditingName] = useState(false);
  const [editingDesc, setEditingDesc] = useState(false);
  const [nameValue, setNameValue] = useState('');
  const [descValue, setDescValue] = useState('');
  const [newCwd, setNewCwd] = useState('');
  const [newWikiPath, setNewWikiPath] = useState('');
  const [deleteConfirm, setDeleteConfirm] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const saveName = useCallback(async () => {
    if (!nameValue.trim()) return;
    try {
      await updateProject(project.id, { name: nameValue.trim() });
      await onProjectChange();
      setEditingName(false);
      toastSuccess('Project name updated');
    } catch (e) {
      toastDaemonError(e, 'Update name failed');
    }
  }, [project.id, nameValue, onProjectChange]);

  const saveDesc = useCallback(async () => {
    const next = descValue.trim();
    try {
      await updateProject(project.id, {
        description: next.length > 0 ? next : null,
      });
      await onProjectChange();
      setEditingDesc(false);
      toastSuccess('Project description updated');
    } catch (e) {
      toastDaemonError(e, 'Update description failed');
    }
  }, [project.id, descValue, onProjectChange]);

  const addCwd = useCallback(async () => {
    if (!newCwd.trim()) return;
    try {
      await addProjectCwd(project.id, newCwd.trim());
      setNewCwd('');
      await onProjectChange();
    } catch (e) {
      toastDaemonError(e, 'Attach cwd failed');
    }
  }, [project.id, newCwd, onProjectChange]);

  const removeCwd = useCallback(
    async (cwd: string) => {
      try {
        await removeProjectCwd(project.id, cwd);
        await onProjectChange();
      } catch (e) {
        toastDaemonError(e, 'Detach cwd failed');
      }
    },
    [project.id, onProjectChange],
  );

  const replaceWikiPaths = useCallback(
    async (next: string[]) => {
      try {
        await updateProject(project.id, { wiki_paths: next });
        await onProjectChange();
      } catch (e) {
        toastDaemonError(e, 'Update wiki paths failed');
      }
    },
    [project.id, onProjectChange],
  );

  const toggleEnabled = useCallback(async () => {
    try {
      if (project.enabled) {
        await disableProject(project.id);
        toastSuccess('Project disabled — SDI hooks will skip its cwds');
      } else {
        await enableProject(project.id);
        toastSuccess('Project enabled — SDI governance restored');
      }
      await onProjectChange();
    } catch (e) {
      toastDaemonError(e, 'Toggle enabled failed');
    }
  }, [project.id, project.enabled, onProjectChange]);

  const requestDelete = useCallback(
    async (force: boolean) => {
      setDeleting(true);
      try {
        await deleteProject(project.id, { force });
        toastSuccess(`Deleted ${project.id}`);
        setDeleteConfirm(false);
        if (onDeleted) onDeleted();
      } catch (e) {
        toastDaemonError(e, 'Delete failed');
        setDeleting(false);
      }
    },
    [project.id, onDeleted],
  );

  const wikiPaths = project.wiki_paths.length > 0 ? project.wiki_paths : ['docs'];

  return (
    <div
      data-testid="project-settings"
      className="space-y-4 rounded-lg border border-border bg-surface p-4"
    >
      {/* Name */}
      <div>
        <label className="text-xs text-muted block mb-1">Name</label>
        {editingName ? (
          <div className="flex gap-2">
            <input
              className="flex-1 bg-background border border-border rounded px-2 py-1.5 text-sm text-foreground focus:outline-none focus:border-primary"
              value={nameValue}
              onChange={(e) => setNameValue(e.target.value)}
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter') saveName();
                else if (e.key === 'Escape') setEditingName(false);
              }}
            />
            <button
              onClick={saveName}
              className="px-2 py-1 text-xs bg-primary text-primary-foreground rounded hover:opacity-90 cursor-pointer"
            >
              Save
            </button>
            <button
              onClick={() => setEditingName(false)}
              className="px-2 py-1 text-xs text-muted hover:text-foreground cursor-pointer"
            >
              Cancel
            </button>
          </div>
        ) : (
          <button
            onClick={() => {
              setNameValue(project.name);
              setEditingName(true);
            }}
            className="text-sm text-foreground hover:text-primary cursor-pointer"
          >
            {project.name}
          </button>
        )}
      </div>

      {/* Description */}
      <div>
        <label className="text-xs text-muted block mb-1">Description</label>
        {editingDesc ? (
          <div className="flex gap-2">
            <input
              className="flex-1 bg-background border border-border rounded px-2 py-1.5 text-sm text-foreground focus:outline-none focus:border-primary"
              value={descValue}
              onChange={(e) => setDescValue(e.target.value)}
              placeholder="What this project is for"
              autoFocus
              onKeyDown={(e) => {
                if (e.key === 'Enter') saveDesc();
                else if (e.key === 'Escape') setEditingDesc(false);
              }}
            />
            <button
              onClick={saveDesc}
              className="px-2 py-1 text-xs bg-primary text-primary-foreground rounded hover:opacity-90 cursor-pointer"
            >
              Save
            </button>
            <button
              onClick={() => setEditingDesc(false)}
              className="px-2 py-1 text-xs text-muted hover:text-foreground cursor-pointer"
            >
              Cancel
            </button>
          </div>
        ) : (
          <button
            onClick={() => {
              setDescValue(project.description ?? '');
              setEditingDesc(true);
            }}
            className="text-sm text-foreground hover:text-primary cursor-pointer"
          >
            {project.description ?? (
              <span className="text-muted italic">No description</span>
            )}
          </button>
        )}
      </div>

      {/* Working directories */}
      <div>
        <label className="text-xs text-muted block mb-1">Working Directories</label>
        <div className="space-y-1.5">
          {project.cwds.map((cwd) => (
            <div key={cwd} className="flex items-center gap-2 group">
              <span
                className="text-xs font-mono text-foreground bg-surface-high rounded px-2 py-1 flex-1 truncate"
                title={cwd}
              >
                {cwd}
              </span>
              <button
                onClick={() => removeCwd(cwd)}
                className="text-xs text-muted hover:text-danger opacity-0 group-hover:opacity-100 transition-opacity cursor-pointer px-1"
                title="Remove"
              >
                ×
              </button>
            </div>
          ))}
          {project.cwds.length === 0 && (
            <div className="text-xs text-muted italic">No working directories</div>
          )}
        </div>
        <div className="flex gap-2 mt-2">
          <input
            className="flex-1 bg-background border border-border rounded px-2 py-1.5 text-xs font-mono text-foreground focus:outline-none focus:border-primary"
            value={newCwd}
            onChange={(e) => setNewCwd(e.target.value)}
            placeholder="/path/to/project"
            onKeyDown={(e) => {
              if (e.key === 'Enter') addCwd();
            }}
          />
          <button
            onClick={addCwd}
            className="px-2 py-1 text-xs bg-primary text-primary-foreground rounded hover:opacity-90 cursor-pointer"
          >
            Add
          </button>
        </div>
      </div>

      {/* Wiki paths */}
      <div>
        <label className="text-xs text-muted block mb-1">Wiki Paths</label>
        <div className="space-y-1.5">
          {wikiPaths.map((wp, i) => (
            <div key={`${wp}-${i}`} className="flex items-center gap-2 group">
              <span
                className="text-xs font-mono text-foreground bg-surface-high rounded px-2 py-1 flex-1 truncate"
                title={wp}
              >
                {wp}
              </span>
              {wikiPaths.length > 1 && (
                <button
                  onClick={() =>
                    replaceWikiPaths(wikiPaths.filter((_, idx) => idx !== i))
                  }
                  className="text-xs text-muted hover:text-danger opacity-0 group-hover:opacity-100 transition-opacity cursor-pointer px-1"
                  title="Remove"
                >
                  ×
                </button>
              )}
            </div>
          ))}
        </div>
        <div className="flex gap-2 mt-2">
          <input
            className="flex-1 bg-background border border-border rounded px-2 py-1.5 text-xs font-mono text-foreground focus:outline-none focus:border-primary"
            value={newWikiPath}
            onChange={(e) => setNewWikiPath(e.target.value)}
            placeholder="docs, wiki, or /absolute/path"
            onKeyDown={(e) => {
              if (e.key === 'Enter' && newWikiPath.trim()) {
                replaceWikiPaths([...wikiPaths, newWikiPath.trim()]);
                setNewWikiPath('');
              }
            }}
          />
          <button
            onClick={() => {
              if (!newWikiPath.trim()) return;
              replaceWikiPaths([...wikiPaths, newWikiPath.trim()]);
              setNewWikiPath('');
            }}
            className="px-2 py-1 text-xs bg-primary text-primary-foreground rounded hover:opacity-90 cursor-pointer"
          >
            Add
          </button>
        </div>
        <p className="text-[10px] text-muted mt-1">
          Relative to project cwd, or absolute paths. Each path becomes a root in
          the wiki tree.
        </p>
      </div>

      {/* SDI governance toggle */}
      <div>
        <label className="text-xs text-muted block mb-1">SDI Governance</label>
        <div className="flex items-center gap-3">
          <button
            data-testid="project-settings-enabled-toggle"
            onClick={toggleEnabled}
            className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors cursor-pointer ${
              project.enabled ? 'bg-success' : 'bg-muted/30'
            }`}
          >
            <span
              className={`inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform ${
                project.enabled ? 'translate-x-4.5' : 'translate-x-0.5'
              }`}
            />
          </button>
          <span className="text-xs text-foreground">
            {project.enabled
              ? 'Active — hooks enforce task registration, D21 + D29 gates run'
              : 'Disabled — Claude operates on this repo without SDI gates'}
          </span>
        </div>
      </div>

      {/* Project id */}
      <div>
        <label className="text-xs text-muted block mb-1">Project ID</label>
        <span className="text-xs font-mono text-muted">{project.id}</span>
      </div>

      {/* Danger zone */}
      <div className="pt-2 mt-2 border-t border-border">
        <label className="text-xs text-muted block mb-1">Danger Zone</label>
        {!deleteConfirm ? (
          <button
            data-testid="project-settings-delete"
            onClick={() => setDeleteConfirm(true)}
            className="px-2 py-1 text-xs text-danger border border-danger/30 rounded hover:bg-danger/10 cursor-pointer"
          >
            Delete project…
          </button>
        ) : (
          <div className="space-y-2">
            <p className="text-xs text-danger">
              This will delete the project and every PROJ-scoped row across the
              schema (plans, scenarios, rounds, tasks, decisions, knowledge,
              patterns, agent notes, activity events). Irreversible.
            </p>
            <div className="flex gap-2 flex-wrap">
              <button
                onClick={() => requestDelete(false)}
                disabled={deleting}
                className="px-2 py-1 text-xs text-danger border border-danger/30 rounded hover:bg-danger/10 disabled:opacity-50 cursor-pointer"
              >
                Delete (refuse if tasks are in_progress)
              </button>
              <button
                onClick={() => requestDelete(true)}
                disabled={deleting}
                className="px-2 py-1 text-xs text-white bg-danger rounded hover:opacity-90 disabled:opacity-50 cursor-pointer"
              >
                Force delete
              </button>
              <button
                onClick={() => setDeleteConfirm(false)}
                disabled={deleting}
                className="px-2 py-1 text-xs text-muted hover:text-foreground cursor-pointer"
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
