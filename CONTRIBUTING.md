# Contributing to sdi-plugin

## Dev setup

After cloning, point Git at the repo's hook directory **once**:

```sh
git config core.hooksPath .githooks
```

This enables the `cargo fmt --all -- --check` pre-commit hook so formatting violations are caught locally instead of in CI. If the hook fires, run `cargo fmt --all`, re-stage, and commit again. Non-Rust contributors without `cargo` on `PATH` are skipped automatically.
