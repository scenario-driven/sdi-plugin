//! Verification-run integrity: failure classification, failure signatures,
//! input fingerprints, evidence references and the retry budget.
//!
//! Everything here is pure (no I/O) so the daemon and the CLI derive the same
//! verdicts from the same inputs. Reading the workspace (git blobs, dirty file
//! contents) is the storage adapter's job (`sdi_db::workspace`); this module
//! only composes the digests.
//!
//! - **Input fingerprint** — a content digest over everything a verification
//!   run consumed: the scenario contract (GWT), the command argv, the declared
//!   tool versions, and the content oid of every input path. A recorded pass is
//!   reusable only while the recomputed fingerprint is identical.
//! - **Failure class** — who is at fault when a run fails. Only the product
//!   class means "the code under test is wrong"; harness / environment / spec
//!   failures must not be booked against the product.
//! - **Failure signature** — a stable identity for "the same cause": contract
//!   id + failing test id + normalized error class + top in-repo frame. Line
//!   numbers, durations, addresses and ordering noise are normalized away so a
//!   retry of the same defect lands on the same signature no matter which task
//!   name it runs under.

use crate::error::{DomainError, DomainResult};
use crate::ids::{Id, Timestamp};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

/// Default retry budget per (plan, scenario, failure class, signature) when
/// the plan's autonomy policy does not set `retry_budget_cap`.
pub const DEFAULT_RETRY_BUDGET_CAP: i32 = 3;

/// Evidence reference prefix that binds a verdict to a recorded run.
pub const RUN_REF_PREFIX: &str = "run:";

// ---------------------------------------------------------------------------
// Failure class
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FailureClass {
    /// The code under test violates the scenario (assertion, compile error,
    /// panic in product code).
    Product,
    /// The verification harness itself is broken: the command could not be
    /// executed as written, timed out, or was killed by a signal.
    Harness,
    /// The machine / toolchain is unfit: missing binary, OOM, disk full,
    /// network unreachable.
    Environment,
    /// The scenario contract is wrong or untestable as written (caller-asserted).
    Spec,
    /// The run passed.
    None,
}

impl FailureClass {
    pub fn is_failure(self) -> bool {
        !matches!(self, FailureClass::None)
    }

    /// Harness / environment failures say nothing about the product; they
    /// block round completion until a passing run supersedes them.
    pub fn is_infrastructure(self) -> bool {
        matches!(self, FailureClass::Harness | FailureClass::Environment)
    }
}

impl fmt::Display for FailureClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            FailureClass::Product => "product",
            FailureClass::Harness => "harness",
            FailureClass::Environment => "environment",
            FailureClass::Spec => "spec",
            FailureClass::None => "none",
        })
    }
}

impl FromStr for FailureClass {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "product" => Ok(Self::Product),
            "harness" => Ok(Self::Harness),
            "environment" => Ok(Self::Environment),
            "spec" => Ok(Self::Spec),
            "none" => Ok(Self::None),
            other => Err(DomainError::Validation(format!(
                "unknown failure class: {other} (expected product|harness|environment|spec|none)"
            ))),
        }
    }
}

// ---------------------------------------------------------------------------
// Execution outcome → classification
// ---------------------------------------------------------------------------

/// Why the command could not be spawned at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpawnError {
    NotFound,
    PermissionDenied,
    Other,
}

impl fmt::Display for SpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SpawnError::NotFound => "not-found",
            SpawnError::PermissionDenied => "permission-denied",
            SpawnError::Other => "other",
        })
    }
}

impl FromStr for SpawnError {
    type Err = DomainError;
    fn from_str(s: &str) -> DomainResult<Self> {
        match s {
            "not-found" => Ok(Self::NotFound),
            "permission-denied" => Ok(Self::PermissionDenied),
            "other" => Ok(Self::Other),
            other => Err(DomainError::Validation(format!(
                "unknown spawn error: {other} (expected not-found|permission-denied|other)"
            ))),
        }
    }
}

/// Observable process outcome of one verification run.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecOutcome {
    #[serde(default)]
    pub exit_code: Option<i32>,
    /// Terminating signal (unix), when the process did not exit normally.
    #[serde(default)]
    pub signal: Option<i32>,
    #[serde(default)]
    pub timed_out: bool,
    #[serde(default)]
    pub spawn_error: Option<SpawnError>,
}

impl ExecOutcome {
    pub fn passed(&self) -> bool {
        self.exit_code == Some(0)
            && self.signal.is_none()
            && !self.timed_out
            && self.spawn_error.is_none()
    }
}

/// Result of classifying a run from its outcome + captured output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Classification {
    pub failure_class: FailureClass,
    /// Normalized error class (e.g. `assertion`, `timeout`, `missing-binary`).
    pub error_class: String,
    /// Lexicographically smallest failing test id found in the output.
    pub failing_test: Option<String>,
    /// First stack frame that points inside the repository (path only).
    pub top_frame: Option<String>,
    /// Human-readable rule that produced the class.
    pub reason: String,
}

const SIGKILL: i32 = 9;

const OOM_MARKERS: &[&str] = &[
    "out of memory",
    "cannot allocate memory",
    "javascript heap out of memory",
    "memoryerror",
    "oom-kill",
    "oomkilled",
];

const ENVIRONMENT_MARKERS: &[(&str, &str)] = &[
    ("no space left on device", "disk-full"),
    ("enospc", "disk-full"),
    ("econnrefused", "network"),
    ("enotfound", "network"),
    ("eai_again", "network"),
    ("could not resolve host", "network"),
    ("network is unreachable", "network"),
    ("connection timed out", "network"),
    ("eaddrinuse", "port-in-use"),
    ("address already in use", "port-in-use"),
    ("too many open files", "fd-exhausted"),
    ("emfile", "fd-exhausted"),
    ("command not found", "missing-binary"),
    ("no such file or directory (os error 2)", "missing-binary"),
];

const COMPILE_MARKERS: &[&str] = &[
    "error[e",
    "could not compile",
    "error ts",
    "syntaxerror",
    "compilation failed",
    "cannot find module",
    "module not found",
    "unresolved import",
];

const ASSERTION_MARKERS: &[&str] = &[
    "assertion failed",
    "assertion `",
    "assertionerror",
    "assert_eq!",
    "assert!(",
    "expect(",
    "expected:",
    "expected ",
    "to equal",
    "tobe(",
    "test result: failed",
    "--- fail:",
    "failed tests/",
    "tests failed",
    "test failed",
    "✕",
    "✗",
    "failures:",
];

const PANIC_MARKERS: &[&str] = &["panicked at", "panic:", "uncaught exception", "traceback"];

/// Classify a finished run. `in_repo_path` maps a raw path token from the
/// output to its repository-relative form when (and only when) the path is
/// inside the repository under test; it is how the top in-repo frame is found
/// without this module touching the filesystem.
pub fn classify(
    outcome: &ExecOutcome,
    output: &str,
    in_repo_path: &dyn Fn(&str) -> Option<String>,
) -> Classification {
    let lower = output.to_lowercase();
    let failing_test = extract_failing_test(output);
    let top_frame = extract_top_frame(output, in_repo_path);
    let mk = |class: FailureClass, error_class: &str, reason: String| Classification {
        failure_class: class,
        error_class: error_class.to_string(),
        failing_test: failing_test.clone(),
        top_frame: top_frame.clone(),
        reason,
    };

    if outcome.passed() {
        return mk(FailureClass::None, "none", "exit 0".into());
    }
    if let Some(err) = outcome.spawn_error {
        return match err {
            SpawnError::NotFound => mk(
                FailureClass::Environment,
                "missing-binary",
                "command binary not found on PATH".into(),
            ),
            SpawnError::PermissionDenied => mk(
                FailureClass::Harness,
                "not-executable",
                "command is not executable".into(),
            ),
            SpawnError::Other => mk(
                FailureClass::Harness,
                "spawn-failed",
                "command could not be spawned".into(),
            ),
        };
    }
    if outcome.timed_out {
        return mk(
            FailureClass::Harness,
            "timeout",
            "run exceeded its timeout and was killed".into(),
        );
    }
    let oom_text = OOM_MARKERS.iter().any(|m| lower.contains(m));
    if let Some(sig) = outcome.signal {
        if sig == SIGKILL || oom_text {
            return mk(
                FailureClass::Environment,
                "oom",
                format!("killed by signal {sig} (OOM / external kill)"),
            );
        }
        return mk(
            FailureClass::Harness,
            &format!("signal-{sig}"),
            format!("terminated by signal {sig}"),
        );
    }
    match outcome.exit_code {
        Some(127) => {
            return mk(
                FailureClass::Environment,
                "missing-binary",
                "exit 127 (command not found)".into(),
            )
        }
        Some(126) => {
            return mk(
                FailureClass::Harness,
                "not-executable",
                "exit 126 (command not executable)".into(),
            )
        }
        Some(137) => {
            return mk(
                FailureClass::Environment,
                "oom",
                "exit 137 (SIGKILL — OOM killer / external kill)".into(),
            )
        }
        _ => {}
    }
    if oom_text {
        return mk(
            FailureClass::Environment,
            "oom",
            "output reports memory exhaustion".into(),
        );
    }
    let code = outcome
        .exit_code
        .map(|c| c.to_string())
        .unwrap_or_else(|| "?".into());
    if COMPILE_MARKERS.iter().any(|m| lower.contains(m)) {
        return mk(
            FailureClass::Product,
            "compile-error",
            format!("exit {code} with compiler/module errors"),
        );
    }
    if ASSERTION_MARKERS.iter().any(|m| lower.contains(m)) || failing_test.is_some() {
        return mk(
            FailureClass::Product,
            "assertion",
            format!("exit {code} with test assertion failures"),
        );
    }
    if PANIC_MARKERS.iter().any(|m| lower.contains(m)) {
        return mk(
            FailureClass::Product,
            "panic",
            format!("exit {code} with an uncaught panic/exception"),
        );
    }
    if let Some((_, ec)) = ENVIRONMENT_MARKERS.iter().find(|(m, _)| lower.contains(m)) {
        return mk(
            FailureClass::Environment,
            ec,
            format!("exit {code} with environment error ({ec})"),
        );
    }
    // Unrecognized nonzero exit: book it against the product. The product
    // budget is capped and escalates; booking an unknown failure as harness
    // would let a real defect dodge that cap.
    let error_class = first_error_line(output)
        .map(|l| normalize_error_line(&l))
        .unwrap_or_else(|| format!("exit-{code}"));
    mk(
        FailureClass::Product,
        &error_class,
        format!("exit {code} without recognized markers"),
    )
}

/// Pick the lexicographically smallest failing test id so parallel test
/// ordering does not change the signature.
pub fn extract_failing_test(output: &str) -> Option<String> {
    let mut found: Vec<String> = Vec::new();
    for raw in output.lines() {
        let line = raw.trim();
        // cargo test: `test path::name ... FAILED`
        if let Some(rest) = line.strip_prefix("test ") {
            if let Some(name) = rest.strip_suffix(" ... FAILED") {
                found.push(name.trim().to_string());
                continue;
            }
        }
        // cargo test failure section: `---- path::name stdout ----`
        if let Some(rest) = line.strip_prefix("---- ") {
            if let Some(name) = rest.strip_suffix(" stdout ----") {
                found.push(name.trim().to_string());
                continue;
            }
        }
        // go test: `--- FAIL: TestName (0.00s)`
        if let Some(rest) = line.strip_prefix("--- FAIL: ") {
            if let Some(name) = rest.split_whitespace().next() {
                found.push(name.to_string());
                continue;
            }
        }
        // pytest: `FAILED tests/test_x.py::test_y - AssertionError`
        if let Some(rest) = line.strip_prefix("FAILED ") {
            if let Some(name) = rest.split(" - ").next() {
                let name = name.trim();
                if !name.is_empty() && !name.starts_with('(') {
                    found.push(name.to_string());
                    continue;
                }
            }
        }
        // jest / vitest: `● Suite › name`
        if let Some(rest) = line.strip_prefix("● ") {
            let name = rest.trim();
            if !name.is_empty() && !name.starts_with("Console") {
                found.push(name.to_string());
                continue;
            }
        }
    }
    found.sort();
    found.dedup();
    found.into_iter().next()
}

/// First stack-frame-like `path:line` token whose path is inside the repo.
/// The line/column is dropped: a same-cause failure must keep its signature
/// when unrelated edits shift line numbers.
pub fn extract_top_frame(
    output: &str,
    in_repo_path: &dyn Fn(&str) -> Option<String>,
) -> Option<String> {
    for raw in output.lines() {
        let line = raw.trim();
        // python: `File "path/to/x.py", line 12, in fn`
        if let Some(rest) = line.strip_prefix("File \"") {
            if let Some(path) = rest.split('"').next() {
                if let Some(p) = in_repo_path(path) {
                    return Some(p);
                }
            }
            continue;
        }
        for token in line.split(|c: char| c.is_whitespace() || "()[]<>'\",".contains(c)) {
            let token = token.trim_start_matches("file://");
            let Some(colon) = token.find(':') else {
                continue;
            };
            let (path, rest) = token.split_at(colon);
            let after = &rest[1..];
            if path.is_empty() || !after.starts_with(|c: char| c.is_ascii_digit()) {
                continue;
            }
            if !(path.contains('/') || path.contains('.')) {
                continue;
            }
            if let Some(p) = in_repo_path(path) {
                return Some(p);
            }
        }
    }
    None
}

fn first_error_line(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|l| l.to_lowercase().contains("error"))
        .map(str::to_string)
}

/// Normalize an error line into a stable class token: lowercase, digits and
/// hex runs collapsed, quoted payloads and path-like tokens elided, bounded
/// length.
pub fn normalize_error_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_quote: Option<char> = None;
    for word in line.to_lowercase().split_whitespace() {
        if word.contains('/') || word.contains('\\') {
            out.push_str("<path> ");
            continue;
        }
        let mut w = String::new();
        let mut prev_digit = false;
        for c in word.chars() {
            if let Some(q) = in_quote {
                if c == q {
                    in_quote = None;
                    w.push_str("<str>");
                }
                continue;
            }
            if c == '"' || c == '`' {
                in_quote = Some(c);
                continue;
            }
            if c.is_ascii_digit() {
                if !prev_digit {
                    w.push('#');
                }
                prev_digit = true;
                continue;
            }
            prev_digit = false;
            w.push(c);
        }
        if !w.is_empty() {
            out.push_str(&w);
            out.push(' ');
        }
    }
    let trimmed = out.trim();
    trimmed.chars().take(120).collect()
}

// ---------------------------------------------------------------------------
// Signatures + fingerprints
// ---------------------------------------------------------------------------

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(64);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Failure signature: `sig:<sha256>` over the contract id (scenario id, or
/// the command when no scenario is bound), the failure class, the failing test
/// id, the normalized error class and the top in-repo frame. The class is part
/// of the digest so a harness failure can never collide with (and consume) a
/// product signature.
pub fn failure_signature(
    contract_id: &str,
    class: FailureClass,
    failing_test: Option<&str>,
    error_class: &str,
    top_frame: Option<&str>,
) -> String {
    let material = format!(
        "sdi-failure-signature-v1\ncontract:{contract_id}\nclass:{class}\ntest:{}\nerror:{error_class}\nframe:{}\n",
        failing_test.unwrap_or(""),
        top_frame.unwrap_or("")
    );
    format!("sig:{}", sha256_hex(material.as_bytes()))
}

/// Namespace a caller-provided signature so it cannot impersonate a computed
/// one.
pub fn caller_signature(raw: &str) -> DomainResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(DomainError::Validation(
            "failure signature must not be empty".into(),
        ));
    }
    if trimmed.starts_with("caller:") || trimmed.starts_with("sig:") {
        return Ok(trimmed.to_string());
    }
    Ok(format!("caller:{trimmed}"))
}

/// Digest of the scenario contract. Editing the GWT invalidates every
/// fingerprint that was computed against the old wording.
pub fn contract_hash(given: &str, when: &str, then: &str) -> String {
    let material = format!("given\0{given}\0when\0{when}\0then\0{then}");
    sha256_hex(material.as_bytes())
}

/// Everything a verification run consumed. `entries` maps every input path
/// (repository-relative) to a content token (`blob:<oid>` or `deleted`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FingerprintMaterial<'a> {
    pub contract: &'a str,
    pub command: &'a [String],
    pub tool_versions: &'a BTreeMap<String, String>,
    pub input_globs: &'a [String],
    pub entries: &'a BTreeMap<String, String>,
}

pub fn compute_fingerprint(m: &FingerprintMaterial<'_>) -> String {
    let mut material = String::from("sdi-input-fingerprint-v1\n");
    material.push_str(&format!("contract:{}\n", m.contract));
    material.push_str(&format!(
        "command:{}\n",
        serde_json::to_string(m.command).unwrap_or_default()
    ));
    for (k, v) in m.tool_versions {
        material.push_str(&format!("tool:{k}={v}\n"));
    }
    let mut globs: Vec<&String> = m.input_globs.iter().collect();
    globs.sort();
    for g in globs {
        material.push_str(&format!("glob:{g}\n"));
    }
    for (path, token) in m.entries {
        material.push_str(&format!("entry:{path}\0{token}\n"));
    }
    format!("sha256:{}", sha256_hex(material.as_bytes()))
}

/// Parse `name=version` tool-version declarations into a sorted map.
pub fn parse_tool_versions(specs: &[String]) -> DomainResult<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for spec in specs {
        let (k, v) = spec.split_once('=').ok_or_else(|| {
            DomainError::Validation(format!("tool version must be name=version, got: {spec}"))
        })?;
        let (k, v) = (k.trim(), v.trim());
        if k.is_empty() || v.is_empty() {
            return Err(DomainError::Validation(format!(
                "tool version must be name=version, got: {spec}"
            )));
        }
        out.insert(k.to_string(), v.to_string());
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Evidence references
// ---------------------------------------------------------------------------

/// How a scenario verdict is evidenced. `Run` binds the verdict to a recorded
/// verification run and is validated strictly (existence, scenario link, exit
/// status, fingerprint freshness); `Free` is the legacy free-text reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceRef {
    Run(Id),
    Free(String),
}

impl EvidenceRef {
    pub fn parse(raw: &str) -> DomainResult<Self> {
        let trimmed = raw.trim();
        match trimmed.strip_prefix(RUN_REF_PREFIX) {
            Some(id) => {
                let id = id.trim();
                if id.is_empty() {
                    return Err(DomainError::EvidenceRunInvalid(
                        "run reference `run:` carries no run id".into(),
                    ));
                }
                Ok(EvidenceRef::Run(Id::from(id)))
            }
            None => Ok(EvidenceRef::Free(trimmed.to_string())),
        }
    }

    pub fn run_id(&self) -> Option<&Id> {
        match self {
            EvidenceRef::Run(id) => Some(id),
            EvidenceRef::Free(_) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Retry budget
// ---------------------------------------------------------------------------

/// Failure counter keyed by (plan, scenario, failure class, signature) —
/// deliberately independent of task id/title so cancelling, recreating or
/// renaming a task cannot reset it. Only a decision releases it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryBudget {
    pub plan_id: Id,
    pub scenario_id: Id,
    pub failure_class: FailureClass,
    pub failure_signature: String,
    /// Failures recorded since the last release.
    pub count: i64,
    /// Effective cap at read time (plan autonomy policy or the default).
    pub cap: i64,
    pub exhausted: bool,
    pub first_run_id: Option<Id>,
    pub last_run_id: Option<Id>,
    pub last_failed_at: Timestamp,
    pub released_by_decision_id: Option<Id>,
    pub released_at: Option<Timestamp>,
    /// Lifetime failures, never reset (audit).
    pub total_failures: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> Option<String> {
        None
    }

    fn outcome(code: i32) -> ExecOutcome {
        ExecOutcome {
            exit_code: Some(code),
            ..Default::default()
        }
    }

    #[test]
    fn exit_zero_is_not_a_failure() {
        let c = classify(&outcome(0), "ok", &none);
        assert_eq!(c.failure_class, FailureClass::None);
    }

    #[test]
    fn infrastructure_exits_never_count_as_product() {
        assert_eq!(
            classify(&outcome(127), "sh: jest: command not found", &none).failure_class,
            FailureClass::Environment
        );
        assert_eq!(
            classify(&outcome(126), "", &none).failure_class,
            FailureClass::Harness
        );
        assert_eq!(
            classify(&outcome(137), "", &none).failure_class,
            FailureClass::Environment
        );
        let timeout = ExecOutcome {
            timed_out: true,
            ..Default::default()
        };
        assert_eq!(
            classify(&timeout, "assertion failed", &none).failure_class,
            FailureClass::Harness
        );
        let sig = ExecOutcome {
            signal: Some(15),
            ..Default::default()
        };
        assert_eq!(
            classify(&sig, "", &none).failure_class,
            FailureClass::Harness
        );
        let killed = ExecOutcome {
            signal: Some(9),
            ..Default::default()
        };
        assert_eq!(classify(&killed, "", &none).error_class, "oom");
        let missing = ExecOutcome {
            spawn_error: Some(SpawnError::NotFound),
            ..Default::default()
        };
        assert_eq!(
            classify(&missing, "", &none).failure_class,
            FailureClass::Environment
        );
    }

    #[test]
    fn assertion_output_is_product() {
        let out = "running 2 tests\ntest api::creates_user ... FAILED\n\
                   thread 'api::creates_user' panicked at src/api.rs:41:5:\n\
                   assertion `left == right` failed\n";
        let in_repo = |p: &str| (p == "src/api.rs").then(|| p.to_string());
        let c = classify(&outcome(101), out, &in_repo);
        assert_eq!(c.failure_class, FailureClass::Product);
        assert_eq!(c.error_class, "assertion");
        assert_eq!(c.failing_test.as_deref(), Some("api::creates_user"));
        assert_eq!(c.top_frame.as_deref(), Some("src/api.rs"));
    }

    #[test]
    fn failing_test_is_order_independent() {
        let a = "--- FAIL: TestB (0.01s)\n--- FAIL: TestA (0.02s)\n";
        let b = "--- FAIL: TestA (0.03s)\n--- FAIL: TestB (0.00s)\n";
        assert_eq!(extract_failing_test(a), extract_failing_test(b));
        assert_eq!(extract_failing_test(a).as_deref(), Some("TestA"));
    }

    #[test]
    fn signature_is_stable_across_line_shifts_and_distinct_per_class() {
        let in_repo = |p: &str| p.starts_with("src/").then(|| p.to_string());
        let o1 = "test t ... FAILED\npanicked at src/a.rs:10:1\nassertion failed";
        let o2 = "test t ... FAILED\npanicked at src/a.rs:99:7\nassertion failed";
        let c1 = classify(&outcome(101), o1, &in_repo);
        let c2 = classify(&outcome(101), o2, &in_repo);
        let s = |c: &Classification| {
            failure_signature(
                "SCN-1",
                c.failure_class,
                c.failing_test.as_deref(),
                &c.error_class,
                c.top_frame.as_deref(),
            )
        };
        assert_eq!(s(&c1), s(&c2));
        let harness =
            failure_signature("SCN-1", FailureClass::Harness, Some("t"), "assertion", None);
        let product =
            failure_signature("SCN-1", FailureClass::Product, Some("t"), "assertion", None);
        assert_ne!(harness, product);
    }

    #[test]
    fn normalize_collapses_volatile_tokens() {
        assert_eq!(
            normalize_error_line("Error: timeout after 5000ms at /tmp/x/y.js"),
            normalize_error_line("Error: timeout after 7000ms at /var/z.js")
        );
    }

    #[test]
    fn fingerprint_changes_with_any_input() {
        let tools = BTreeMap::from([("node".to_string(), "20".to_string())]);
        let entries = BTreeMap::from([("a.rs".to_string(), "blob:1".to_string())]);
        let edited = BTreeMap::from([("a.rs".to_string(), "blob:2".to_string())]);
        let cmd = vec!["cargo".to_string(), "test".to_string()];
        let base = FingerprintMaterial {
            contract: "c",
            command: &cmd,
            tool_versions: &tools,
            input_globs: &[],
            entries: &entries,
        };
        let f1 = compute_fingerprint(&base);
        assert_eq!(f1, compute_fingerprint(&base));
        let changed = FingerprintMaterial {
            entries: &edited,
            ..base.clone()
        };
        assert_ne!(f1, compute_fingerprint(&changed));
        let other_contract = FingerprintMaterial {
            contract: "d",
            ..base.clone()
        };
        assert_ne!(f1, compute_fingerprint(&other_contract));
        let other_tools = BTreeMap::from([("node".to_string(), "22".to_string())]);
        let tool_bump = FingerprintMaterial {
            tool_versions: &other_tools,
            ..base
        };
        assert_ne!(f1, compute_fingerprint(&tool_bump));
    }

    #[test]
    fn evidence_ref_parses_run_prefix() {
        assert_eq!(
            EvidenceRef::parse("run:RUN-01").unwrap(),
            EvidenceRef::Run(Id::from("RUN-01"))
        );
        assert!(matches!(
            EvidenceRef::parse("src/lib.rs:42").unwrap(),
            EvidenceRef::Free(_)
        ));
        assert!(EvidenceRef::parse("run:").is_err());
    }

    #[test]
    fn caller_signature_is_namespaced() {
        assert_eq!(caller_signature("flaky-db").unwrap(), "caller:flaky-db");
        assert!(caller_signature("  ").is_err());
    }
}
