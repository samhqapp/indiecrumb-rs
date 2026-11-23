//! IndieCrumb client. The core (this file) holds the transport, the ingest key, the install id, the
//! on/off switch and the offline queue. Feedback, the usage ping and crash reports are cargo features,
//! so an app only compiles the parts it uses. Every send is blocking: call it off the UI thread.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(feature = "crash")]
pub mod crash;
#[cfg(feature = "feedback")]
pub mod feedback;
#[cfg(feature = "usage")]
pub mod usage;

/// The queue holds at most this many requests, this many bytes, for this many seconds.
const QUEUE_FILES: usize = 10;
const QUEUE_BYTES: u64 = 1 << 20;
const QUEUE_AGE: u64 = 7 * 86400;
const INSTALL_AGE: u64 = 90 * 86400;
const BACKOFF_MIN: u64 = 60;
const BACKOFF_MAX: u64 = 6 * 3600;
const REVOKED_PAUSE: u64 = 7 * 86400;

/// One HTTP reply.
pub struct Response {
    pub status: u16,
    /// `Retry-After` in seconds, when the server sent one.
    pub retry_after: Option<u64>,
}

/// Sends one `POST` with a JSON body. Return `Err` for a network failure (the request stays queued).
pub trait Transport: Send + Sync {
    fn post(&self, url: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<Response, String>;
}

/// The built-in transport (feature `ureq`), 20 second timeout.
#[cfg(feature = "ureq")]
pub struct Ureq(ureq::Agent);

#[cfg(feature = "ureq")]
impl Default for Ureq {
    fn default() -> Self {
        let cfg = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(20)))
            .http_status_as_error(false)
            .build();
        Ureq(cfg.into())
    }
}

#[cfg(feature = "ureq")]
impl Transport for Ureq {
    fn post(&self, url: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<Response, String> {
        let mut req = self.0.post(url).header("Content-Type", "application/json");
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let r = req.send(body).map_err(|e| e.to_string())?;
        let retry_after = r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|s| s.trim().parse().ok());
        Ok(Response { status: r.status().as_u16(), retry_after })
    }
}

pub struct Config {
    /// Base URL, such as `https://crumb.samhq.app`.
    pub endpoint: String,
    /// The public ingest key, `ic_pub_...`.
    pub key: String,
    /// The app version, digits and dots only, such as `1.4.2`.
    pub version: String,
    /// The build number, digits only (may be empty).
    pub build: String,
    /// A folder the SDK owns, such as `<app data>/indiecrumb`.
    pub dir: PathBuf,
    /// The app's Settings switch. Off: nothing is sent and every queued item is deleted.
    pub enabled: bool,
    pub transport: Box<dyn Transport>,
}

/// What happened to one item.
#[derive(Debug, PartialEq, Eq)]
pub enum Delivery {
    /// The server took it.
    Sent,
    /// It waits on disk and goes with the next `flush`.
    Queued,
    /// The server refused it for good (bad body, unknown key); it was deleted.
    Rejected(u16),
    /// The switch is off; nothing was stored or sent.
    Off,
}

#[derive(Serialize, Deserialize, Default)]
struct State {
    install: String,
    install_made: u64,
    retry_at: u64,
    backoff: u64,
}

pub struct Client {
    endpoint: String,
    key: String,
    #[allow(dead_code)] // read by the feature modules
    version: String,
    #[allow(dead_code)]
    build: String,
    dir: PathBuf,
    transport: Box<dyn Transport>,
    enabled: Arc<AtomicBool>,
    state: Mutex<State>,
    send_lock: Mutex<()>,
    #[cfg(feature = "usage")]
    usage: Mutex<usage::Counts>,
    #[cfg(feature = "usage")]
    channel: Mutex<usage::Channel>,
}

enum Outcome {
    Sent,
    Retry,
    Drop(u16),
}

impl Client {
    pub fn new(cfg: Config) -> io::Result<Client> {
        make_private_dir(&cfg.dir)?;
        let state: State = read_json(&cfg.dir.join("state.json")).unwrap_or_default();
        let c = Client {
            #[cfg(feature = "usage")]
            usage: Mutex::new(read_json(&cfg.dir.join("usage.json")).unwrap_or_default()),
            #[cfg(feature = "usage")]
            channel: Mutex::new(usage::Channel::Production),
            endpoint: cfg.endpoint.trim_end_matches('/').to_string(),
            key: cfg.key,
            version: cfg.version,
            build: cfg.build,
            dir: cfg.dir,
            transport: cfg.transport,
            enabled: Arc::new(AtomicBool::new(true)),
            state: Mutex::new(state),
            send_lock: Mutex::new(()),
        };
        c.set_enabled(cfg.enabled);
        Ok(c)
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Wire this to the Settings switch. Off deletes the queue, crash files and usage counts at once.
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::SeqCst);
        if !on {
            let _g = self.send_lock.lock().unwrap();
            if let Ok(rd) = fs::read_dir(&self.dir) {
                for e in rd.flatten() {
                    let p = e.path();
                    if e.file_name() == "state.json" {
                        continue;
                    }
                    let _ = if p.is_dir() { fs::remove_dir_all(&p) } else { fs::remove_file(&p) };
                }
            }
            #[cfg(feature = "usage")]
            {
                *self.usage.lock().unwrap() = Default::default();
            }
        }
    }

    /// The random 32-hex install id, made on this device and renewed every 90 days.
    pub fn install_id(&self) -> String {
        let mut s = self.state.lock().unwrap();
        let now = now();
        if s.install.len() != 32 || now.saturating_sub(s.install_made) > INSTALL_AGE {
            s.install = random_hex(16);
            s.install_made = now;
            self.save_state(&s);
        }
        s.install.clone()
    }

    /// Sends every queued item, oldest first, until one fails. Returns how many went.
    pub fn flush(&self) -> usize {
        self.flush_inner().0
    }

    /// Returns the sent count and the items the server refused for good.
    fn flush_inner(&self) -> (usize, Vec<(PathBuf, u16)>) {
        let _g = self.send_lock.lock().unwrap();
        let mut sent = 0;
        let mut dropped = vec![];
        for (path, _) in self.queue_files() {
            let Ok(raw) = fs::read(&path) else { continue };
            let Some(nl) = raw.iter().position(|&b| b == b'\n') else {
                let _ = fs::remove_file(&path);
                continue;
            };
            let head = String::from_utf8_lossy(&raw[..nl]).to_string();
            let (route, install) = head.split_once(' ').unwrap_or((&head, "0"));
            match self.post(route, &raw[nl + 1..], install == "1") {
                Outcome::Sent => {
                    let _ = fs::remove_file(&path);
                    sent += 1;
                }
                Outcome::Drop(code) => {
                    let _ = fs::remove_file(&path);
                    dropped.push((path, code));
                }
                Outcome::Retry => break,
            }
        }
        (sent, dropped)
    }

    /// Stores one request in the queue, then sends the queue.
    #[allow(dead_code)]
    pub(crate) fn enqueue_and_flush(&self, route: &str, body: &[u8], install: bool) -> Delivery {
        if !self.enabled() {
            return Delivery::Off;
        }
        let name = format!("{:010}-{}", now(), random_hex(4));
        let qdir = self.dir.join("queue");
        let file = qdir.join(&name);
        let mut data = format!("{route} {}\n", install as u8).into_bytes();
        data.extend_from_slice(body);
        if data.len() as u64 > QUEUE_BYTES || make_private_dir(&qdir).is_err() || write_private(&file, &data).is_err() {
            return Delivery::Rejected(413);
        }
        self.prune_queue();
        let (_, dropped) = self.flush_inner();
        if let Some((_, code)) = dropped.iter().find(|(p, _)| *p == file) {
            Delivery::Rejected(*code)
        } else if file.exists() {
            Delivery::Queued
        } else {
            Delivery::Sent
        }
    }

    /// Sends one request now, without the queue (the usage ping: a stale ping must not count for a later day).
    #[allow(dead_code)]
    pub(crate) fn send_now(&self, route: &str, body: &[u8], install: bool) -> Delivery {
        if !self.enabled() {
            return Delivery::Off;
        }
        let _g = self.send_lock.lock().unwrap();
        match self.post(route, body, install) {
            Outcome::Sent => Delivery::Sent,
            Outcome::Retry => Delivery::Queued,
            Outcome::Drop(c) => Delivery::Rejected(c),
        }
    }

    /// One POST with the backoff rules: 2xx sent; 410 key revoked, pause 7 days; 429, 5xx or no network,
    /// wait `Retry-After` or 1 minute doubling to 6 hours; any other status drops the item.
    fn post(&self, route: &str, body: &[u8], install: bool) -> Outcome {
        let now = now();
        if !self.enabled() || self.state.lock().unwrap().retry_at > now {
            return Outcome::Retry;
        }
        let id = if install { Some(self.install_id()) } else { None };
        let mut headers = vec![("X-IC-Key", self.key.as_str())];
        if let Some(id) = &id {
            headers.push(("X-IC-Install", id.as_str()));
        }
        let res = self.transport.post(&format!("{}{route}", self.endpoint), &headers, body);
        let mut s = self.state.lock().unwrap();
        let out = match res {
            Ok(r) if (200..300).contains(&r.status) => {
                s.backoff = 0;
                Outcome::Sent
            }
            Ok(r) if r.status == 410 => {
                s.retry_at = now + REVOKED_PAUSE;
                Outcome::Drop(410)
            }
            Ok(r) if r.status != 429 && r.status < 500 => Outcome::Drop(r.status),
            other => {
                s.backoff = (s.backoff * 2).clamp(BACKOFF_MIN, BACKOFF_MAX);
                let wait = other.ok().and_then(|r| r.retry_after).unwrap_or(0).max(s.backoff);
                s.retry_at = now + wait.min(REVOKED_PAUSE);
                Outcome::Retry
            }
        };
        self.save_state(&s);
        out
    }

    /// Oldest first.
    fn queue_files(&self) -> Vec<(PathBuf, u64)> {
        let mut v: Vec<(PathBuf, u64)> = fs::read_dir(self.dir.join("queue"))
            .map(|rd| rd.flatten().map(|e| (e.path(), e.metadata().map(|m| m.len()).unwrap_or(0))).collect())
            .unwrap_or_default();
        v.sort();
        v
    }

    /// Deletes items older than 7 days, then the oldest until at most 10 files and 1 MB are left.
    fn prune_queue(&self) {
        let now = now();
        let mut files = self.queue_files();
        files.retain(|(p, _)| {
            let made: u64 = p.file_name().and_then(|n| n.to_str()).and_then(|n| n.get(..10)).and_then(|n| n.parse().ok()).unwrap_or(0);
            let keep = now.saturating_sub(made) <= QUEUE_AGE;
            if !keep {
                let _ = fs::remove_file(p);
            }
            keep
        });
        let mut total: u64 = files.iter().map(|f| f.1).sum();
        let mut i = 0;
        while files.len() - i > QUEUE_FILES || total > QUEUE_BYTES {
            let _ = fs::remove_file(&files[i].0);
            total -= files[i].1;
            i += 1;
        }
    }

    fn save_state(&self, s: &State) {
        if let Ok(b) = serde_json::to_vec(s) {
            let _ = write_private(&self.dir.join("state.json"), &b);
        }
    }

    #[allow(dead_code)]
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn random_hex(bytes: usize) -> String {
    let mut b = vec![0u8; bytes];
    getrandom::fill(&mut b).expect("OS random source");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn read_json<T: for<'de> Deserialize<'de>>(p: &Path) -> Option<T> {
    serde_json::from_slice(&fs::read(p).ok()?).ok()
}

/// Writes through a temp file and a rename, owner-only on Unix.
fn write_private(p: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = p.with_extension("tmp");
    fs::write(&tmp, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    }
    fs::rename(&tmp, p)
}

fn make_private_dir(p: &Path) -> io::Result<()> {
    fs::create_dir_all(p)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(p, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// `macos`, `windows`, `linux`, `ios` or `android`, as the server names them.
#[allow(dead_code)]
pub(crate) fn os_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "android") {
        "android"
    } else {
        "linux"
    }
}

#[allow(dead_code)]
pub(crate) fn arch() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else {
        "x86_64"
    }
}

/// The OS major version on macOS (`15`); 0 elsewhere, which the server files under the bare OS name.
// shortcut: Windows 10 vs 11 and Linux send 0; read RtlGetVersion when a Windows app needs the split.
#[allow(dead_code)]
pub(crate) fn os_major() -> u32 {
    #[cfg(target_os = "macos")]
    {
        extern "C" {
            fn sysctlbyname(name: *const std::ffi::c_char, old: *mut u8, oldlen: *mut usize, new: *const u8, newlen: usize) -> i32;
        }
        let mut buf = [0u8; 32];
        let mut len = buf.len();
        // SAFETY: the name is a NUL-terminated literal and buf/len describe a valid buffer.
        let ok = unsafe { sysctlbyname(c"kern.osproductversion".as_ptr(), buf.as_mut_ptr(), &mut len, std::ptr::null(), 0) } == 0;
        if ok {
            let s = String::from_utf8_lossy(&buf[..len.min(buf.len())]);
            return s.trim_end_matches('\0').split('.').next().and_then(|m| m.parse().ok()).unwrap_or(0);
        }
    }
    0
}

/// Replaces the home folder and `/Users/<name>`-style parts with `~`, so no user name or home path
/// leaves the device, then cuts to `max` characters.
#[allow(dead_code)]
pub(crate) fn scrub(s: &str, max: usize) -> String {
    let mut out = s.to_string();
    for var in ["HOME", "USERPROFILE"] {
        if let Ok(h) = std::env::var(var) {
            if h.len() > 1 {
                out = out.replace(&h, "~");
            }
        }
    }
    for prefix in ["/Users/", "/home/", "\\Users\\"] {
        while let Some(i) = out.find(prefix) {
            let rest = &out[i + prefix.len()..];
            let end = rest.find(['/', '\\', ' ', '\n', '"', '\'']).unwrap_or(rest.len());
            out.replace_range(i..i + prefix.len() + end, "~");
        }
    }
    out.chars().take(max).collect()
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;

    /// (url, headers, body)
    pub type Sent = (String, Vec<(String, String)>, Vec<u8>);

    /// A transport that answers from a list and records every request.
    #[derive(Default, Clone)]
    pub struct Mock {
        pub replies: Arc<Mutex<Vec<Result<Response, String>>>>,
        pub sent: Arc<Mutex<Vec<Sent>>>,
    }

    impl Transport for Mock {
        fn post(&self, url: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<Response, String> {
            self.sent.lock().unwrap().push((url.into(), headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), body.to_vec()));
            let mut r = self.replies.lock().unwrap();
            if r.is_empty() {
                Ok(Response { status: 202, retry_after: None })
            } else {
                r.remove(0)
            }
        }
    }

    pub fn status(code: u16) -> Result<Response, String> {
        Ok(Response { status: code, retry_after: None })
    }

    pub fn client(dir: &Path, mock: &Mock) -> Client {
        Client::new(Config {
            endpoint: "https://x.test/".into(),
            key: "ic_pub_test".into(),
            version: "1.2.3".into(),
            build: "45".into(),
            dir: dir.to_path_buf(),
            enabled: true,
            transport: Box::new(mock.clone()),
        })
        .unwrap()
    }

    pub fn tempdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("indiecrumb-test-{name}-{}", random_hex(4)));
        fs::create_dir_all(&d).unwrap();
        d
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    #[test]
    fn install_id_is_32_hex_and_stable() {
        let d = tempdir("install");
        let c = client(&d, &Mock::default());
        let a = c.install_id();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()));
        assert_eq!(a, client(&d, &Mock::default()).install_id());
    }

    #[test]
    fn queue_keeps_item_on_failure_and_sends_later() {
        let d = tempdir("queue");
        let m = Mock::default();
        m.replies.lock().unwrap().push(Err("offline".into()));
        let c = client(&d, &m);
        assert_eq!(c.enqueue_and_flush("/ic/v1/x", b"{}", false), Delivery::Queued);
        assert_eq!(c.queue_files().len(), 1);
        // Backoff blocks an early retry.
        assert_eq!(c.flush(), 0);
        c.state.lock().unwrap().retry_at = 0;
        assert_eq!(c.flush(), 1);
        assert!(c.queue_files().is_empty());
        let sent = m.sent.lock().unwrap();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].0, "https://x.test/ic/v1/x");
        assert!(sent[1].1.iter().all(|(k, _)| k != "X-IC-Install"));
    }

    #[test]
    fn queue_caps_files() {
        let d = tempdir("caps");
        let m = Mock::default();
        let c = client(&d, &m);
        c.state.lock().unwrap().retry_at = u64::MAX; // hold everything
        for _ in 0..15 {
            c.enqueue_and_flush("/ic/v1/x", &[b'a'; 1000], false);
        }
        assert_eq!(c.queue_files().len(), QUEUE_FILES);
        for _ in 0..4 {
            c.enqueue_and_flush("/ic/v1/x", &vec![b'a'; 300 << 10], false);
        }
        assert!(c.queue_files().iter().map(|f| f.1).sum::<u64>() <= QUEUE_BYTES);
    }

    #[test]
    fn rejected_is_dropped_and_revoked_pauses() {
        let d = tempdir("drop");
        let m = Mock::default();
        m.replies.lock().unwrap().extend([status(400), status(410)]);
        let c = client(&d, &m);
        assert_eq!(c.send_now("/ic/v1/x", b"{}", true), Delivery::Rejected(400));
        assert_eq!(c.send_now("/ic/v1/x", b"{}", true), Delivery::Rejected(410));
        assert_eq!(c.send_now("/ic/v1/x", b"{}", true), Delivery::Queued); // paused, not sent
        assert_eq!(m.sent.lock().unwrap().len(), 2);
    }

    #[test]
    fn off_clears_and_sends_nothing() {
        let d = tempdir("off");
        let m = Mock::default();
        m.replies.lock().unwrap().push(Err("offline".into()));
        let c = client(&d, &m);
        c.enqueue_and_flush("/ic/v1/x", b"{}", false);
        c.set_enabled(false);
        assert!(c.queue_files().is_empty());
        assert_eq!(c.enqueue_and_flush("/ic/v1/x", b"{}", false), Delivery::Off);
        assert_eq!(c.send_now("/ic/v1/x", b"{}", false), Delivery::Off);
        assert_eq!(m.sent.lock().unwrap().len(), 1);
    }

    #[test]
    fn scrub_removes_user_paths() {
        assert_eq!(scrub("open /Users/alice/Library/x.txt failed", 256), "open ~/Library/x.txt failed");
        assert_eq!(scrub("C:\\Users\\bob\\AppData", 256), "C:~\\AppData");
        assert_eq!(scrub("/home/carol/.cargo/registry/x.rs", 256), "~/.cargo/registry/x.rs");
        assert_eq!(scrub("abcdef", 3), "abc");
    }
}
