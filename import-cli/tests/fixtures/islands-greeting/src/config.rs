//! Native: the app's config and its pending edits, as inboard's `Config`
//! and `pending::State`, which the marked home screen is lent.

/// A user, which can be cloned, as inboard's `model::org::User`.
#[derive(Debug, Clone)]
pub struct User {
    name: String,
}

impl User {
    pub fn display_name(&self) -> &str {
        &self.name
    }
}

#[derive(Debug)]
pub struct Config {
    user: User,
}

impl Config {
    pub fn new(name: &str) -> Self {
        Self {
            user: User {
                name: name.to_owned(),
            },
        }
    }

    pub fn user(&self) -> &User {
        &self.user
    }
}

/// Edits waiting for review: lines added and removed per file.
#[derive(Debug, Default)]
pub struct Pending {
    pub files: Vec<(usize, usize)>,
}

impl Pending {
    pub fn has_pending(&self) -> bool {
        !self.files.is_empty()
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn diff_counts(&self) -> (usize, usize) {
        self.files
            .iter()
            .fold((0, 0), |(adds, removes), (add, remove)| {
                (adds + add, removes + remove)
            })
    }
}
