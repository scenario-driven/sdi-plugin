//! `sdi project disable | enable | delete | update --description / --wiki-path`
//! end-to-end smoke: drives the real `sdi` binary against a daemon
//! auto-started under a temp $SDI_HOME.

use std::path::PathBuf;
use std::process::Command;

fn sdi_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sdi"))
}

fn bin_dir() -> PathBuf {
    sdi_bin().parent().unwrap().to_path_buf()
}

struct Harness {
    home: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "sdi-cli-projlife-{}-{}",
            std::process::id(),
            ulid::Ulid::new()
        ));
        std::fs::create_dir_all(&home).unwrap();
        Harness { home }
    }
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(sdi_bin())
            .args(args)
            .env("SDI_HOME", &self.home)
            .env("PATH", bin_dir())
            .output()
            .expect("run sdi");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
    fn run_ok(&self, args: &[&str]) -> serde_json::Value {
        let (code, stdout, stderr) = self.run(args);
        assert_eq!(code, 0, "args={args:?} stdout={stdout} stderr={stderr}");
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("bad json: {e}\n{stdout}"))
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = Command::new(sdi_bin())
            .args(["daemon", "stop"])
            .env("SDI_HOME", &self.home)
            .env("PATH", bin_dir())
            .output();
    }
}

fn make_project(h: &Harness, prefix: &str) -> String {
    let suffix = ulid::Ulid::new().to_string();
    let key = format!("{prefix}{}", &suffix[..4]);
    let slug = format!("{}-{}", prefix.to_lowercase(), suffix[..6].to_lowercase());
    let p = h.run_ok(&["project", "create", &key, "Lifecycle Test", "--slug", &slug]);
    p["id"].as_str().unwrap().to_string()
}

#[test]
fn project_update_handles_description_and_wiki_paths_patch_style() {
    let h = Harness::new();
    let id = make_project(&h, "UPD");

    // Set description.
    let v = h.run_ok(&["project", "update", &id, "--description", "first body"]);
    assert_eq!(v["description"], "first body");

    // Update with --name only; description preserved.
    let v = h.run_ok(&["project", "update", &id, "--name", "Renamed"]);
    assert_eq!(v["name"], "Renamed");
    assert_eq!(v["description"], "first body");

    // Replace wiki_paths via repeated --wiki-path.
    let v = h.run_ok(&[
        "project",
        "update",
        &id,
        "--wiki-path",
        "docs",
        "--wiki-path",
        "wiki",
    ]);
    let wp = v["wiki_paths"].as_array().unwrap();
    assert_eq!(wp.len(), 2);

    // Empty --description clears.
    let v = h.run_ok(&["project", "update", &id, "--description", ""]);
    assert!(
        v.get("description").is_none(),
        "expected description cleared"
    );

    // No flags at all is a user error (no patch).
    let (code, _stdout, stderr) = h.run(&["project", "update", &id]);
    assert_ne!(code, 0);
    assert!(stderr.contains("at least one"), "stderr: {stderr}");
}

#[test]
fn project_disable_enable_round_trip() {
    let h = Harness::new();
    let id = make_project(&h, "DIS");

    let v = h.run_ok(&["project", "disable", &id]);
    assert_eq!(v["enabled"], false);
    // Idempotent.
    let v = h.run_ok(&["project", "disable", &id]);
    assert_eq!(v["enabled"], false);
    let v = h.run_ok(&["project", "enable", &id]);
    assert_eq!(v["enabled"], true);
}

#[test]
fn project_delete_clean_project_succeeds() {
    let h = Harness::new();
    let id = make_project(&h, "DEL");
    let v = h.run_ok(&["project", "delete", &id]);
    assert_eq!(v["ok"], true);
    assert_eq!(v["forced"], false);
    // view should now fail.
    let (code, _stdout, _stderr) = h.run(&["project", "view", &id]);
    assert_ne!(code, 0);
}

#[test]
fn project_delete_force_flag_passes_through() {
    let h = Harness::new();
    let id = make_project(&h, "FRC");
    let v = h.run_ok(&["project", "delete", &id, "--force"]);
    assert_eq!(v["forced"], true);
}
