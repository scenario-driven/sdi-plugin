//! D1 smoke: drives the `sdi` binary through `daemon start → status → stop`
//! and `doctor` against a freshly-created `SDI_HOME` so no global state is
//! touched.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

fn sdi_bin() -> PathBuf {
    // CARGO_BIN_EXE_<bin_name> is set by cargo for integration tests.
    PathBuf::from(env!("CARGO_BIN_EXE_sdi"))
}

fn sdid_bin_dir() -> PathBuf {
    sdi_bin().parent().unwrap().to_path_buf()
}

fn fresh_home() -> PathBuf {
    let tmp = std::env::temp_dir().join(format!(
        "sdi-cli-test-{}-{}",
        std::process::id(),
        ulid::Ulid::new()
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    tmp
}

fn run(args: &[&str], home: &PathBuf) -> (i32, String, String) {
    let out = Command::new(sdi_bin())
        .args(args)
        .env("SDI_HOME", home)
        // sdid sibling resolution finds sdid by current_exe's parent dir.
        // That parent is `target/debug/`, where cargo just built `sdid` too.
        .env("PATH", sdid_bin_dir())
        .output()
        .expect("run sdi");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn doctor_reports_clean_under_fresh_home() {
    let home = fresh_home();
    let (code, stdout, _stderr) = run(&["doctor"], &home);
    assert_eq!(code, 0, "doctor exit: {code}, stdout: {stdout}");
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("doctor json");
    assert_eq!(v["paths"]["lm8"]["ok"], true);
    assert_eq!(v["db"]["ok"], true);
    assert_eq!(v["daemon"]["running"], false);
}

#[test]
fn daemon_start_status_stop_cycle() {
    let home = fresh_home();

    let (c1, s1, _) = run(&["daemon", "status"], &home);
    assert_eq!(c1, 0);
    let v: serde_json::Value = serde_json::from_str(&s1).unwrap();
    assert_eq!(v["running"], false);

    let (c2, s2, e2) = run(&["daemon", "start"], &home);
    assert_eq!(c2, 0, "start failed: {e2}");
    let v: serde_json::Value = serde_json::from_str(&s2).unwrap();
    assert_eq!(v["started"], true);
    let port = v["port"].as_u64().unwrap();
    assert!(port > 0);

    // Daemon is detached. Give the process a moment to finish writing pid.
    std::thread::sleep(Duration::from_millis(80));
    let (c3, s3, _) = run(&["daemon", "status"], &home);
    assert_eq!(c3, 0);
    let v: serde_json::Value = serde_json::from_str(&s3).unwrap();
    assert_eq!(v["running"], true);
    assert_eq!(v["port"].as_u64().unwrap(), port);

    let (c4, _, _) = run(&["daemon", "stop"], &home);
    assert_eq!(c4, 0);

    let (c5, s5, _) = run(&["daemon", "status"], &home);
    assert_eq!(c5, 0);
    let v: serde_json::Value = serde_json::from_str(&s5).unwrap();
    assert_eq!(v["running"], false);
}

/// Regression: `daemon stop` must actually terminate the process even when a
/// long-lived `/events` SSE stream is open. axum's graceful shutdown waits for
/// in-flight connections to finish; an open SSE stream never finishes, so an
/// unbounded wait closed the listener (port stops responding, `stop` reports
/// success) but left the process orphaned holding the DB. The daemon now bounds
/// the graceful shutdown and force-exits.
#[cfg(unix)]
#[test]
fn daemon_stop_does_not_orphan_on_open_sse() {
    use std::io::Write;

    let home = fresh_home();
    let (c, s, e) = run(&["daemon", "start"], &home);
    assert_eq!(c, 0, "start failed: {e}");
    let v: serde_json::Value = serde_json::from_str(&s).unwrap();
    let port = v["port"].as_u64().unwrap() as u16;
    std::thread::sleep(Duration::from_millis(150));

    let pid: i32 = std::fs::read_to_string(home.join(".cache/sdi/sdid.pid"))
        .unwrap()
        .trim()
        .parse()
        .unwrap();

    // Open a long-lived SSE connection (a dashboard tab) and keep it open.
    let mut sse = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    sse.write_all(
        b"GET /events HTTP/1.1\r\nHost: localhost\r\nAccept: text/event-stream\r\nConnection: keep-alive\r\n\r\n",
    )
    .unwrap();
    sse.flush().unwrap();
    std::thread::sleep(Duration::from_millis(250));

    let (cstop, _, _) = run(&["daemon", "stop"], &home);
    assert_eq!(cstop, 0);

    // The PROCESS must exit (bounded shutdown), not just stop serving the port.
    let mut alive = true;
    for _ in 0..80 {
        let dead = !Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .unwrap()
            .status
            .success();
        if dead {
            alive = false;
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(sse); // keep the SSE stream open across the stop
    assert!(
        !alive,
        "daemon (pid {pid}) orphaned after stop with an open SSE stream"
    );
}
