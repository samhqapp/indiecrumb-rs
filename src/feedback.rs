//! In-app feedback (feature `feedback`). Sent only when the user submits it; queued on disk when offline.

use crate::{Client, Delivery};

const ROUTE: &str = "/ic/v1/feedback";
const TEXT_MAX: usize = 4000;
const IMAGE_MAX: usize = 300 << 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bug,
    Idea,
    Question,
}

pub struct Feedback {
    pub kind: Kind,
    /// 1 to 4,000 characters.
    pub text: String,
    /// Only when the user typed it, to get a reply.
    pub email: Option<String>,
    /// A PNG or JPEG of at most 300 KB and 2,000 x 2,000 pixels. Scale it down before you pass it.
    pub screenshot: Option<Vec<u8>>,
    /// The paid tier, so the dashboard can mark the item "Pro".
    pub pro: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum FeedbackError {
    EmptyText,
    TextTooLong,
    BadEmail,
    ScreenshotTooLarge,
    ScreenshotNotPngOrJpeg,
}

impl Client {
    /// Checks the item, stores it in the queue and sends the queue. Blocking.
    pub fn send_feedback(&self, f: Feedback) -> Result<Delivery, FeedbackError> {
        let body = self.feedback_body(&f)?;
        Ok(self.enqueue_and_flush(ROUTE, &body, true))
    }

    pub(crate) fn feedback_body(&self, f: &Feedback) -> Result<Vec<u8>, FeedbackError> {
        let text = f.text.trim();
        if text.is_empty() {
            return Err(FeedbackError::EmptyText);
        }
        if text.chars().count() > TEXT_MAX {
            return Err(FeedbackError::TextTooLong);
        }
        let email = f.email.as_deref().map(str::trim).filter(|e| !e.is_empty());
        if let Some(e) = email {
            let ok = e.len() <= 254 && e.split_once('@').is_some_and(|(a, b)| !a.is_empty() && b.contains('.') && !e.contains(char::is_whitespace));
            if !ok {
                return Err(FeedbackError::BadEmail);
            }
        }
        if let Some(img) = &f.screenshot {
            if img.len() > IMAGE_MAX {
                return Err(FeedbackError::ScreenshotTooLarge);
            }
            if !img.starts_with(b"\x89PNG\r\n\x1a\n") && !img.starts_with(b"\xff\xd8\xff") {
                return Err(FeedbackError::ScreenshotNotPngOrJpeg);
            }
        }
        let kind = match f.kind {
            Kind::Bug => "bug",
            Kind::Idea => "idea",
            Kind::Question => "question",
        };
        let mut body = serde_json::json!({
            "kind": kind,
            "text": text,
            "version": self.version,
            "build": self.build,
            "os": match crate::os_major() {
                0 => crate::os_name().to_string(),
                m => format!("{} {m}", crate::os_name()),
            },
            "device": crate::arch(),
            "pro": f.pro,
        });
        if let Some(e) = email {
            body["email"] = e.into();
        }
        if let Some(img) = &f.screenshot {
            body["screenshot"] = base64(img).into();
        }
        Ok(serde_json::to_vec(&body).unwrap())
    }
}

/// Standard base64 with padding, which Go's `[]byte` JSON field reads.
fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { T[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    fn fb(text: &str) -> Feedback {
        Feedback { kind: Kind::Bug, text: text.into(), email: None, screenshot: None, pro: false }
    }

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn checks_fields() {
        let c = client(&tempdir("fbcheck"), &Mock::default());
        assert_eq!(c.send_feedback(fb("  ")), Err(FeedbackError::EmptyText));
        assert_eq!(c.send_feedback(fb(&"a".repeat(4001))), Err(FeedbackError::TextTooLong));
        let mut f = fb("hi");
        f.email = Some("nope".into());
        assert_eq!(c.send_feedback(f), Err(FeedbackError::BadEmail));
        let mut f = fb("hi");
        f.screenshot = Some(b"GIF89a".to_vec());
        assert_eq!(c.send_feedback(f), Err(FeedbackError::ScreenshotNotPngOrJpeg));
    }

    #[test]
    fn sends_the_documented_body() {
        let m = Mock::default();
        let c = client(&tempdir("fbsend"), &m);
        let mut f = fb(" Crash on save ");
        f.email = Some("a@b.co".into());
        f.screenshot = Some(b"\x89PNG\r\n\x1a\nxx".to_vec());
        assert_eq!(c.send_feedback(f), Ok(Delivery::Sent));
        let sent = m.sent.lock().unwrap();
        assert_eq!(sent[0].0, "https://x.test/ic/v1/feedback");
        assert!(sent[0].1.iter().any(|(k, v)| k == "X-IC-Install" && v.len() == 32));
        let v: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["build", "device", "email", "kind", "os", "pro", "screenshot", "text", "version"]);
        assert_eq!(v["text"], "Crash on save");
        assert_eq!(v["screenshot"], "iVBORw0KGgp4eA==");
    }
}
