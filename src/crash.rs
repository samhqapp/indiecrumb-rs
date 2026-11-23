//! Crash reports (feature `crash`). A panic hook writes one JSON file to `<dir>/crashes/`; nothing is
//! sent from the crashing process. On the next launch the app either asks the user (`pending_crashes`,
//! `send_crash`, `discard_crash`; no install id is sent) or sends them all (`send_all_crashes`).
//!
//! The server route `POST /ic/v1/crash` is not built yet; this body follows plan section 5 and 7.

use crate::{Client, Delivery};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::Ordering;

const ROUTE: &str = "/ic/v1/crash";
const SPOOL_FILES: usize = 10;
const SPOOL_AGE: u64 = 7 * 86400;
const MAX_FRAMES: usize = 200;
const STR_MAX: usize = 256;

/// One frame: the code offset inside its binary, which the server maps to a function with the
/// release's symbol file.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The binary's file name only, such as `hushpen` (empty when unknown).
    pub image: String,
    /// `0x`-hex offset from the binary's load address.
    pub offset: String,
}

/// Exactly the JSON body that is sent.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct CrashReport {
    #[serde(skip)]
    pub id: String,
    pub kind: String,
    pub message: String,
    pub file: String,
    pub line: u32,
    pub time: u64,
    pub version: String,
    pub build: String,
    pub os: String,
    pub os_major: u32,
    pub arch: String,
    pub frames: Vec<Frame>,
}

impl Client {
    /// Installs the panic hook. The previous hook still runs after it. Call once, early in `main`.
    pub fn install_panic_hook(&self) {
        let dir = self.dir.join("crashes");
        let enabled = self.enabled.clone();
        let (version, build) = (self.version.clone(), self.build.clone());
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if enabled.load(Ordering::SeqCst) {
                let message = info
                    .payload()
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| info.payload().downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                let (file, line) = info.location().map(|l| (l.file().to_string(), l.line())).unwrap_or_default();
                let r = report(&message, &file, line, &version, &build, frames());
                write_spool(&dir, &r);
            }
            prev(info);
        }));
    }

    /// Reports waiting on disk, oldest first. Files older than 7 days are deleted.
    pub fn pending_crashes(&self) -> Vec<CrashReport> {
        let dir = self.dir.join("crashes");
        let mut names: Vec<_> = fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect()).unwrap_or_default();
        names.sort();
        let now = crate::now();
        names
            .into_iter()
            .filter_map(|n| {
                let p = dir.join(&n);
                let r: Option<CrashReport> = crate::read_json(&p);
                match r {
                    Some(mut r) if now.saturating_sub(r.time) <= SPOOL_AGE => {
                        r.id = n;
                        Some(r)
                    }
                    _ => {
                        let _ = fs::remove_file(&p);
                        None
                    }
                }
            })
            .collect()
    }

    /// Ask mode: sends one report the user approved, without the install id.
    pub fn send_crash(&self, id: &str) -> Delivery {
        self.send_spooled(id, false)
    }

    /// Deletes one report the user declined.
    pub fn discard_crash(&self, id: &str) {
        if let Some(p) = self.spool_path(id) {
            let _ = fs::remove_file(p);
        }
    }

    /// Auto mode: sends every waiting report, with the install id. Call at launch, off the UI thread.
    pub fn send_all_crashes(&self) -> Vec<Delivery> {
        self.pending_crashes().iter().map(|r| self.send_spooled(&r.id, true)).collect()
    }

    fn send_spooled(&self, id: &str, install: bool) -> Delivery {
        let Some(p) = self.spool_path(id) else { return Delivery::Rejected(404) };
        let Ok(body) = fs::read(&p) else { return Delivery::Rejected(404) };
        let _ = fs::remove_file(&p); // the queue owns it now
        self.enqueue_and_flush(ROUTE, &body, install, !install) // Ask mode: the user approved it
    }

    fn spool_path(&self, id: &str) -> Option<PathBuf> {
        let ok = !id.is_empty() && !id.starts_with('.') && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.');
        ok.then(|| self.dir.join("crashes").join(id))
    }
}

fn report(message: &str, file: &str, line: u32, version: &str, build: &str, frames: Vec<Frame>) -> CrashReport {
    CrashReport {
        id: String::new(),
        kind: "panic".into(),
        message: crate::scrub(message, STR_MAX),
        file: short_path(file),
        line,
        time: crate::now(),
        version: version.into(),
        build: build.into(),
        os: crate::os_name().into(),
        os_major: crate::os_major(),
        arch: crate::arch().into(),
        frames,
    }
}

/// A source path without the home folder: an absolute path keeps its last three parts
/// (`ureq-3.4.2/src/lib.rs`), a workspace-relative one stays as it is.
fn short_path(file: &str) -> String {
    let s = crate::scrub(file, 4096);
    let absolute = s.starts_with('/') || s.starts_with('~') || s.get(1..2) == Some(":");
    if !absolute {
        return s.chars().take(STR_MAX).collect();
    }
    let parts: Vec<&str> = s.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    parts[parts.len().saturating_sub(3)..].join("/").chars().take(STR_MAX).collect()
}

fn write_spool(dir: &std::path::Path, r: &CrashReport) {
    if crate::make_private_dir(dir).is_err() {
        return;
    }
    if fs::read_dir(dir).map(|rd| rd.count()).unwrap_or(0) >= SPOOL_FILES {
        return; // keep the first crashes; a crash loop must not fill the disk
    }
    if let Ok(b) = serde_json::to_vec(r) {
        let _ = crate::write_private(&dir.join(format!("{:010}-{}.json", r.time, crate::random_hex(4))), &b);
    }
}

/// The current stack as (binary name, offset) pairs.
fn frames() -> Vec<Frame> {
    let mut out = vec![];
    backtrace::trace(|f| {
        out.push(frame(f.ip() as usize, f.module_base_address().map(|b| b as usize)));
        out.len() < MAX_FRAMES
    });
    out
}

#[cfg(unix)]
fn frame(ip: usize, _base: Option<usize>) -> Frame {
    #[repr(C)]
    struct DlInfo {
        fname: *const std::ffi::c_char,
        fbase: *const u8,
        sname: *const std::ffi::c_char,
        saddr: *const u8,
    }
    extern "C" {
        fn dladdr(addr: *const u8, info: *mut DlInfo) -> i32;
    }
    let mut info = DlInfo { fname: std::ptr::null(), fbase: std::ptr::null(), sname: std::ptr::null(), saddr: std::ptr::null() };
    // SAFETY: dladdr only reads the address and fills the struct; fname is a NUL-terminated C string or null.
    let found = unsafe { dladdr(ip as *const u8, &mut info) } != 0 && !info.fbase.is_null();
    if !found {
        return Frame { image: String::new(), offset: format!("{ip:#x}") };
    }
    let path = if info.fname.is_null() { String::new() } else { unsafe { std::ffi::CStr::from_ptr(info.fname) }.to_string_lossy().to_string() };
    let image = path.rsplit(['/', '\\']).next().unwrap_or("").chars().take(STR_MAX).collect();
    Frame { image, offset: format!("{:#x}", ip - info.fbase as usize) }
}

// shortcut: Windows frames carry no binary name; add GetModuleFileNameW with the Windows crash step.
#[cfg(not(unix))]
fn frame(ip: usize, base: Option<usize>) -> Frame {
    Frame { image: String::new(), offset: format!("{:#x}", base.map_or(ip, |b| ip - b)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    #[test]
    fn short_path_drops_home() {
        assert_eq!(short_path("crates/app/src/main.rs"), "crates/app/src/main.rs");
        assert_eq!(short_path("/Users/alice/.cargo/registry/src/idx/ureq-3.4.2/src/lib.rs"), "ureq-3.4.2/src/lib.rs");
        assert_eq!(short_path("C:\\Users\\bob\\x\\src\\a.rs"), "x/src/a.rs");
    }

    #[test]
    fn panic_is_spooled_then_sent_on_next_launch() {
        let d = tempdir("crash");
        let m = Mock::default();
        let c = client(&d, &m);
        c.install_panic_hook();
        let _ = std::panic::catch_unwind(|| panic!("open /Users/alice/secret.txt failed"));
        let _ = std::panic::take_hook(); // restore the default for other tests
        assert!(m.sent.lock().unwrap().is_empty(), "nothing is sent from the crashing process");

        let next = client(&d, &m);
        let p = next.pending_crashes();
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].message, "open ~/secret.txt failed");
        assert!(p[0].file.ends_with("crash.rs"));
        assert!(!p[0].frames.is_empty());
        assert!(p[0].frames.iter().all(|f| !f.image.contains('/')));

        assert_eq!(next.send_crash(&p[0].id), Delivery::Sent);
        assert!(next.pending_crashes().is_empty());
        let sent = m.sent.lock().unwrap();
        assert_eq!(sent[0].0, "https://x.test/ic/v1/crash");
        assert!(sent[0].1.iter().all(|(k, _)| k != "X-IC-Install"), "Ask mode sends no install id");
        let v: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["arch", "build", "file", "frames", "kind", "line", "message", "os", "os_major", "time", "version"]);
    }

    #[test]
    fn discard_and_bad_ids() {
        let d = tempdir("discard");
        let c = client(&d, &Mock::default());
        write_spool(&d.join("crashes"), &report("x", "a.rs", 1, "1", "1", vec![]));
        let id = c.pending_crashes()[0].id.clone();
        assert_eq!(c.send_crash("../state.json"), Delivery::Rejected(404));
        c.discard_crash(&id);
        assert!(c.pending_crashes().is_empty());
    }
}
