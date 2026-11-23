use indiecrumb::feedback::{Feedback, Kind};
use indiecrumb::{Client, Config, Ureq};

fn main() {
    let c = Client::new(Config {
        endpoint: "https://crumb.samhq.app".into(),
        key: "ic_pub_sample".into(),
        version: "1.0".into(),
        build: "1".into(),
        dir: std::env::temp_dir().join("indiecrumb-sample"),
        enabled: std::env::args().any(|a| a == "--send"),
        transport: Box::new(Ureq::default()),
    })
    .unwrap();
    let f = Feedback { kind: Kind::Idea, text: "sample".into(), email: None, screenshot: None, pro: false };
    println!("{:?}", c.send_feedback(f));
}
