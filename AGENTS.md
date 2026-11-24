# indiecrumb-rs

The IndieCrumb client for Rust apps: in-app feedback, a daily usage ping, and crash reports.
Crate `indiecrumb`, MIT, public repo. Small on purpose: a reader can read all of it.

```text
Cargo.toml     features: feedback, usage, crash, ureq (HTTP client), gpui (Report a problem UI)
src/           the crate; each module sits behind its cargo feature
tests/e2e.rs   end-to-end test against a fake server (needs feedback, usage, ureq)
tests/fixtures/
samples/       feedback-only (size check), gpui-render (draws the UI to PNG, macOS)
```

## Build and test

```sh
cargo build
cargo test --features feedback,usage,ureq
cargo build --features crash,feedback,usage,ureq
```

The `gpui` feature needs macOS to build the native alert. Run `samples/gpui-render` to see the UI.

## Rules

- The core is `serde`, `serde_json` and `getrandom` only. A new feature must not add a dependency to
  the core. Keep the default feature set empty.
- The library sends nothing by itself. Every request starts from a call the app makes. Sends block, so
  apps call them from a background thread.
- The on/off switch (`set_enabled(false)`) deletes the queue, crash files and counts.
- Limits: the offline queue holds 10 files, 1 MB and 7 days. Crash files: 10 files, 7 days.
- Releases are git tags (`vX.Y.Z`) that match the version in `Cargo.toml`. Apps depend on the tag.
- Public repo: no keys, no private paths, no private project names. The ingest key in examples is a
  placeholder.
