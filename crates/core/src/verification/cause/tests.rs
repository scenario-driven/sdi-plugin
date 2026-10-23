use super::*;

fn cause(output: &str) -> FailureCauseIdentity {
    derive_failure_cause(
        "contract",
        FailureClass::Product,
        "assertion",
        output,
        &|_| None,
    )
}

#[test]
fn startup_causes_are_distinct_and_stable_across_runtime_noise() {
    let reports = [
        "thread 'first_wrapper' panicked at src/fixture.rs:12:3:\ncalled Result::unwrap() on an Err value: Unix socket path exceeds SUN_LEN: /tmp/runtime-a/socket",
        "thread 'second_wrapper' panicked at src/fixture.rs:99:7:\nartifact protocol/schema mismatch, expected 3 received 4",
        "thread 'third_wrapper' panicked at src/fixture.rs:44:2:\ndatabase /tmp/journal-a is already owned by another process",
    ];
    let identities = reports.map(cause);
    assert_ne!(identities[0].signature, identities[1].signature);
    assert_ne!(identities[1].signature, identities[2].signature);
    assert_ne!(identities[0].signature, identities[2].signature);
    for (report, identity) in reports.iter().zip(identities) {
        let changed = format!(
            "2026-01-01T01:02:03Z task renamed 123e4567-e89b-12d3-a456-426614174000 port 4040\n{}",
            report
                .replace("wrapper", "renamed_method")
                .replace(":12:3", ":88:9")
                .replace("/tmp/", "/var/")
        );
        assert_eq!(identity.signature, cause(&changed).signature);
        assert_eq!(identity.basis, "primary-diagnostic");
    }
}

#[test]
fn compiler_codes_and_semantic_quoted_types_are_retained() {
    let reports = [
        "src/widget.ts(17,3): error TS2540: Cannot assign to 'revision' because it is a read-only property.",
        "src/bridge.ts(28,6): error TS2322: Type 'LiveRecord' is not assignable to type 'StoredRecord'.",
        "src/view.ts(39,4): error TS2322: Type 'queued' is not assignable to type 'VisibleState'.",
    ];
    let identities = reports.map(cause);
    for a in 0..identities.len() {
        for b in a + 1..identities.len() {
            assert_ne!(identities[a].signature, identities[b].signature);
        }
    }
    let moved = reports[0].replace("src/widget.ts(17,3)", "/tmp/project/src/renamed.ts(999,88)");
    assert_eq!(identities[0].signature, cause(&moved).signature);
    assert_eq!(
        identities[0].signature,
        cause(&format!("\x1b[31m{}\x1b[0m", reports[0])).signature
    );
    assert_ne!(
        identities[0].signature,
        cause(&reports[0].replace("revision", "otherField")).signature
    );
}

#[test]
fn primary_diagnostic_ignores_secondary_churn_and_wrapper_names() {
    let primary = "error[E0308]: mismatched types: expected `String`, found `u64`";
    assert_eq!(
        cause(primary),
        cause(&format!(
            "{primary}\nerror[E0432]: unresolved import `adapter`"
        ))
    );
    assert_ne!(
        cause(primary).signature,
        cause("error[E0308]: mismatched types: expected `String`, found `bool`").signature
    );
    let first = "database already owned by another process\nerror TS2322: Type 'a' is not assignable to type 'b'.";
    assert_eq!(
        cause(first),
        cause("database already owned by another process")
    );
}

#[test]
fn unknown_reports_are_explicitly_coarse_without_raw_error_class_entropy() {
    let a = derive_failure_cause(
        "contract",
        FailureClass::Product,
        "error-a-volatile",
        "thread 'a' panicked at src/a.rs:1:2:\nassertion failed",
        &|_| None,
    );
    let b = derive_failure_cause(
        "contract",
        FailureClass::Product,
        "error-b-other",
        "thread 'b' panicked at src/b.rs:9:8:\nopaque failure",
        &|_| None,
    );
    assert_eq!(a, b);
    assert_eq!(a.basis, "coarse-fallback");
    assert_eq!(a.diagnostic, None);
    assert!(a.signature.starts_with("sig:v2:fallback:"));
    assert_ne!(
        a.signature,
        derive_failure_cause(
            "other-contract",
            FailureClass::Product,
            "unknown",
            "",
            &|_| None
        )
        .signature
    );
    assert_ne!(
        a.signature,
        derive_failure_cause("contract", FailureClass::Harness, "unknown", "", &|_| None).signature
    );
}

#[test]
fn rust_primary_multiline_type_context_distinguishes_causes() {
    let first = "error[E0308]: mismatched types\n --> src/adapter.rs:10:2\n | wrapper()\n = note: expected struct `String`\n            found enum `Option<String>`\nerror[E0432]: unresolved import `secondary`\n = note: expected other diagnostic";
    let moved = first
        .replace("src/adapter.rs:10:2", "/tmp/root/src/other.rs:99:7")
        .replace("wrapper()", "renamed_method()")
        .replace("`secondary`", "`changed`")
        .replace("other diagnostic", "changed diagnostic");
    assert_eq!(cause(first), cause(&moved));
    assert_ne!(
        cause(first).signature,
        cause(&first.replace("Option<String>", "Result<String, Error>")).signature
    );
    assert!(cause(first)
        .diagnostic
        .unwrap()
        .contains("found enum `Option<String>`"));
}

#[test]
fn startup_uses_inner_message_instead_of_test_stack_or_source_echo() {
    let report = "test restart_sun_len_case ... FAILED\n---- protocol_schema_mismatch stdout ----\nthread 'SUN_LEN_path_exceeds' panicked at src/fixture.rs:1:2:\n  1 | assert!(unix_socket_path_exceeds_sun_len());\n  2: module::protocol_schema_mismatch\ncalled Result::unwrap() on an Err value: database /tmp/db-a already owned by another process";
    assert_eq!(
        cause(report),
        cause("database /var/db-b already owned by another process")
    );
    assert_eq!(
        cause("test restart_sun_len_case ... FAILED").basis,
        "coarse-fallback"
    );
}

#[test]
fn compiler_markers_in_wrapper_names_or_source_echo_are_not_diagnostics() {
    let report = "test wrapper_error TS2540: dummy ... FAILED\nthread 'error TS2322: wrapper' panicked at src/fixture.rs:1:2:\n 1 | let text = \"error TS2540: dummy\";\nstartup failed: database /tmp/db-a already owned by another process";
    assert_eq!(
        cause(report),
        cause("database /var/db-b already owned by another process")
    );
    assert_eq!(
        cause(
            "test wrapper_error TS2540: dummy ... FAILED\n 1 | let text = \"error TS2540: dummy\";"
        )
        .basis,
        "coarse-fallback"
    );
}

#[test]
fn selected_compiler_diagnostic_normalizes_runtime_values_but_retains_literals() {
    let a = "src/client.ts(1,2): error TS2345: Argument 'State' at '/tmp/run-a/client' request 123e4567-e89b-12d3-a456-426614174000: task WORK-00000000000000000000000001 port 4040 2026-01-01T01:02:03Z localhost:4040 is not assignable to 'PublicState'.";
    let b = a
        .replace("/tmp/run-a/client", "/var/run-b/other")
        .replace(
            "123e4567-e89b-12d3-a456-426614174000",
            "223e4567-e89b-12d3-a456-426614174111",
        )
        .replace(
            "WORK-00000000000000000000000001",
            "OTHER-00000000000000000000000002",
        )
        .replace("4040", "9090")
        .replace("2026-01-01T01:02:03Z", "2027-02-02T02:03:04Z")
        .replace("localhost:9090", "127.0.0.1:9090");
    assert_eq!(cause(a), cause(&b));
    assert_ne!(
        cause("error TS2554: Expected 2 arguments, but got 1."),
        cause("error TS2554: Expected 3 arguments, but got 1.")
    );
    assert_ne!(
        cause("error TS2322: Type '1' is not assignable to type '2'."),
        cause("error TS2322: Type '1' is not assignable to type '3'.")
    );
}

#[test]
fn rust_span_only_types_are_semantic_without_source_or_wrapper_noise() {
    let first = "error[E0308]: mismatched types\n --> src/widget.rs:17:9\n  |\n17 | consume(true);\n  | ------- ^^^^ expected `u32`, found `bool`\n  | arguments to this function are incorrect\n";
    let moved = first
        .replace("src/widget.rs:17:9", "/tmp/root/src/view.rs:89:3")
        .replace("17 | consume(true);", "89 | renamed_wrapper(true);");
    assert_eq!(cause(first), cause(&moved));
    assert_ne!(
        cause(first),
        cause(&first.replace("found `bool`", "found `String`"))
    );
    assert_eq!(cause("error[E0308]: mismatched types\n 3 | let text = \"expected `u32`, found `bool`\";\nerror[E0432]: unresolved import `secondary`").basis, "coarse-fallback");
}
