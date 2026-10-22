//! Where the bot answers `?play`: the channels each server's mods chose with
//! `/play-channel`, unless `PLAY_CHANNELS` overrides them all.
//!
//! A server with no channels configured gets no answers. The sets persist
//! through a [`Store`]: memory, a JSON file (local runs), or an HTTP
//! endpoint (on Cloudflare, the `PlayBot` Durable Object's storage, reached
//! through the container's outbound interception).
//!
//! A thread follows its parent: it is answered when its parent channel is,
//! and `/play-channel` run in a thread changes its parent.
//!
//! The stored document maps guild IDs to channel IDs, all as strings
//! (snowflakes do not fit a JavaScript number): `{"123": ["456", "789"]}`.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;

/// Discord's `ADMINISTRATOR` permission bit.
pub const ADMINISTRATOR: u64 = 1 << 3;
/// Discord's `MANAGE_CHANNELS` permission bit.
pub const MANAGE_CHANNELS: u64 = 1 << 4;

/// Whether a member with these permissions (computed in the target channel)
/// may change where the bot answers.
#[must_use]
pub fn may_configure(permissions: u64) -> bool {
    permissions & (ADMINISTRATOR | MANAGE_CHANNELS) != 0
}

/// The channels the bot answers in.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Channels {
    guilds: BTreeMap<u64, BTreeSet<u64>>,
    /// `PLAY_CHANNELS`: when non-empty, the only channels answered.
    only: HashSet<u64>,
}

impl Channels {
    /// Channels from a stored document, with `only` (`PLAY_CHANNELS`)
    /// overriding them when non-empty.
    pub fn from_json(text: &str, only: HashSet<u64>) -> Result<Self, String> {
        let text = text.trim();
        let mut guilds = BTreeMap::new();
        if !text.is_empty() {
            let value: serde_json::Value =
                serde_json::from_str(text).map_err(|error| format!("channel config: {error}"))?;
            let map = value
                .as_object()
                .ok_or("channel config: expected an object of guild IDs")?;
            for (guild, channels) in map {
                let guild = id(guild)?;
                let channels = channels
                    .as_array()
                    .ok_or_else(|| format!("channel config: guild {guild}: expected a list"))?;
                let set: BTreeSet<u64> = channels
                    .iter()
                    .map(|channel| match channel {
                        serde_json::Value::String(text) => id(text),
                        other => Err(format!("channel config: not a channel ID: {other}")),
                    })
                    .collect::<Result<_, _>>()?;
                if !set.is_empty() {
                    guilds.insert(guild, set);
                }
            }
        }
        Ok(Self { guilds, only })
    }

    /// The stored document.
    #[must_use]
    pub fn to_json(&self) -> String {
        let map: serde_json::Map<String, serde_json::Value> = self
            .guilds
            .iter()
            .map(|(guild, channels)| {
                let list = channels.iter().map(|c| c.to_string().into()).collect();
                (guild.to_string(), serde_json::Value::Array(list))
            })
            .collect();
        serde_json::Value::Object(map).to_string()
    }

    /// Whether `PLAY_CHANNELS` overrides the per-server sets.
    #[must_use]
    pub fn overridden(&self) -> bool {
        !self.only.is_empty()
    }

    /// Whether to answer in `channel` (of `guild`; `None` for a DM) whose
    /// parent, when it is a thread, is `parent`: a thread is answered exactly
    /// when its parent is, under `PLAY_CHANNELS` too.
    #[must_use]
    pub fn listens_in(&self, guild: Option<u64>, channel: u64, parent: Option<u64>) -> bool {
        self.listens(guild, target(channel, parent))
    }

    /// Whether to answer in the (non-thread) `channel` of `guild`.
    #[must_use]
    pub fn listens(&self, guild: Option<u64>, channel: u64) -> bool {
        if self.overridden() {
            return self.only.contains(&channel);
        }
        guild.is_some_and(|guild| {
            self.guilds
                .get(&guild)
                .is_some_and(|set| set.contains(&channel))
        })
    }

    /// Answer in `channel`; false if it already was.
    pub fn add(&mut self, guild: u64, channel: u64) -> bool {
        self.guilds.entry(guild).or_default().insert(channel)
    }

    /// Stop answering in `channel`; false if it was not listed.
    pub fn remove(&mut self, guild: u64, channel: u64) -> bool {
        let Some(set) = self.guilds.get_mut(&guild) else {
            return false;
        };
        let removed = set.remove(&channel);
        if set.is_empty() {
            self.guilds.remove(&guild);
        }
        removed
    }

    /// How many channels each server chose.
    #[must_use]
    pub fn per_server(&self) -> BTreeMap<u64, usize> {
        self.guilds
            .iter()
            .map(|(guild, set)| (*guild, set.len()))
            .collect()
    }

    /// Channels chosen across every server.
    #[must_use]
    pub fn count(&self) -> usize {
        self.guilds.values().map(BTreeSet::len).sum()
    }

    /// A server's channels, in ID order.
    #[must_use]
    pub fn list(&self, guild: u64) -> Vec<u64> {
        self.guilds
            .get(&guild)
            .map(|set| set.iter().copied().collect())
            .unwrap_or_default()
    }
}

/// The channel a decision is about: a thread's parent, else the channel.
#[must_use]
pub fn target(channel: u64, parent: Option<u64>) -> u64 {
    parent.unwrap_or(channel)
}

/// Thread → parent (`None`: not a thread), remembered for the most recent
/// `capacity` channels.
#[derive(Debug)]
pub struct Parents {
    map: HashMap<u64, Option<u64>>,
    order: VecDeque<u64>,
    capacity: usize,
}

impl Parents {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            capacity: capacity.max(1),
        }
    }

    /// `Some(parent)` when known (`parent` is `None` for a non-thread).
    #[must_use]
    pub fn get(&self, channel: u64) -> Option<Option<u64>> {
        self.map.get(&channel).copied()
    }

    pub fn insert(&mut self, channel: u64, parent: Option<u64>) {
        if self.map.insert(channel, parent).is_none() {
            self.order.push_back(channel);
        }
        while self.order.len() > self.capacity {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            }
        }
    }
}

fn id(text: &str) -> Result<u64, String> {
    text.parse()
        .map_err(|_| format!("channel config: not an ID: {text:?}"))
}

/// Where [`Channels`] persist.
#[derive(Debug)]
pub enum Store {
    /// Nothing persists.
    Memory,
    /// A JSON file; a missing file is an empty config.
    File(PathBuf),
    /// `GET` loads and `PUT` saves the document at this `http://` URL.
    Http(reqwest::Client, String),
}

impl Store {
    /// `PLAY_CONFIG_URL`, else `PLAY_CONFIG_FILE`, else
    /// `~/.config/rawr-play/channels.json`, else memory.
    #[must_use]
    pub fn from_env() -> Self {
        if let Some(url) = std::env::var("PLAY_CONFIG_URL")
            .ok()
            .filter(|url| !url.is_empty())
        {
            return Self::Http(reqwest::Client::new(), url);
        }
        std::env::var_os("PLAY_CONFIG_FILE")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join(".config/rawr-play/channels.json"))
            })
            .map_or(Self::Memory, Self::File)
    }

    /// The stored document (empty when nothing was stored yet).
    pub async fn load(&self) -> Result<String, String> {
        match self {
            Self::Memory => Ok(String::new()),
            Self::File(path) => match std::fs::read_to_string(path) {
                Ok(text) => Ok(text),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
                Err(error) => Err(format!("{}: {error}", path.display())),
            },
            Self::Http(client, url) => {
                let response = client.get(url).send().await.map_err(|e| e.to_string())?;
                let status = response.status();
                if !status.is_success() {
                    return Err(format!("GET {url}: {status}"));
                }
                response.text().await.map_err(|e| e.to_string())
            }
        }
    }

    /// Replace the stored document.
    pub async fn save(&self, json: &str) -> Result<(), String> {
        match self {
            Self::Memory => Ok(()),
            Self::File(path) => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|error| format!("{}: {error}", parent.display()))?;
                }
                let temporary = path.with_extension("json.new");
                std::fs::write(&temporary, json)
                    .and_then(|()| std::fs::rename(&temporary, path))
                    .map_err(|error| format!("{}: {error}", path.display()))
            }
            Self::Http(client, url) => {
                let response = client
                    .put(url)
                    .header("content-type", "application/json")
                    .body(json.to_owned())
                    .send()
                    .await
                    .map_err(|e| e.to_string())?;
                let status = response.status();
                if status.is_success() {
                    Ok(())
                } else {
                    Err(format!("PUT {url}: {status}"))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none() -> HashSet<u64> {
        HashSet::new()
    }

    #[test]
    fn play_channel_default_is_nowhere() {
        let channels = Channels::from_json("", none()).unwrap();
        assert!(!channels.listens(Some(1), 10));
        assert!(!channels.listens(None, 10));
        assert!(channels.list(1).is_empty());
    }

    #[test]
    fn play_channel_add_remove_list() {
        let mut channels = Channels::default();
        assert!(channels.add(1, 20));
        assert!(channels.add(1, 10));
        assert!(!channels.add(1, 10));
        assert!(channels.add(2, 30));
        assert_eq!(channels.list(1), [10, 20]);
        assert!(channels.listens(Some(1), 10));
        // A channel answers only in the server it was added for.
        assert!(!channels.listens(Some(2), 10));
        assert!(!channels.listens(None, 10));
        assert!(channels.remove(1, 10));
        assert!(!channels.remove(1, 10));
        assert!(!channels.remove(9, 10));
        assert!(!channels.listens(Some(1), 10));
        assert!(channels.remove(1, 20));
        assert!(channels.list(1).is_empty());
        assert_eq!(channels.to_json(), r#"{"2":["30"]}"#);
        assert_eq!(channels.count(), 1);
    }

    #[test]
    fn play_channel_json_round_trip() {
        let big = u64::MAX - 1; // beyond a JavaScript number's exact range
        let mut channels = Channels::default();
        channels.add(big, big);
        channels.add(5, 6);
        let json = channels.to_json();
        assert_eq!(json, format!(r#"{{"{big}":["{big}"],"5":["6"]}}"#));
        assert_eq!(Channels::from_json(&json, none()).unwrap(), channels);
        assert!(Channels::from_json(r#"{"1":[2]}"#, none()).is_err());
        assert!(Channels::from_json(r#"{"x":["2"]}"#, none()).is_err());
        assert!(Channels::from_json("[]", none()).is_err());
        assert!(Channels::from_json("{", none()).is_err());
    }

    #[test]
    fn play_channel_env_override() {
        let only: HashSet<u64> = [10].into();
        let mut channels = Channels::from_json(r#"{"1":["20"]}"#, only).unwrap();
        assert!(channels.overridden());
        assert!(channels.listens(Some(1), 10));
        assert!(channels.listens(Some(7), 10));
        assert!(channels.listens(None, 10));
        assert!(!channels.listens(Some(1), 20));
        // Edits still persist; the override only decides where to answer.
        channels.add(1, 30);
        assert_eq!(channels.to_json(), r#"{"1":["20","30"]}"#);
        assert!(!channels.listens(Some(1), 30));
    }

    #[test]
    fn play_channel_thread_follows_parent() {
        let mut channels = Channels::default();
        channels.add(1, 10);
        // A thread of an enabled channel is answered.
        assert!(channels.listens_in(Some(1), 99, Some(10)));
        // A plain channel is decided by itself.
        assert!(channels.listens_in(Some(1), 10, None));
        // A thread of a channel that is not enabled is not, even if the
        // thread itself was once listed.
        channels.add(1, 98);
        assert!(!channels.listens_in(Some(1), 98, Some(20)));
        // /play-channel in a thread changes the parent.
        assert_eq!(target(99, Some(10)), 10);
        assert_eq!(target(10, None), 10);
        // PLAY_CHANNELS matches the parent likewise.
        let only: HashSet<u64> = [10].into();
        let channels = Channels::from_json("", only).unwrap();
        assert!(channels.listens_in(Some(7), 99, Some(10)));
        assert!(channels.listens_in(None, 10, None));
        assert!(!channels.listens_in(Some(7), 10, Some(20)));
    }

    #[test]
    fn play_channel_parents_are_bounded() {
        let mut parents = Parents::new(2);
        assert_eq!(parents.get(1), None);
        parents.insert(1, Some(10));
        parents.insert(2, None);
        assert_eq!(parents.get(1), Some(Some(10)));
        assert_eq!(parents.get(2), Some(None));
        parents.insert(2, None);
        parents.insert(3, Some(30));
        assert_eq!(parents.get(1), None);
        assert_eq!(parents.get(2), Some(None));
        assert_eq!(parents.get(3), Some(Some(30)));
    }

    #[test]
    fn play_channel_permission_check() {
        assert!(!may_configure(0));
        assert!(!may_configure(1 << 11 | 1 << 13)); // send messages, manage messages
        assert!(may_configure(MANAGE_CHANNELS));
        assert!(may_configure(ADMINISTRATOR));
        assert!(may_configure(MANAGE_CHANNELS | 1 << 11));
    }

    #[test]
    fn play_channel_file_store() {
        let dir = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tmp"))
            .join(format!("play-channel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::File(dir.join("rawr-play/channels.json"));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            assert_eq!(store.load().await.unwrap(), "");
            store.save(r#"{"1":["2"]}"#).await.unwrap();
            let channels = Channels::from_json(&store.load().await.unwrap(), none()).unwrap();
            assert!(channels.listens(Some(1), 2));
            assert_eq!(Store::Memory.load().await.unwrap(), "");
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
