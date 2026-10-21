//! Workspace snapshot reader — the filesystem half of the input fingerprint.
//!
//! A verification run's inputs are the content of every path matched by its
//! declared globs (whole repository when none are declared). Content is
//! identified by git object id so the token is the same whether a file is
//! committed, staged, or dirty-but-identical: tracked clean files use the
//! index oid from `git ls-files -s`; files reported by `git status` (modified,
//! untracked, staged-then-edited) are re-hashed from the working tree with
//! `git hash-object`, which applies the same filters git itself would. Missing
//! files map to `deleted`. Ignored files are never inputs.
//!
//! All git commands run at the repository top level so `ls-files` and
//! `status --porcelain` (which always prints top-level-relative paths) agree
//! on path spelling; globs given relative to `cwd` are re-rooted with the
//! `--show-prefix` of `cwd`.

use sdi_core::error::{DomainError, DomainResult};
use sdi_core::verification::{compute_fingerprint, FingerprintMaterial};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Override for the git executable (absolute path), for daemons launched
/// with a minimal PATH.
pub const ENV_GIT_BIN: &str = "SDI_GIT_BIN";

fn git_bin() -> String {
    std::env::var(ENV_GIT_BIN).unwrap_or_else(|_| "git".into())
}

fn fp_err(msg: impl Into<String>) -> DomainError {
    DomainError::Validation(format!("input fingerprint: {}", msg.into()))
}

fn git(dir: &Path, args: &[&str], stdin: Option<&[u8]>) -> DomainResult<Vec<u8>> {
    let mut cmd = Command::new(git_bin());
    cmd.arg("-C")
        .arg(dir)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    let mut child = cmd
        .spawn()
        .map_err(|e| fp_err(format!("cannot run git ({}): {e}", git_bin())))?;
    if let Some(input) = stdin {
        let mut pipe = child
            .stdin
            .take()
            .ok_or_else(|| fp_err("git stdin unavailable"))?;
        pipe.write_all(input)
            .map_err(|e| fp_err(format!("write git stdin: {e}")))?;
    }
    let out = child
        .wait_with_output()
        .map_err(|e| fp_err(format!("wait for git: {e}")))?;
    if !out.status.success() {
        return Err(fp_err(format!(
            "git {} failed in {}: {}",
            args.join(" "),
            dir.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(out.stdout)
}

fn git_line(dir: &Path, args: &[&str]) -> DomainResult<String> {
    let raw = git(dir, args, None)?;
    Ok(String::from_utf8_lossy(&raw)
        .trim_end_matches('\n')
        .to_string())
}

/// `HEAD` commit of the repository containing `cwd`, if it is a git work tree
/// with at least one commit.
pub fn head_commit(cwd: &Path) -> Option<String> {
    git_line(cwd, &["rev-parse", "HEAD"])
        .ok()
        .filter(|s| !s.is_empty())
}

/// Top level of the git work tree containing `cwd`.
pub fn toplevel(cwd: &Path) -> Option<PathBuf> {
    git_line(cwd, &["rev-parse", "--show-toplevel"])
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// Content tokens for every input path, keyed by top-level-relative path.
#[derive(Debug, Clone)]
pub struct WorkspaceSnapshot {
    pub toplevel: PathBuf,
    pub entries: BTreeMap<String, String>,
}

fn pathspecs(prefix: &str, globs: &[String]) -> DomainResult<Vec<String>> {
    let mut out = Vec::with_capacity(globs.len());
    for g in globs {
        let g = g.trim();
        if g.is_empty() {
            continue;
        }
        if g.starts_with(':') {
            // Explicit pathspec magic: the caller owns its semantics.
            out.push(g.to_string());
            continue;
        }
        if Path::new(g).is_absolute() {
            return Err(fp_err(format!(
                "input glob must be relative to the run cwd, got absolute `{g}`"
            )));
        }
        let rel = g.trim_start_matches("./");
        out.push(format!(":(glob){prefix}{rel}"));
    }
    Ok(out)
}

/// Snapshot the inputs of a run executed in `cwd` with the declared `globs`.
pub fn snapshot(cwd: &Path, globs: &[String]) -> DomainResult<WorkspaceSnapshot> {
    if !cwd.is_dir() {
        return Err(fp_err(format!("cwd {} does not exist", cwd.display())));
    }
    let toplevel = PathBuf::from(git_line(cwd, &["rev-parse", "--show-toplevel"]).map_err(
        |_| {
            fp_err(format!(
                "{} is not inside a git work tree (the fingerprint is content-addressed by git)",
                cwd.display()
            ))
        },
    )?);
    let prefix = git_line(cwd, &["rev-parse", "--show-prefix"])?;
    let specs = pathspecs(&prefix, globs)?;

    let mut entries: BTreeMap<String, String> = BTreeMap::new();

    // Tracked files: index oid. Gitlinks (submodules, mode 160000) keep the
    // recorded commit oid — the submodule's own tree is not an input here.
    let mut ls_args: Vec<&str> = vec!["ls-files", "-s", "-z", "--"];
    ls_args.extend(specs.iter().map(String::as_str));
    let ls = git(&toplevel, &ls_args, None)?;
    for rec in ls.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let rec = String::from_utf8_lossy(rec);
        let Some((meta, path)) = rec.split_once('\t') else {
            continue;
        };
        let mut parts = meta.split_whitespace();
        let (_mode, oid) = (parts.next(), parts.next());
        if let Some(oid) = oid {
            entries.insert(path.to_string(), format!("blob:{oid}"));
        }
    }

    // Dirty + untracked files: re-hash from the working tree.
    let mut st_args: Vec<&str> = vec![
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
        "--no-renames",
        "--",
    ];
    st_args.extend(specs.iter().map(String::as_str));
    let st = git(&toplevel, &st_args, None)?;
    let mut to_hash: Vec<String> = Vec::new();
    for rec in st.split(|b| *b == 0).filter(|r| r.len() > 3) {
        let rec = String::from_utf8_lossy(rec);
        let path = rec[3..].to_string();
        let abs = toplevel.join(&path);
        match std::fs::symlink_metadata(&abs) {
            Err(_) => {
                entries.insert(path, "deleted".into());
            }
            Ok(meta) if meta.is_dir() => {
                // Dirty submodule / nested repo: its content is outside this
                // repository's object store; keep the ls-files token.
            }
            Ok(_) => to_hash.push(path),
        }
    }
    if !to_hash.is_empty() {
        let stdin = to_hash.join("\n") + "\n";
        let out = git(
            &toplevel,
            &["hash-object", "--stdin-paths"],
            Some(stdin.as_bytes()),
        )?;
        let oids: Vec<String> = String::from_utf8_lossy(&out)
            .lines()
            .map(str::to_string)
            .collect();
        if oids.len() != to_hash.len() {
            return Err(fp_err("git hash-object returned a mismatched oid count"));
        }
        for (path, oid) in to_hash.into_iter().zip(oids) {
            entries.insert(path, format!("blob:{oid}"));
        }
    }
    Ok(WorkspaceSnapshot { toplevel, entries })
}

/// Compute the input fingerprint of a run.
pub fn fingerprint(
    cwd: &Path,
    globs: &[String],
    command: &[String],
    tool_versions: &BTreeMap<String, String>,
    contract: &str,
) -> DomainResult<String> {
    let snap = snapshot(cwd, globs)?;
    Ok(compute_fingerprint(&FingerprintMaterial {
        contract,
        command,
        tool_versions,
        input_globs: globs,
        entries: &snap.entries,
    }))
}

/// Map a path token from test output to a repository-relative path when it
/// points at an existing file inside `toplevel` and outside vendored /
/// dependency trees. Used to find the top in-repo stack frame.
pub fn in_repo_path(toplevel: &Path, cwd: &Path, raw: &str) -> Option<String> {
    const EXCLUDED: &[&str] = &[
        "node_modules/",
        "/.cargo/",
        "/rustc/",
        "target/",
        "/site-packages/",
        "/dist-packages/",
        "vendor/",
    ];
    let raw = raw.trim();
    if raw.is_empty() || EXCLUDED.iter().any(|x| raw.contains(x)) {
        return None;
    }
    let candidate = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        cwd.join(raw.trim_start_matches("./"))
    };
    let canon = candidate.canonicalize().ok()?;
    let top = toplevel.canonicalize().ok()?;
    if !canon.is_file() {
        return None;
    }
    let rel = canon.strip_prefix(&top).ok()?;
    Some(rel.to_string_lossy().replace('\\', "/"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(dir: &Path, args: &[&str]) {
        let ok = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.email=t@t", "-c", "user.name=t"])
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn repo() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sdi-ws-{}-{}",
            std::process::id(),
            ulid::Ulid::new()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        sh(&dir, &["init", "-q"]);
        std::fs::write(dir.join("src/a.rs"), "fn a() {}\n").unwrap();
        std::fs::write(dir.join("README.md"), "r\n").unwrap();
        sh(&dir, &["add", "."]);
        sh(&dir, &["commit", "-qm", "init"]);
        dir
    }

    #[test]
    fn fingerprint_tracks_content_not_commit_state() {
        let dir = repo();
        let cmd = vec!["true".to_string()];
        let tools = BTreeMap::new();
        let f0 = fingerprint(&dir, &[], &cmd, &tools, "c").unwrap();
        std::fs::write(dir.join("src/a.rs"), "fn a() { 1; }\n").unwrap();
        let f1 = fingerprint(&dir, &[], &cmd, &tools, "c").unwrap();
        assert_ne!(f0, f1, "dirty edit must change the fingerprint");
        // Committing the identical content keeps the fingerprint: it is
        // content-addressed, not commit-addressed.
        sh(&dir, &["commit", "-qam", "edit"]);
        let f2 = fingerprint(&dir, &[], &cmd, &tools, "c").unwrap();
        assert_eq!(f1, f2);
        // Reverting restores the original fingerprint.
        std::fs::write(dir.join("src/a.rs"), "fn a() {}\n").unwrap();
        assert_eq!(f0, fingerprint(&dir, &[], &cmd, &tools, "c").unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn globs_scope_the_inputs() {
        let dir = repo();
        let cmd = vec!["true".to_string()];
        let tools = BTreeMap::new();
        let globs = vec!["src/**".to_string()];
        let f0 = fingerprint(&dir, &globs, &cmd, &tools, "c").unwrap();
        std::fs::write(dir.join("README.md"), "changed\n").unwrap();
        assert_eq!(f0, fingerprint(&dir, &globs, &cmd, &tools, "c").unwrap());
        std::fs::write(dir.join("src/new.rs"), "x\n").unwrap();
        assert_ne!(f0, fingerprint(&dir, &globs, &cmd, &tools, "c").unwrap());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn non_git_cwd_is_rejected() {
        let dir = std::env::temp_dir().join(format!("sdi-ws-nogit-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(&dir).unwrap();
        let err = fingerprint(&dir, &[], &[], &BTreeMap::new(), "c").unwrap_err();
        assert!(err.to_string().contains("git work tree"), "{err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
