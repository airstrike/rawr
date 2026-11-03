//! `Rc` and `Arc` of a crate's type in its signature (#775): rawr's
//! pointer stands for the crate's own, so a pointer the crate gives keeps
//! its count and identity, and one the program makes becomes the crate's
//! pointer the first time it crosses.

use std::rc::Rc;
use std::sync::{Arc, OnceLock};

#[derive(Debug)]
pub struct Config {
    name: String,
    level: i64,
}

impl Config {
    pub fn new(name: &str, level: i64) -> Self {
        Config {
            name: name.to_string(),
            level,
        }
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn level(&self) -> i64 {
        self.level
    }
}

static SHARED: OnceLock<Arc<Config>> = OnceLock::new();

/// A pointer the crate keeps one copy of itself.
pub fn shared() -> Arc<Config> {
    SHARED
        .get_or_init(|| Arc::new(Config::new("shared", 1)))
        .clone()
}

pub fn config(name: &str) -> Arc<Config> {
    Arc::new(Config::new(name, 2))
}

pub fn level(config: Arc<Config>) -> i64 {
    config.level
}

pub fn same(first: &Arc<Config>, second: &Arc<Config>) -> bool {
    Arc::ptr_eq(first, second)
}

pub fn keep(config: Arc<Config>) -> Arc<Config> {
    config
}

/// The count native code sees while it holds the pointer by value.
pub fn held(config: Arc<Config>) -> usize {
    Arc::strong_count(&config)
}

pub struct Node {
    label: String,
}

impl Node {
    pub fn label(&self) -> String {
        self.label.clone()
    }
}

pub fn node(label: &str) -> Rc<Node> {
    Rc::new(Node {
        label: label.to_string(),
    })
}

pub fn describe(node: &Rc<Node>) -> String {
    format!("{} node", node.label)
}

/// The count native code sees while it holds the pointer by value.
pub fn holders(node: Rc<Node>) -> usize {
    Rc::strong_count(&node)
}
