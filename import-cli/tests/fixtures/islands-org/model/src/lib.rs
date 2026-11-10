//! inboard's `model` crate: an organization keeps its users by id and
//! looks one up by a borrowed id. The output borrows `self`.
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Id(pub u64);

#[derive(Debug, Clone)]
pub struct User {
    pub id: Id,
    pub name: String,
}

#[derive(Debug, Default)]
pub struct Organization {
    users: BTreeMap<Id, User>,
}

impl Organization {
    pub fn insert_user(&mut self, user: User) {
        self.users.insert(user.id, user);
    }

    pub fn user(&self, id: &Id) -> Option<&User> {
        self.users.get(id)
    }
}
