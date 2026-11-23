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
| `gpui` | `ui::ReportProblem` (the "Report a problem" form), `ui::ask_crash` (with `crash` and `feedback`) and `ui::sharing_row` (the Settings switch) | `gpui-kit` |

The default feature set is empty. Code of a feature you do not turn on is not compiled, so it is not
in your binary. Check: `samples/feedback-only` links only `feedback`; its release binary has no
`/ic/v1/ping` or `/ic/v1/crash` string and no `usage` or `crash` symbol.

```toml
indiecrumb = { git = "https://github.com/samhqapp/indiecrumb-rs", tag = "v0.3.1", features = ["feedback", "ureq"] }
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
ic.install_panic_hook();                // feature crash, early in main; also catches fatal signals and
                                        // ends with _exit, so the OS crash dialog never shows
// later, off the UI thread:
ic.ping_if_due(false);                  // feature usage: at launch and every hour
ic.send_feedback(Feedback { kind: Kind::Bug, text, email: None, screenshot: Some(png), pro: false })?;
ic.set_enabled(false);                  // the user turned it off: queue, crash files and counts deleted
```

Every send blocks. Call it from a background thread. The library sends nothing by itself: every
request starts from a call your app makes.

## UI (feature `gpui`)

For GPUI apps. Each part looks native: an Apple grouped form on macOS, Fluent on Windows, libadwaita
on Linux (picked with `cfg(target_os)`, Light or Dark from the window). Call `gpui_kit::init` first.

```rust
let form = cx.new(|cx| ReportProblem::new(settings.share_reports, window, cx)
    .on_send(|report, _, _| send_in_background(report))  // text, email, include_crash
    .on_close(|window, _| window.remove_window()));
// In Settings: "Share anonymous usage", with a "1" note whose tooltip lists what this build sends.
// `focus`: kept by the app, `cx.focus_handle().tab_stop(true)`. Tab reaches the row, Space flips it.
sharing_row(Theme::of(window), &focus, settings.share_reports, |on, _, _| ic.set_enabled(on))
// Ask mode, once at launch, like macOS: the platform alert "<App> quit unexpectedly." (Report…, Ignore),
// then the Problem Report window: a comment, the exact text that is sent, Don't Send and Send to
// Developer. Ignore or closing deletes the crash. The window never activates the app.
indiecrumb::ui::ask_crash(ic.clone(), "My App", cx);   // ic: Arc<Client>
```

`samples/gpui-render` draws every look in Light and Dark to PNG files (macOS).

## When anything is sent

| What | When |
|---|---|
| Usage ping | at most once per UTC day, when the app calls `ping_if_due` (at launch, or on its own timer). A failed ping keeps its counts for the next try and is never queued. |
| Feedback | only when the user submits it. Offline, it waits in the queue and goes with the next `flush`. |
| Crash report | never from the crashing process. The panic hook (or the handler for SIGSEGV, SIGBUS, SIGILL, SIGFPE, SIGTRAP and SIGABRT) writes one file; on the next launch the app asks the user (`ui::ask_crash`, or `pending_crashes`, `send_crash`, `discard_crash`) or sends all (`send_all_crashes`). |

Queue: one file per request in `<dir>/queue/`, at most 10 files, 1 MB and 7 days (oldest goes first).
Crash files: `<dir>/crashes/`, at most 10, deleted after 7 days.
Backoff: `429`, `5xx` and network errors wait `Retry-After` or 1 minute doubling to 6 hours. `410`
(key revoked) stops all sends for 7 days. Any other `4xx` deletes the item.

Off switch: `enabled: false` or `set_enabled(false)` sends nothing automatic (no ping, no Auto-mode
crash report) and deletes the queue, the crash files and the usage counts. Only `state.json` (the
install id and the backoff time) stays. What the user sends by hand (feedback, an Ask-mode crash report
they approved) still goes, without the install id. An item queued while off keeps no install id even
if sharing turns on before it is sent.

## Every field sent

Headers on every request: `Content-Type: application/json`, `X-IC-Key: <ingest key>`.
`X-IC-Install: <32 hex>`: a random id made on the device (not from hardware, user or account), renewed
every 90 days. It is sent with the ping, feedback and Auto-mode crash reports, never with an Ask-mode
crash report, and never while the switch is off. The server keeps it in memory only, for per-install limits, and never stores it.

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
| `channel` | `production` (default), `beta` or `internal`, set with `Client::set_channel(usage::Channel::Beta)`. The server stores anything else as `production`. |

The server stores only daily totals per app, version, OS, arch and channel.

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

At most 64 KB, 200 frames, strings at most 256 characters. The server rejects unknown fields.

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
