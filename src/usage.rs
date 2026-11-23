//! The daily usage ping (feature `usage`): one `POST /ic/v1/ping` per UTC day with the uses and
//! counters since the last ping. Call `ping_if_due` at launch and, for apps that stay open, every hour.

use crate::{Client, Delivery};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const ROUTE: &str = "/ic/v1/ping";
const MAX_COUNTERS: usize = 16;
const USES_MAX: u64 = 1023;

#[derive(Serialize, Deserialize, Default)]
pub(crate) struct Counts {
    /// UTC day number (days since 1970) and month number (year * 12 + month) of the last accepted ping.
    last_day: Option<u64>,
    last_month: Option<u64>,
    uses: u64,
    counters: BTreeMap<String, u64>,
}

/// The release channel the ping reports. The server keeps each channel's totals apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Channel {
    Production,
    Beta,
    Internal,
}

impl Channel {
    fn as_str(self) -> &'static str {
        match self {
            Channel::Production => "production",
            Channel::Beta => "beta",
            Channel::Internal => "internal",
        }
    }
}

impl Client {
    /// Sets the channel later pings report. Default `Production`.
    pub fn set_channel(&self, c: Channel) {
        *self.channel.lock().unwrap() = c;
    }

    /// Counts one use of the app's main action (one dictation, one cleanup).
    pub fn record_use(&self) {
        self.change_counts(|c| c.uses = c.uses.saturating_add(1));
    }

    /// Adds to a named counter. The project must define the name on the dashboard, else the server drops it.
    /// At most 16 names are kept.
    pub fn add_counter(&self, name: &str, n: u64) {
        self.change_counts(|c| {
            if n > 0 && (c.counters.contains_key(name) || c.counters.len() < MAX_COUNTERS) {
                let v = c.counters.entry(name.to_string()).or_default();
                *v = v.saturating_add(n);
            }
        });
    }

    /// Sends the ping when none went today (UTC). Returns `None` when it is not due. A failed ping keeps
    /// its counts for the next try; it is never queued, so a late ping can't count for the wrong day.
    pub fn ping_if_due(&self, pro: bool) -> Option<Delivery> {
        self.ping_at(crate::now(), pro)
    }

    fn ping_at(&self, now: u64, pro: bool) -> Option<Delivery> {
        if !self.enabled() {
            return Some(Delivery::Off);
        }
        let (day, month) = (now / 86400, month_number(now / 86400));
        let (body, counters) = {
            let c = self.usage.lock().unwrap();
            if c.last_day == Some(day) {
                return None;
            }
            let body = serde_json::json!({
                "version": self.version,
                "build": self.build,
                "os": crate::os_name(),
                "os_major": crate::os_major(),
                "arch": crate::arch(),
                "d": 1,
                "m": (c.last_month != Some(month)) as u8,
                "uses": c.uses.min(USES_MAX),
                "pro": pro,
                "counters": c.counters,
                "channel": self.channel.lock().unwrap().as_str(),
            });
            (serde_json::to_vec(&body).unwrap(), c.counters.clone())
        };
        let res = self.send_now(ROUTE, &body, true);
        if matches!(res, Delivery::Sent | Delivery::Rejected(_)) {
            self.change_counts(|c| {
                c.last_day = Some(day);
                c.last_month = Some(month);
                c.uses = 0;
                for (k, v) in &counters {
                    let left = c.counters.get(k).copied().unwrap_or(0).saturating_sub(*v);
                    if left == 0 {
                        c.counters.remove(k);
                    } else {
                        c.counters.insert(k.clone(), left);
                    }
                }
            });
        }
        Some(res)
    }

    fn change_counts(&self, f: impl FnOnce(&mut Counts)) {
        if !self.enabled() {
            return;
        }
        let mut c = self.usage.lock().unwrap();
        f(&mut c);
        if let Ok(b) = serde_json::to_vec(&*c) {
            let _ = crate::write_private(&self.dir.join("usage.json"), &b);
        }
    }
}

/// year * 12 + month (0-based) for a day number, from Howard Hinnant's civil_from_days.
fn month_number(days: u64) -> u64 {
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    (y * 12 + m - 1) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::*;

    #[test]
    fn month_number_matches_calendar() {
        assert_eq!(month_number(0), 1970 * 12); // 1970-01-01
        assert_eq!(month_number(20_415), 2025 * 12 + 10); // 2025-11-23
        assert_eq!(month_number(20_423), 2025 * 12 + 11); // 2025-12-01
    }

    #[test]
    fn one_ping_a_day_with_documented_body() {
        let m = Mock::default();
        let c = client(&tempdir("ping"), &m);
        for _ in 0..3 {
            c.record_use();
        }
        c.add_counter("cleanups", 2);
        let day = 20_415 * 86400 + 3600;
        assert_eq!(c.ping_at(day, true), Some(Delivery::Sent));
        assert_eq!(c.ping_at(day + 3600, true), None);
        let sent = m.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].0, "https://x.test/ic/v1/ping");
        let v: serde_json::Value = serde_json::from_slice(&sent[0].2).unwrap();
        let mut keys: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["arch", "build", "channel", "counters", "d", "m", "os", "os_major", "pro", "uses", "version"]);
        assert_eq!(v["channel"], "production");
        assert_eq!((v["d"].as_u64(), v["m"].as_u64(), v["uses"].as_u64()), (Some(1), Some(1), Some(3)));
        assert_eq!(v["counters"]["cleanups"], 2);
        drop(sent);
        // Next day: same month, counts reset.
        assert_eq!(c.ping_at(day + 86400, false), Some(Delivery::Sent));
        let v: serde_json::Value = serde_json::from_slice(&m.sent.lock().unwrap()[1].2).unwrap();
        assert_eq!((v["m"].as_u64(), v["uses"].as_u64()), (Some(0), Some(0)));
        c.set_channel(Channel::Beta);
        assert_eq!(c.ping_at(day + 2 * 86400, false), Some(Delivery::Sent));
        let v: serde_json::Value = serde_json::from_slice(&m.sent.lock().unwrap()[2].2).unwrap();
        assert_eq!(v["channel"], "beta");
    }

    #[test]
    fn failed_ping_keeps_counts() {
        let m = Mock::default();
        m.replies.lock().unwrap().push(Err("offline".into()));
        let c = client(&tempdir("pingfail"), &m);
        c.record_use();
        assert_eq!(c.ping_at(86400 * 20_000, false), Some(Delivery::Queued));
        assert_eq!(c.usage.lock().unwrap().uses, 1);
        assert_eq!(c.usage.lock().unwrap().last_day, None);
    }
}
