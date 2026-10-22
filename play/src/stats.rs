//! What the bot reports about itself: boot phases and aggregate render
//! counts, never message content, user IDs or server names.
//!
//! The bot accumulates a [`Batch`] and sends it (with its [`Status`]) every
//! minute to `PLAY_REPORT_URL` (on Cloudflare, the `PlayBot` Durable Object
//! through the container's intercepted host), which keeps hourly totals and
//! serves the dashboard. Events (`booting`, `channels loaded`, `ready`,
//! errors) go there as they happen. Without `PLAY_REPORT_URL` they only go
//! to stderr.
//!
//! Per-server rows name a server only by its token ([`ServerKey`]); without
//! the key the report carries no per-server rows at all.

use std::collections::BTreeMap;

use crate::server::ServerKey;

/// Durations kept per batch; renders past this still count.
pub const MAX_DURATIONS: usize = 10_000;

/// How a render ended.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// An image was sent.
    Ok,
    /// The snippet did not check or failed while running.
    Code,
    /// A limit stopped it: time, operations, depth, size, memory, the
    /// process timeout, or a crash.
    Limit,
}

/// Classify a finished `rawr-play` process as [`message::reply`] reads it.
///
/// [`message::reply`]: crate::message::reply
#[must_use]
pub fn classify(success: bool, has_image: bool, stderr: &str) -> Outcome {
    if success && has_image {
        return Outcome::Ok;
    }
    let stderr = stderr.trim();
    if success
        || stderr.is_empty()
        || stderr.contains("memory allocation")
        || stderr.contains("limit exceeded")
        || stderr.contains("out of memory")
    {
        Outcome::Limit
    } else {
        Outcome::Code
    }
}

/// One server's counts since the last report.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ServerCounts {
    pub ok: u64,
    pub code: u64,
    pub limit: u64,
    pub rate_limited: u64,
    /// Unix seconds of the last render or rate limit.
    pub last: u64,
}

impl ServerCounts {
    fn add(&mut self, other: Self) {
        self.ok += other.ok;
        self.code += other.code;
        self.limit += other.limit;
        self.rate_limited += other.rate_limited;
        self.last = self.last.max(other.last);
    }
}

/// Counts since the last report.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Batch {
    pub ok: u64,
    pub code: u64,
    pub limit: u64,
    pub rate_limited: u64,
    /// Wall time of each render, in milliseconds.
    pub durations_ms: Vec<u32>,
    /// By guild ID; kept in memory only, reported by token.
    pub per_server: BTreeMap<u64, ServerCounts>,
    /// Playground renders (`/play`), counted apart from Discord's.
    pub web_ok: u64,
    pub web_code: u64,
    pub web_limit: u64,
}

impl Batch {
    /// A render that ended with `outcome` after `millis`, in `guild` (`None`
    /// for a DM).
    pub fn record(&mut self, outcome: Outcome, millis: u128, guild: Option<u64>) {
        if let Some(guild) = guild {
            let server = self.per_server.entry(guild).or_default();
            match outcome {
                Outcome::Ok => server.ok += 1,
                Outcome::Code => server.code += 1,
                Outcome::Limit => server.limit += 1,
            }
            server.last = now();
        }
        match outcome {
            Outcome::Ok => self.ok += 1,
            Outcome::Code => self.code += 1,
            Outcome::Limit => self.limit += 1,
        }
        if self.durations_ms.len() < MAX_DURATIONS {
            self.durations_ms
                .push(u32::try_from(millis).unwrap_or(u32::MAX));
        }
    }

    /// A playground render; its time joins the same histogram.
    pub fn record_web(&mut self, outcome: Outcome, millis: u128) {
        match outcome {
            Outcome::Ok => self.web_ok += 1,
            Outcome::Code => self.web_code += 1,
            Outcome::Limit => self.web_limit += 1,
        }
        if self.durations_ms.len() < MAX_DURATIONS {
            self.durations_ms
                .push(u32::try_from(millis).unwrap_or(u32::MAX));
        }
    }

    pub fn rate_limited(&mut self, guild: Option<u64>) {
        self.rate_limited += 1;
        if let Some(guild) = guild {
            let server = self.per_server.entry(guild).or_default();
            server.rate_limited += 1;
            server.last = now();
        }
    }

    /// The batch so far, leaving an empty one.
    pub fn take(&mut self) -> Self {
        std::mem::take(self)
    }

    /// Put back a batch that could not be sent.
    pub fn restore(&mut self, unsent: Self) {
        self.ok += unsent.ok;
        self.code += unsent.code;
        self.limit += unsent.limit;
        self.rate_limited += unsent.rate_limited;
        self.web_ok += unsent.web_ok;
        self.web_code += unsent.web_code;
        self.web_limit += unsent.web_limit;
        let room = MAX_DURATIONS.saturating_sub(self.durations_ms.len());
        self.durations_ms
            .extend(unsent.durations_ms.into_iter().take(room));
        for (guild, counts) in unsent.per_server {
            self.per_server.entry(guild).or_default().add(counts);
        }
    }

    /// Per-server rows by token: every server with activity in this batch
    /// or chosen channels (`channels`, by guild). None without a key.
    #[must_use]
    pub fn server_rows(
        &self,
        key: Option<&ServerKey>,
        channels: &BTreeMap<u64, usize>,
    ) -> Vec<serde_json::Value> {
        let Some(key) = key else {
            return Vec::new();
        };
        let mut guilds: Vec<u64> = self
            .per_server
            .keys()
            .chain(channels.keys())
            .copied()
            .collect();
        guilds.sort_unstable();
        guilds.dedup();
        guilds
            .into_iter()
            .map(|guild| {
                let counts = self.per_server.get(&guild).copied().unwrap_or_default();
                serde_json::json!({
                    "server": key.encrypt(guild),
                    "ok": counts.ok,
                    "code": counts.code,
                    "limit": counts.limit,
                    "rate_limited": counts.rate_limited,
                    "channels": channels.get(&guild).copied().unwrap_or(0),
                    "last": Some(counts.last).filter(|&last| last != 0),
                })
            })
            .collect()
    }

    /// The report body: this batch, its per-server rows, and the bot's status.
    #[must_use]
    pub fn to_json(&self, status: &Status, per_server: Vec<serde_json::Value>) -> String {
        serde_json::json!({
            "version": status.version,
            "boot": status.boot,
            "ready": status.ready,
            "servers": status.servers,
            "channels": status.channels,
            "channels_loaded": status.channels_loaded,
            "ok": self.ok,
            "code": self.code,
            "limit": self.limit,
            "rate_limited": self.rate_limited,
            "durations_ms": self.durations_ms,
            "per_server": per_server,
            "web_ok": self.web_ok,
            "web_code": self.web_code,
            "web_limit": self.web_limit,
        })
        .to_string()
    }
}

/// The bot's state as of a report.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// The image's git commit (`PLAY_VERSION`, baked in at build).
    pub version: String,
    /// Unix seconds at start.
    pub boot: u64,
    /// Unix seconds when the gateway was first ready.
    pub ready: Option<u64>,
    pub servers: u64,
    pub channels: u64,
    pub channels_loaded: bool,
}

/// Unix seconds now.
#[must_use]
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// Where reports go: `PLAY_REPORT_URL` (`/events` and `/stats` under it),
/// or nowhere but stderr.
#[derive(Debug)]
pub struct Report {
    target: Option<(reqwest::Client, String)>,
}

impl Report {
    #[must_use]
    pub fn from_env() -> Self {
        let url = std::env::var("PLAY_REPORT_URL")
            .ok()
            .filter(|url| !url.is_empty());
        Self {
            target: url.map(|url| (reqwest::Client::new(), url.trim_end_matches('/').to_owned())),
        }
    }

    /// A boot phase or error, to stderr and to the dashboard (best effort).
    pub async fn event(&self, phase: &str, detail: &str) {
        eprintln!(
            "rawr-play-bot: {phase}{}{detail}",
            if detail.is_empty() { "" } else { ": " }
        );
        let Some((client, base)) = &self.target else {
            return;
        };
        let body = serde_json::json!({ "phase": phase, "detail": detail }).to_string();
        let sent = client
            .post(format!("{base}/events"))
            .header("content-type", "application/json")
            .body(body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;
        if let Err(error) = sent.and_then(reqwest::Response::error_for_status) {
            eprintln!("rawr-play-bot: cannot report {phase}: {error}");
        }
    }

    /// Send a report body; false if it did not arrive.
    pub async fn stats(&self, body: String) -> bool {
        let Some((client, base)) = &self.target else {
            return true;
        };
        let sent = client
            .put(format!("{base}/stats"))
            .header("content-type", "application/json")
            .body(body)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;
        match sent.and_then(reqwest::Response::error_for_status) {
            Ok(_) => true,
            Err(error) => {
                eprintln!("rawr-play-bot: cannot report stats: {error}");
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_stats_classify() {
        assert_eq!(classify(true, true, ""), Outcome::Ok);
        assert_eq!(
            classify(false, false, "error[E0425]: cannot find value `x`"),
            Outcome::Code
        );
        assert_eq!(
            classify(false, false, "render time limit exceeded"),
            Outcome::Limit
        );
        assert_eq!(
            classify(
                false,
                false,
                "operation limit exceeded: more than 1000000 operations"
            ),
            Outcome::Limit
        );
        assert_eq!(
            classify(false, false, "memory allocation of 8 bytes failed"),
            Outcome::Limit
        );
        // Killed with nothing said, or a success without an image: a crash.
        assert_eq!(classify(false, false, ""), Outcome::Limit);
        assert_eq!(classify(true, false, ""), Outcome::Limit);
    }

    #[test]
    fn play_stats_batch_take_and_restore() {
        let mut batch = Batch::default();
        batch.record(Outcome::Ok, 120, Some(9));
        batch.record(Outcome::Code, 40, None);
        batch.record(Outcome::Limit, u128::MAX, Some(9));
        batch.rate_limited(Some(8));
        let sent = batch.take();
        assert_eq!(batch, Batch::default());
        assert_eq!(
            (sent.ok, sent.code, sent.limit, sent.rate_limited),
            (1, 1, 1, 1)
        );
        assert_eq!(sent.durations_ms, [120, 40, u32::MAX]);
        assert_eq!(sent.per_server[&9].ok, 1);
        assert_eq!(sent.per_server[&9].limit, 1);
        assert_eq!(sent.per_server[&8].rate_limited, 1);
        batch.record(Outcome::Ok, 7, Some(9));
        batch.restore(sent);
        assert_eq!(batch.per_server[&9].ok, 2);
        assert_eq!(
            (batch.ok, batch.code, batch.limit, batch.rate_limited),
            (2, 1, 1, 1)
        );
        assert_eq!(batch.durations_ms, [7, 120, 40, u32::MAX]);
    }

    #[test]
    fn play_stats_web_renders_count_apart() {
        let mut batch = Batch::default();
        batch.record_web(Outcome::Ok, 30);
        batch.record_web(Outcome::Code, 5);
        batch.record(Outcome::Ok, 20, None);
        assert_eq!((batch.ok, batch.web_ok, batch.web_code), (1, 1, 1));
        assert_eq!(batch.durations_ms, [30, 5, 20]);
        let sent = batch.take();
        batch.restore(sent);
        assert_eq!((batch.web_ok, batch.web_code, batch.web_limit), (1, 1, 0));
    }

    #[test]
    fn play_stats_durations_are_bounded() {
        let mut batch = Batch::default();
        for _ in 0..MAX_DURATIONS + 5 {
            batch.record(Outcome::Ok, 1, None);
        }
        assert_eq!(batch.ok, (MAX_DURATIONS + 5) as u64);
        assert_eq!(batch.durations_ms.len(), MAX_DURATIONS);
        let mut other = Batch::default();
        other.restore(batch.clone());
        other.restore(batch);
        assert_eq!(other.durations_ms.len(), MAX_DURATIONS);
    }

    #[test]
    fn play_stats_report_body() {
        let mut batch = Batch::default();
        batch.record(Outcome::Ok, 15, None);
        let status = Status {
            version: "abc123".into(),
            boot: 100,
            ready: Some(105),
            servers: 2,
            channels: 3,
            channels_loaded: true,
        };
        let body: serde_json::Value =
            serde_json::from_str(&batch.to_json(&status, Vec::new())).unwrap();
        assert_eq!(body["version"], "abc123");
        assert_eq!(body["ready"], 105);
        assert_eq!(body["servers"], 2);
        assert_eq!(body["ok"], 1);
        assert_eq!(body["durations_ms"], serde_json::json!([15]));
        let status = Status {
            ready: None,
            ..status
        };
        let body: serde_json::Value =
            serde_json::from_str(&batch.to_json(&status, Vec::new())).unwrap();
        assert!(body["ready"].is_null());
        assert_eq!(body["per_server"], serde_json::json!([]));
        assert_eq!(body["web_ok"], 0);
    }

    #[test]
    fn play_stats_server_rows_by_token() {
        let key = ServerKey::from_hex(&"ab".repeat(32)).unwrap();
        let mut batch = Batch::default();
        batch.record(Outcome::Ok, 10, Some(1_000_000_000_000_000_001));
        batch.record(Outcome::Ok, 10, None);
        let channels = BTreeMap::from([(1_000_000_000_000_000_001, 2), (7, 1)]);
        // Without the key: no per-server data at all.
        assert!(batch.server_rows(None, &channels).is_empty());
        let rows = batch.server_rows(Some(&key), &channels);
        assert_eq!(rows.len(), 2);
        let text = serde_json::Value::Array(rows.clone()).to_string();
        assert!(
            !text.contains("1000000000000000001"),
            "no plaintext ID: {text}"
        );
        let row = rows
            .iter()
            .find(|row| row["ok"] == 1)
            .expect("the active server");
        let token = row["server"].as_str().unwrap();
        assert_eq!(key.decrypt(token), Ok(1_000_000_000_000_000_001));
        assert_eq!(row["channels"], 2);
        assert!(row["last"].as_u64().is_some());
        let idle = rows.iter().find(|row| row["ok"] == 0).unwrap();
        assert_eq!(key.decrypt(idle["server"].as_str().unwrap()), Ok(7));
        assert!(idle["last"].is_null());
    }
}
