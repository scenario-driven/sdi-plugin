use super::{sha256_hex, FailureClass};
use serde::{Deserialize, Serialize};

/// Computed provenance describes one primary diagnostic, not every failure in
/// a report. Unsupported reports explicitly retain conservative accounting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureCauseIdentity {
    pub version: u8,
    pub basis: String,
    pub diagnostic: Option<String>,
    pub signature: String,
}

/// Derive the first supported actionable diagnostic in report order. Test,
/// task and stack-frame labels never participate in the identity.
pub fn derive_failure_cause(
    contract_id: &str,
    class: FailureClass,
    error_class: &str,
    output: &str,
    in_repo_path: &dyn Fn(&str) -> Option<String>,
) -> FailureCauseIdentity {
    let clean = strip_ansi(output);
    let lines: Vec<_> = clean.lines().collect();
    let diagnostic = lines
        .iter()
        .enumerate()
        .find_map(|(index, line)| {
            if is_wrapper_line(line) {
                return None;
            }
            if let Some(mut diagnostic) = compiler_diagnostic(line, in_repo_path) {
                if line.contains("error[E") {
                    for continuation in &lines[index + 1..] {
                        let continuation = continuation.trim();
                        if continuation.starts_with("error")
                            || continuation.starts_with("warning")
                            || continuation.starts_with("thread '")
                            || continuation.starts_with("test ")
                        {
                            break;
                        }
                        let detail = continuation
                            .strip_prefix("= note: ")
                            .or_else(|| rust_span_detail(continuation))
                            .unwrap_or(continuation);
                        if detail.starts_with("expected ") || detail.starts_with("found ") {
                            diagnostic.push_str(" | ");
                            diagnostic.push_str(&normalize(detail, in_repo_path));
                        }
                    }
                }
                if diagnostic == "compiler:error[E0308]:mismatched types" {
                    Some(None)
                } else {
                    Some(Some(diagnostic))
                }
            } else {
                startup_diagnostic(line).map(Some)
            }
        })
        .flatten();
    let (basis, detail, namespace) = match diagnostic.as_deref() {
        Some(diagnostic) => ("primary-diagnostic", diagnostic, "sig:v2:"),
        None => (
            "coarse-fallback",
            broad_error_class(error_class),
            "sig:v2:fallback:",
        ),
    };
    let material = format!(
        "sdi-failure-cause-v2\ncontract:{contract_id}\nclass:{class}\nbasis:{basis}\ndiagnostic:{detail}\n"
    );
    FailureCauseIdentity {
        version: 2,
        basis: basis.into(),
        diagnostic,
        signature: format!("{namespace}{}", sha256_hex(material.as_bytes())),
    }
}

fn rust_span_detail(line: &str) -> Option<&str> {
    let annotation = line.strip_prefix('|')?.trim_start();
    if !annotation.starts_with(['^', '-']) {
        return None;
    }
    let detail = annotation.trim_start_matches(|c: char| c == '^' || c == '-' || c.is_whitespace());
    (detail.starts_with("expected ") || detail.starts_with("found ")).then_some(detail)
}

fn broad_error_class(class: &str) -> &str {
    match class {
        "compile-error" | "assertion" | "panic" | "timeout" | "missing-binary"
        | "permission-denied" | "spawn-error" | "not-executable" | "oom" | "disk-full"
        | "network" | "port-in-use" | "fd-exhausted" => class,
        _ => "unknown",
    }
}

fn compiler_diagnostic(
    line: &str,
    in_repo_path: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    let line = line.trim();
    // Source locations are routing context; identifiers and types in the
    // diagnostic itself distinguish otherwise identical compiler codes.
    let start = line.find("error TS").or_else(|| line.find("error[E"));
    if let Some(start) = start {
        let diagnostic = &line[start..];
        let (code, message) = diagnostic.split_once(": ")?;
        if message.trim().is_empty() || !code.chars().any(|c| c.is_ascii_digit()) {
            return None;
        }
        return Some(format!(
            "compiler:{}:{}",
            code,
            normalize(message, in_repo_path)
        ));
    }
    // Rust also emits actionable compiler errors without an error code.
    let message = line.strip_prefix("error: ")?;
    if message.starts_with("could not compile") || message.starts_with("aborting due to") {
        return None;
    }
    let known = [
        "cannot ",
        "unresolved ",
        "mismatched ",
        "expected ",
        "missing ",
    ];
    known
        .iter()
        .any(|prefix| message.starts_with(prefix))
        .then(|| format!("compiler:{}", normalize(message, in_repo_path)))
}

fn is_wrapper_line(line: &str) -> bool {
    let line = line.trim();
    line.starts_with("test ")
        || line.starts_with("thread '")
        || line.starts_with("---- ")
        || line.starts_with("--- FAIL:")
        || line.starts_with("FAILED ")
        || line.starts_with("at ")
        || line.starts_with("--> ")
        || line.starts_with("● ")
        || line.starts_with("failures:")
        || line.contains(" | ")
        || line
            .split_once(':')
            .is_some_and(|(frame, _)| frame.chars().all(|c| c.is_ascii_digit()))
}

fn startup_diagnostic(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    if (lower.contains("sun_len") || lower.contains("socket") || lower.contains("unix"))
        && lower.contains("path")
        && (lower.contains("too long")
            || lower.contains("exceed")
            || lower.contains("shorter than")
            || lower.contains("length limit"))
    {
        return Some("startup:unix-socket-path-limit".into());
    }
    let mismatch = ["mismatch", "incompatible", "unsupported", "does not match"]
        .iter()
        .any(|marker| lower.contains(marker));
    if mismatch && (lower.contains("protocol") || lower.contains("schema")) {
        return Some(format!(
            "startup:{}-mismatch",
            if lower.contains("protocol") {
                "protocol"
            } else {
                "schema"
            }
        ));
    }
    if (lower.contains("database") || lower.contains("db ") || lower.contains("writerlock"))
        && (lower.contains("already owned")
            || lower.contains("another process")
            || lower.contains("owner lock")
            || lower.contains("already locked"))
    {
        return Some("startup:database-owner-lifetime".into());
    }
    None
}

fn strip_ansi(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) {
                    break;
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn normalize(message: &str, in_repo_path: &dyn Fn(&str) -> Option<String>) -> String {
    let words: Vec<_> = message.split_whitespace().collect();
    words
        .iter()
        .enumerate()
        .map(|(index, word)| {
            let token = word
                .trim_matches(|c: char| "\"'`()[],;".contains(c))
                .trim_end_matches(['.', ':']);
            let token = token
                .split_once('=')
                .map(|(_, value)| value)
                .unwrap_or(token);
            let uuid = token.len() == 36
                && token.chars().enumerate().all(|(i, c)| {
                    if [8, 13, 18, 23].contains(&i) {
                        c == '-'
                    } else {
                        c.is_ascii_hexdigit()
                    }
                });
            let id_suffix = token.rsplit('-').next().unwrap_or(token);
            let ulid = id_suffix.len() == 26
                && id_suffix
                    .chars()
                    .all(|c| "0123456789ABCDEFGHJKMNPQRSTVWXYZ".contains(c));
            if uuid
                || ulid
                || token.starts_with("0x") && token[2..].chars().all(|c| c.is_ascii_hexdigit())
            {
                return "<id>".into();
            }
            if token.starts_with('/')
                || token.starts_with("file://")
                || token.starts_with('\\')
                || token.as_bytes().get(1) == Some(&b':')
                || in_repo_path(token).is_some()
            {
                return "<path>".into();
            }
            let host = token.split(':').next().unwrap_or(token);
            let ipv4 = host.split('.').count() == 4
                && host.split('.').all(|part| part.parse::<u8>().is_ok());
            if token.starts_with("http://")
                || token.starts_with("https://")
                || token.starts_with("localhost:")
                || ipv4
            {
                return "<address>".into();
            }
            // Literal types and arity are semantic compiler information;
            // only explicitly runtime numeric context is removed.
            let quoted = word.starts_with(['\'', '"', '`']);
            let previous = index
                .checked_sub(1)
                .map(|i| words[i].trim_end_matches([':', '=']))
                .unwrap_or("");
            let runtime_number = ["port", "line", "column", "pid", "duration"].contains(&previous)
                || ["port=", "pid=", "line=", "column="]
                    .iter()
                    .any(|prefix| word.starts_with(prefix))
                || token.contains('T') && token.contains('-') && token.contains(':');
            if !quoted && runtime_number && token.chars().any(|c| c.is_ascii_digit()) {
                return "<number>".into();
            }
            (*word).to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests;
