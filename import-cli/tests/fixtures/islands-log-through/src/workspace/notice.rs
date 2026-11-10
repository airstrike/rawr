// rawr
//! Notices, marked, as inboard's `workspace/inbox.rs` builds its items: it
//! logs through its `log` crate's re-export of tracing's `warn!`, whose
//! level check compares tracing-core's `Level` with its `LevelFilter`, two
//! crates this package does not name.

use log::time::Timestamp;

/// A notification whose time is text, as inboard's server notifications.
pub struct Notification {
    pub created_at: String,
}

/// An event inside a closure, as inboard's inbox logs a time it cannot
/// read in `unwrap_or_else`'s closure and falls back to the current one.
pub fn fallback() -> String {
    let notification = Notification {
        created_at: "yesterday".to_owned(),
    };
    let timestamp = Timestamp::from_rfc3339(&notification.created_at).unwrap_or_else(|e| {
        log::warn!("bad created_at {:?}: {e}", notification.created_at);
        Timestamp::current()
    });
    format!("timestamp {}", timestamp.0)
}
