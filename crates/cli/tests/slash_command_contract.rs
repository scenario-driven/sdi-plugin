//! Contract test — every `sdi …` shell example inside the plugin's slash
//! command docs (`plugin/commands/*.md`) and the bundled skill
//! (`plugin/skills/sdi/SKILL.md`) MUST parse against the real clap App.
//!
//! Why: the slash command markdown is what the LLM reads at runtime when the
//! human types `/scenario` or `/round`. If a doc references a flag the CLI
//! never had (e.g. `--verdict` vs the real `--result`, or fictional
//! `--in-flight` flags) the LLM will issue a command the daemon rejects and
//! the user sees a generic clap error instead of acting on intent.
//!
//! Strategy: walk every fenced-code block, extract lines starting with
//! `sdi …`, normalize them into argv arrays, and feed them to
//! `clap::Command::try_get_matches_from`. Placeholder tokens like
//! `<PLAN-ID>`, `<SHORT-CODE>`, `"<title>"` are accepted as opaque values —
//! the test only validates that the flag and subcommand surface matches.

use clap::CommandFactory;
use sdi_cli::cli::App;
use std::path::{Path, PathBuf};

fn plugin_root() -> PathBuf {
    // crates/cli → ../../plugin
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest.parent().unwrap().parent().unwrap().join("plugin")
}

fn collect_docs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    let commands = plugin_root().join("commands");
    if commands.exists() {
        for entry in std::fs::read_dir(&commands).unwrap() {
            let p = entry.unwrap().path();
            if p.extension().and_then(|s| s.to_str()) == Some("md") {
                out.push(p);
            }
        }
    }
    let skill = plugin_root().join("skills/sdi/SKILL.md");
    if skill.exists() {
        out.push(skill);
    }
    out.sort();
    out
}

/// Extract one logical `sdi …` invocation per fenced code block line.
/// Folds shell continuation backslashes onto one logical line.
fn extract_sdi_invocations(doc: &Path) -> Vec<(usize, String)> {
    let raw = std::fs::read_to_string(doc).unwrap();
    let mut out = Vec::new();
    let mut in_fence = false;
    let mut buf = String::new();
    let mut buf_line: usize = 0;
    for (idx, line) in raw.lines().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            // Flush any pending invocation when the fence closes.
            if in_fence && !buf.is_empty() {
                out.push((buf_line, buf.trim().to_string()));
                buf.clear();
            }
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            continue;
        }
        if buf.is_empty() {
            // Start a new invocation only when we see an `sdi` token at the
            // line head (after any prompt-style `$ ` decoration).
            let head = trimmed.trim_start_matches("$ ").trim_start();
            if head.starts_with("sdi ") || head == "sdi" {
                buf_line = idx + 1;
                buf.push_str(head);
            } else {
                continue;
            }
        } else {
            // Continuation line inside the same fenced code block.
            buf.push(' ');
            buf.push_str(trimmed);
        }
        // If the line does NOT end with a backslash, this invocation is done.
        if !buf.trim_end().ends_with('\\') {
            out.push((buf_line, buf.trim().to_string()));
            buf.clear();
        } else {
            // Drop the trailing backslash so it doesn't reach argv split.
            let s = buf.trim_end().trim_end_matches('\\').to_string();
            buf = s;
        }
    }
    if !buf.is_empty() {
        out.push((buf_line, buf.trim().to_string()));
    }
    out
}

/// Best-effort shell-style argv split. Honors single and double quotes; treats
/// everything else as whitespace-separated tokens. Sufficient for our doc
/// examples — production parsing is shell's job, not ours.
fn split_argv(line: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in line.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (None, '"') | (None, '\'') => quote = Some(ch),
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            (_, c) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Lines we explicitly DO NOT validate against clap: they are pure shell
/// fragments (heredocs, redirections, comments, multi-statement) or they
/// invoke a binary the App doesn't host (`cat`, `node`, etc.). The doc
/// extraction layer filters most of these by requiring an `sdi` head; this
/// list catches the residual cases.
fn should_skip(line: &str) -> bool {
    // Trailing inline comments like `sdi project by-cwd "$(pwd)"          # → project.id`
    if line.contains('#') {
        // Stripped below in normalize_for_clap; never skip outright.
    }
    // Subshell substitution (`$(…)`) — we keep the line but the substitution
    // expands to a single opaque token under our naive splitter, which is what
    // clap needs anyway.
    false
}

/// Strip an inline `# …` comment tail so clap doesn't see it as an arg.
fn strip_inline_comment(line: &str) -> String {
    // Walk the line tracking quote state; cut at the first un-quoted `#`.
    let mut out = String::new();
    let mut quote: Option<char> = None;
    for ch in line.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => {
                quote = None;
                out.push(c);
            }
            (None, '"') | (None, '\'') => {
                quote = Some(ch);
                out.push(ch);
            }
            (None, '#') => break,
            (_, c) => out.push(c),
        }
    }
    out.trim().to_string()
}

fn validate(doc: &Path, app: &mut clap::Command) -> Vec<String> {
    let mut errs = Vec::new();
    for (line_no, raw) in extract_sdi_invocations(doc) {
        if should_skip(&raw) {
            continue;
        }
        let cleaned = strip_inline_comment(&raw);
        if cleaned.is_empty() {
            continue;
        }
        let argv = split_argv(&cleaned);
        if argv.is_empty() || argv[0] != "sdi" {
            continue;
        }
        match app.try_get_matches_from_mut(argv.iter()) {
            Ok(_) => {}
            Err(e) => {
                // Clap returns Err for `--help` / `--version` style "displays"
                // too; the kind tells us if it was a real parse failure.
                use clap::error::ErrorKind::*;
                match e.kind() {
                    DisplayHelp | DisplayVersion | DisplayHelpOnMissingArgumentOrSubcommand => {
                        continue
                    }
                    _ => errs.push(format!(
                        "{}:{} — `{}` did not parse: {}",
                        doc.file_name().unwrap().to_string_lossy(),
                        line_no,
                        cleaned,
                        e.kind(),
                    )),
                }
            }
        }
    }
    errs
}

#[test]
fn every_documented_sdi_invocation_parses_against_the_real_app() {
    let docs = collect_docs();
    assert!(
        !docs.is_empty(),
        "expected to find at least one slash command doc under plugin/"
    );
    let mut app = App::command();
    let mut all_errs = Vec::new();
    for doc in &docs {
        all_errs.extend(validate(doc, &mut app));
    }
    assert!(
        all_errs.is_empty(),
        "slash command docs reference flags / subcommands the CLI does not have:\n{}",
        all_errs.join("\n")
    );
}
