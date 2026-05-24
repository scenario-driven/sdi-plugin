//! XDG paths + LM-8 invariant (carried from Clawket).
//!
//! User data MUST NOT resolve under `~/.claude/plugins/`. Each of the five user
//! data paths is checked; any overlap aborts daemon startup.

use sdi_core::error::{DomainError, DomainResult};
use std::path::{Path, PathBuf};

pub const ENV_ALLOW_OVERLAP: &str = "SDI_ALLOW_PLUGIN_OVERLAP";
pub const ENV_HOME_OVERRIDE: &str = "SDI_HOME";

/// Resolved user-data paths (PRD §5.3, LM-8 invariant).
#[derive(Debug, Clone)]
pub struct Paths {
    pub data: PathBuf,
    pub cache: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
    pub db_file: PathBuf,
    pub socket_file: PathBuf,
    pub pid_file: PathBuf,
    pub port_file: PathBuf,
    pub log_file: PathBuf,
}

impl Paths {
    /// Resolve paths from XDG. `SDI_HOME` lets tests pin a base directory.
    pub fn resolve() -> DomainResult<Self> {
        let base = std::env::var(ENV_HOME_OVERRIDE)
            .ok()
            .map(PathBuf::from)
            .or_else(dirs::home_dir);
        let home = base.ok_or_else(|| {
            DomainError::Validation(
                "could not determine home directory (and SDI_HOME unset)".into(),
            )
        })?;

        // Allow tests/sandboxes to plant XDG roots under a custom $SDI_HOME.
        let data = home.join(".local/share/sdi");
        let cache = home.join(".cache/sdi");
        let config = home.join(".config/sdi");
        let state = home.join(".local/state/sdi");

        let p = Paths {
            db_file: data.join("sdi.db"),
            socket_file: cache.join("sdid.sock"),
            pid_file: cache.join("sdid.pid"),
            port_file: cache.join("sdid.port"),
            log_file: state.join("sdid.log"),
            data,
            cache,
            config,
            state,
        };

        p.ensure_no_plugin_overlap()?;
        Ok(p)
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        for p in [&self.data, &self.cache, &self.config, &self.state] {
            std::fs::create_dir_all(p)?;
        }
        Ok(())
    }

    /// LM-8 — refuse any user-data path that resolves under `~/.claude/plugins/`.
    pub fn ensure_no_plugin_overlap(&self) -> DomainResult<()> {
        if std::env::var(ENV_ALLOW_OVERLAP).is_ok() {
            tracing::warn!("SDI_ALLOW_PLUGIN_OVERLAP set — LM-8 invariant bypassed");
            return Ok(());
        }
        let plugin_root = dirs::home_dir().map(|h| h.join(".claude/plugins"));
        let Some(root) = plugin_root else {
            return Ok(());
        };
        for (name, path) in [
            ("data", &self.data),
            ("cache", &self.cache),
            ("config", &self.config),
            ("state", &self.state),
            ("db_file", &self.db_file),
        ] {
            if starts_with_resolved(path, &root) {
                return Err(DomainError::PathInvariant(format!(
                    "{name} path {} resolves under {} — LM-8 invariant violated",
                    path.display(),
                    root.display()
                )));
            }
        }
        Ok(())
    }
}

fn starts_with_resolved(candidate: &Path, prefix: &Path) -> bool {
    let cand = candidate
        .canonicalize()
        .unwrap_or_else(|_| candidate.to_path_buf());
    let pref = prefix
        .canonicalize()
        .unwrap_or_else(|_| prefix.to_path_buf());
    cand.starts_with(pref)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_under_sdi_home() {
        let tmp = std::env::temp_dir().join(format!("sdi-paths-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        std::env::set_var(ENV_HOME_OVERRIDE, &tmp);
        std::env::remove_var(ENV_ALLOW_OVERLAP);
        let p = Paths::resolve().unwrap();
        assert!(p.data.starts_with(&tmp));
        assert!(p.cache.starts_with(&tmp));
        assert_eq!(p.db_file.file_name().unwrap(), "sdi.db");
        std::fs::remove_dir_all(&tmp).ok();
    }
}
