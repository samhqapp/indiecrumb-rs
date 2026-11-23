# indiecrumb (Rust)

The IndieCrumb client for Rust apps: in-app feedback, a daily usage ping, and crash reports.
MIT licensed. Small on purpose, so you can read all of it before you ship it.

## Modules are cargo features

| Feature | What it adds | Extra dependency |
|---|---|---|
| (none) | the core: transport trait, ingest key, install id, on/off switch, offline queue | `serde`, `serde_json`, `getrandom` |
| `feedback` | `Client::send_feedback` | none |
| `usage` | `Client::record_use`, `add_counter`, `ping_if_due` | none |
| `crash` | `Client::install_panic_hook`, `pending_crashes`, `send_crash`, `discard_crash`, `send_all_crashes` | `backtrace` |
| `ureq` | `Ureq`, a ready `Transport` (rustls) | `ureq` |

The default feature set is empty. Code of a feature you do not turn on is not compiled, so it is not
in your binary. Check: `samples/feedback-only` links only `feedback`; its release binary has no
`/ic/v1/ping` or `/ic/v1/crash` string and no `usage` or `crash` symbol.

```toml
indiecrumb = { version = "0.1", features = ["feedback", "ureq"] }
```

Leave `ureq` off to pass your own HTTP client: implement `Transport::post` (one blocking POST).

## Use

```rust
let ic = indiecrumb::Client::new(indiecrumb::Config {
    endpoint: "https://crumb.samhq.app".into(),
    key: "ic_pub_...".into(),           // public, ships in the app, can only submit
    version: "1.4.2".into(),
    build: "142".into(),
    dir: data_dir.join("indiecrumb"),  // a folder the SDK owns
    enabled: settings.share_reports,    // the Settings switch
    transport: Box::new(indiecrumb::Ureq::default()),
})?;
ic.install_panic_hook();                // feature crash, early in main
// later, off the UI thread:
ic.ping_if_due(false);                  // feature usage: at launch and every hour
ic.send_feedback(Feedback { kind: Kind::Bug, text, email: None, screenshot: Some(png), pro: false })?;
ic.set_enabled(false);                  // the user turned it off: queue, crash files and counts deleted
```

Every send blocks. Call it from a background thread.

## When anything is sent

| What | When |
|---|---|
| Usage ping | at most once per UTC day, when the app calls `ping_if_due` (at launch, or on its own timer). A failed ping keeps its counts for the next try and is never queued. |
| Feedback | only when the user submits it. Offline, it waits in the queue and goes with the next `flush`. |
| Crash report | never from the crashing process. The panic hook writes one file; on the next launch the app asks the user (`pending_crashes`, `send_crash`, `discard_crash`) or sends all (`send_all_crashes`). |

Queue: one file per request in `<dir>/queue/`, at most 10 files, 1 MB and 7 days (oldest goes first).
Crash files: `<dir>/crashes/`, at most 10, deleted after 7 days.
Backoff: `429`, `5xx` and network errors wait `Retry-After` or 1 minute doubling to 6 hours. `410`
(key revoked) stops all sends for 7 days. Any other `4xx` deletes the item.

Off switch: `enabled: false` or `set_enabled(false)` sends nothing and deletes the queue, the crash
files and the usage counts. Only `state.json` (the install id and the backoff time) stays.

## Every field sent

Headers on every request: `Content-Type: application/json`, `X-IC-Key: <ingest key>`.
`X-IC-Install: <32 hex>`: a random id made on the device (not from hardware, user or account), renewed
every 90 days. It is sent with the ping, feedback and Auto-mode crash reports, never with an Ask-mode
crash report. The server keeps it in memory only, for per-install limits, and never stores it.

Nothing sends an IP address on purpose (the server sees the connection's address and keeps it in
memory only), a hostname, a user name, or a file path. Crash text has the home folder and
`/Users/<name>` parts replaced with `~`.

### `POST /ic/v1/ping` (feature `usage`), at most 2 KB

| Field | Value |
|---|---|
| `version` | the app version, digits and dots (`1.4.2`) |
| `build` | the build number, digits only; checked, not stored |
| `os` | `macos`, `ios`, `windows`, `linux` or `android` |
| `os_major` | the OS major version (`15`); `0` on Windows and Linux |
| `arch` | `arm64` or `x86_64` |
| `d` | `1`: the first ping of a UTC day (one ping a day) |
| `m` | `1` on the first ping of a UTC month, else `0` |
| `uses` | uses counted with `record_use` since the last ping, capped at 1023 |
| `pro` | the paid tier, `true` or `false` |
| `counters` | `{name: n}` from `add_counter` since the last ping, at most 16 names; the server drops names the project did not define |

The server stores only daily totals per app, version, OS and arch.

### `POST /ic/v1/feedback` (feature `feedback`)

| Field | Value |
|---|---|
| `kind` | `bug`, `idea` or `question` |
| `text` | what the user typed, 1 to 4,000 characters |
| `email` | only when the user typed one |
| `screenshot` | only when the user attached one: base64 PNG or JPEG, at most 300 KB and 2,000 x 2,000 pixels |
| `version`, `build` | as above |
| `os` | such as `macos 15` |
| `device` | the CPU arch, `arm64` or `x86_64` |
| `pro` | the paid tier |

### `POST /ic/v1/crash` (feature `crash`)

The server route is not built yet; this is the planned shape (at most 64 KB, 200 frames, strings
at most 256 characters).

| Field | Value |
|---|---|
| `kind` | `panic` |
| `message` | the panic message, home paths replaced with `~`, at most 256 characters |
| `file`, `line` | where it panicked; an absolute path keeps only its last three parts (`ureq-3.4.2/src/lib.rs`) |
| `time` | Unix seconds of the crash |
| `version`, `build`, `os`, `os_major`, `arch` | as in the ping |
| `frames` | up to 200 `{image, offset}`: the binary's file name only and the hex offset from its load address |

## Tests

```sh
cargo test --features feedback,usage,crash,ureq
IC_E2E_URL=http://127.0.0.1:18480 IC_KEY=ic_pub_... EDGE_SECRET=... cargo test --features feedback,usage,ureq --test e2e
```
