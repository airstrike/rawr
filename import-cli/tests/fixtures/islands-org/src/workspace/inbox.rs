// rawr
//! The inbox, marked, as inboard's `workspace/inbox.rs` names who sent an
//! invite. `fn user(&self, id: &Id) -> Option<&User>` has elided
//! lifetimes, and its output borrows its receiver and not its id.

use model::Id;

use crate::Config;

/// An invite, as inboard's server notification of one.
pub struct Invite {
    pub invited_by: Id,
}

pub fn inviter(config: &Config, invited_by: Id) -> String {
    let inv = Invite { invited_by };
    let inviter = config
        .org
        .user(&inv.invited_by)
        .map(|u| u.name.as_str())
        .unwrap_or("Someone");
    format!("invited by {inviter}")
}
