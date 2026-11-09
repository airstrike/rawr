//! Native: the colors a theme is made of, as inboard's `themedef`. Each
//! clone of a theme is counted, so a run under rawr shows it clones no
//! more than `cargo run` does.

use std::sync::atomic::{AtomicUsize, Ordering};

static CLONES: AtomicUsize = AtomicUsize::new(0);

/// How many times a theme was cloned.
pub fn clones() -> usize {
    CLONES.load(Ordering::Relaxed)
}

#[derive(Debug, PartialEq)]
pub struct Theme {
    pub name: String,
    pub colors: Colors,
}

impl Clone for Theme {
    fn clone(&self) -> Self {
        CLONES.fetch_add(1, Ordering::Relaxed);
        Self {
            name: self.name.clone(),
            colors: self.colors.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Colors {
    pub accent: u32,
    pub text: u32,
}

/// A theme's fonts, which can't be cloned, as a loaded font can't.
#[derive(Debug)]
pub struct Fonts {
    pub family: String,
    pub size: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hue {
    Warm,
    Cool,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            name: "light".to_owned(),
            colors: Colors {
                accent: 120,
                text: 7,
            },
        }
    }
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            name: "dark".to_owned(),
            colors: Colors {
                accent: 250,
                text: 9,
            },
        }
    }
}

impl Fonts {
    pub fn new(family: &str, size: u32) -> Self {
        Self {
            family: family.to_owned(),
            size,
        }
    }
}
