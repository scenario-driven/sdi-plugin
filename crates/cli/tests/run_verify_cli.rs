//! `sdi run verify` / `sdi run status` / `sdi run budget` end to end through
//! the `sdi` binary against an auto-spawned daemon under a temp $SDI_HOME:
//! command execution, report capture, exit-status classification, input
//! fingerprint reuse/staleness, and run-ref task completion.

use std::path::{Path, PathBuf};
use std::process::Command;

fn sdi_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sdi"))
}

fn bin_dir() -> PathBuf {
    sdi_bin().parent().unwrap().to_path_buf()
}

struct Harness {
    home: PathBuf,
    repo: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let id = format!("{}-{}", std::process::id(), ulid::Ulid::new());
        let home = std::env::temp_dir().join(format!("sdi-cli-verify-{id}"));
        let repo = std::env::temp_dir().join(format!("sdi-cli-verify-repo-{id}"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["add", "."],
            vec![
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-qm",
                "i",
            ],
        ] {
            assert!(Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(&args)
                .status()
                .unwrap()
                .success());
        }
        let repo = repo.canonicalize().unwrap();
        Harness { home, repo }
    }

    fn path_env() -> String {
        // The daemon needs git and the verified commands need a shell.
        format!("{}:/usr/bin:/bin", bin_dir().display())
    }

    fn run_in(&self, cwd: &Path, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(sdi_bin())
            .args(args)
            .current_dir(cwd)
            .env("SDI_HOME", &self.home)
            .env("PATH", Self::path_env())
            .output()
            .expect("run sdi");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }

    fn json(&self, args: &[&str]) -> (i32, serde_json::Value, String) {
        let (code, stdout, stderr) = self.run_in(&self.repo, args);
        let v = serde_json::from_str(&stdout)
            .unwrap_or_else(|e| panic!("bad json ({e}) args={args:?}\n{stdout}\n{stderr}"));
        (code, v, stderr)
    }

    fn ok(&self, args: &[&str]) -> serde_json::Value {
        let (code, v, stderr) = self.json(args);
        assert_eq!(code, 0, "args={args:?} stdout={v} stderr={stderr}");
        v
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = Command::new(sdi_bin())
            .args(["daemon", "stop"])
            .env("SDI_HOME", &self.home)
            .env("PATH", Self::path_env())
            .output();
        let _ = std::fs::remove_dir_all(&self.repo);
    }
}

#[test]
fn verify_classifies_captures_and_evidences_runs() {
    let h = Harness::new();
    let suffix = ulid::Ulid::new().to_string();
    let repo = h.repo.to_string_lossy().into_owned();
    let project = h.ok(&[
        "project",
        "create",
        &format!("V{}", &suffix[..4]),
        "verify",
        "--slug",
        &format!("v-{}", suffix[..8].to_lowercase()),
        "--cwd",
        &repo,
    ]);
    let project_id = project["id"].as_str().unwrap().to_string();
    let plan = h.ok(&[
        "plan",
        "create",
        &project_id,
        &format!("P-{}", &suffix[..6]),
        "verify",
    ]);
    let plan_id = plan["id"].as_str().unwrap().to_string();
    let scn = h.ok(&[
        "scenario",
        "create",
        &plan_id,
        "SC-1",
        "--given",
        "a lib",
        "--when",
        "tests run",
        "--then",
        "they pass",
        "--confirmed",
    ]);
    let scn_id = scn["id"].as_str().unwrap().to_string();
    h.ok(&["plan", "approve", &plan_id]);
    let round = h.ok(&["round", "create", &plan_id, "R1"]);
    let round_id = round["id"].as_str().unwrap().to_string();
    let act = h.ok(&["round", "activate", &round_id]);
    let head = String::from_utf8(
        Command::new("git")
            .arg("-C")
            .arg(&h.repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    assert_eq!(
        act["baseline"]["commit"].as_str(),
        Some(head.trim()),
        "activation records the CLI's HEAD as the baseline commit"
    );
    let task = h.ok(&[
        "task",
        "create",
        &round_id,
        "T-1",
        "make it pass",
        "--scenario",
        &scn_id,
    ]);
    let task_id = task["id"].as_str().unwrap().to_string();
    h.ok(&["task", "start", &task_id]);

    // Product failure: nonzero exit with an assertion. Exit status propagates.
    let (code, v, stderr) = h.json(&[
        "run",
        "verify",
        "--scenario",
        "SC-1",
        "--input",
        "src/**",
        "--",
        "sh",
        "-c",
        "echo 'test lib::t ... FAILED'; echo 'assertion failed: x'; exit 3",
    ]);
    assert_eq!(code, 3, "stderr={stderr}");
    assert!(
        stderr.contains("assertion failed"),
        "output is echoed to stderr"
    );
    assert_eq!(v["failure_class"], "product");
    assert_eq!(v["failing_test"], "lib::t");
    assert_eq!(
        v["task_id"],
        task_id.as_str(),
        "auto-linked to the owning task"
    );
    let report = v["report_path"].as_str().unwrap();
    assert!(std::fs::read_to_string(report)
        .unwrap()
        .contains("assertion failed"));
    assert!(v["report_sha256"].as_str().unwrap().starts_with("sha256:"));

    // Environment: exit 127 and a missing binary.
    let (_, v, _) = h.json(&[
        "run",
        "verify",
        "--scenario",
        "SC-1",
        "--",
        "sh",
        "-c",
        "exit 127",
    ]);
    assert_eq!(v["failure_class"], "environment");
    let (code, v, _) = h.json(&[
        "run",
        "verify",
        "--scenario",
        "SC-1",
        "--",
        "definitely-not-a-binary-sdi",
    ]);
    assert_eq!(code, 1);
    assert_eq!(v["failure_class"], "environment");
    assert_eq!(v["spawn_error"], "not-found");

    // Harness: timeout.
    let (_, v, _) = h.json(&[
        "run",
        "verify",
        "--scenario",
        "SC-1",
        "--timeout",
        "1",
        "--",
        "sleep",
        "10",
    ]);
    assert_eq!(v["failure_class"], "harness");
    assert_eq!(v["timed_out"], true);

    // Budgets: product counted once, infrastructure counted separately.
    let b = h.ok(&["run", "budget", "--scenario", "SC-1"]);
    let rows = b["budgets"].as_array().unwrap();
    let count = |class: &str| -> i64 {
        rows.iter()
            .filter(|r| r["failure_class"] == class)
            .map(|r| r["count"].as_i64().unwrap())
            .sum()
    };
    assert_eq!(count("product"), 1);
    assert_eq!(count("environment"), 2);
    assert_eq!(count("harness"), 1);

    // Pass, reuse check fresh, evidence by run ref.
    let pass = h.ok(&[
        "run",
        "verify",
        "--scenario",
        "SC-1",
        "--input",
        "src/**",
        "--tool-version",
        "sh=posix",
        "--",
        "sh",
        "-c",
        "echo ok",
    ]);
    assert_eq!(pass["result"], "success");
    let run_id = pass["id"].as_str().unwrap().to_string();
    let st = h.ok(&["run", "status", "--scenario", "SC-1"]);
    assert_eq!(st["status"], "fresh");
    assert_eq!(st["run_id"], run_id.as_str());
    let cand = h.ok(&[
        "run",
        "status",
        "--scenario",
        "SC-1",
        "--input",
        "src/**",
        "--tool-version",
        "sh=posix",
        "--",
        "sh",
        "-c",
        "echo ok",
    ]);
    assert_eq!(cand["reuse_allowed"], true);

    // Stale after an input edit → completion refused, then accepted after revert.
    std::fs::write(h.repo.join("src/lib.rs"), "pub fn f() { 1; }\n").unwrap();
    let st = h.ok(&["run", "status", "--scenario", "SC-1"]);
    assert_eq!(st["status"], "stale");
    let evidence = format!("SC-1=passing@run:{run_id}");
    let (code, _, stderr) = h.run_in(
        &h.repo,
        &["task", "complete", &task_id, "--evidence", &evidence],
    );
    assert_ne!(code, 0);
    assert!(
        stderr.contains("EVIDENCE_STALE") || stderr.contains("stale"),
        "{stderr}"
    );
    std::fs::write(h.repo.join("src/lib.rs"), "pub fn f() {}\n").unwrap();
    let done = h.ok(&["task", "complete", &task_id, "--evidence", &evidence]);
    assert_eq!(done["status"], "done");
}
