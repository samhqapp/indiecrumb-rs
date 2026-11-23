//! Live run against a local IndieCrumb stack. Skipped unless IC_E2E_URL is set:
//! IC_E2E_URL=http://127.0.0.1:18480 IC_KEY=ic_pub_... EDGE_SECRET=... cargo test --features feedback,usage,ureq --test e2e

use indiecrumb::feedback::{Feedback, Kind};
use indiecrumb::{Client, Config, Delivery, Response, Transport, Ureq};

/// The local stack wants the edge secret that Cloudflare adds in production.
struct WithEdgeSecret(Ureq, String);

impl Transport for WithEdgeSecret {
    fn post(&self, url: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<Response, String> {
        let mut h = headers.to_vec();
        h.push(("X-Edge-Secret", &self.1));
        self.0.post(url, &h, body)
    }
}

#[test]
fn ping_and_feedback_reach_the_server() {
    let Ok(url) = std::env::var("IC_E2E_URL") else { return eprintln!("IC_E2E_URL not set, skipped") };
    let dir = std::env::temp_dir().join(format!("indiecrumb-e2e-{}", std::process::id()));
    let c = Client::new(Config {
        endpoint: url,
        key: std::env::var("IC_KEY").unwrap(),
        version: "0.9.1".into(),
        build: "91".into(),
        dir: dir.clone(),
        enabled: true,
        transport: Box::new(WithEdgeSecret(Ureq::default(), std::env::var("EDGE_SECRET").unwrap())),
    })
    .unwrap();
    c.record_use();
    assert_eq!(c.ping_if_due(false), Some(Delivery::Sent));
    let png = include_bytes!("fixtures/screenshot.png").to_vec();
    let f = Feedback { kind: Kind::Bug, text: "e2e from indiecrumb-rs: the export button does nothing".into(), email: None, screenshot: Some(png), pro: false };
    assert_eq!(c.send_feedback(f), Ok(Delivery::Sent));
    std::fs::remove_dir_all(dir).unwrap();
}
