use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};

pub type ProjectId = Id;

/// Default wiki root mirrored from Clawket — every project starts with `docs/`
/// as its single tree root unless the user replaces it. Kept as a named
/// constant so the daemon, CLI, and SQL DEFAULT all reference the same value.
pub fn default_wiki_paths() -> Vec<String> {
    vec!["docs".to_string()]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: ProjectId,
    /// Short ticket prefix (e.g. "SDI"). Globally unique.
    pub key: String,
    pub name: String,
    /// URL-safe slug derived from name. Globally unique.
    pub slug: String,
    /// Anchored working directories — entry to "active project" detection.
    pub cwds: Vec<String>,
    /// Free-form description shown in the dashboard project settings.
    /// `None` means the user has not authored one (distinct from the empty
    /// string, which clears a previously-set description).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Soft-disable flag. When `false`, the SDI hook layer (`projectByCwd`
    /// consumers in `plugin/adapters/shared/sdi-hooks.cjs`) skips every
    /// mutating gate for cwds anchored to this project — Claude Code
    /// operates without SDI governance until the user re-enables.
    /// Default `true` so new + existing projects stay protected.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// Wiki tree roots (relative to the project cwd, or absolute). Default
    /// is a single `docs` entry. Drives the dashboard `WikiView` tree.
    #[serde(default = "default_wiki_paths")]
    pub wiki_paths: Vec<String>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

fn default_enabled() -> bool {
    true
}
