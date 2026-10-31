//! Types `events` names only through its aliases, as iced names
//! iced_core's `Element` and iced_graphics's `geometry::Cache`: `events`
//! depends on this crate and re-exports none of it.

use std::cell::RefCell;
use std::marker::PhantomData;

pub mod geometry {
    use super::{PhantomData, RefCell};

    /// What a renderer draws, as iced_graphics's `geometry::Renderer`.
    pub trait Renderer {
        /// What it draws into.
        type Geometry;

        /// `label`, drawn.
        fn draw(label: &str) -> Self::Geometry;
    }

    /// The labels a renderer drew, kept until cleared, as iced_graphics's
    /// `geometry::Cache`.
    pub struct Cache<R: Renderer> {
        drawn: RefCell<Vec<String>>,
        renderer: PhantomData<R>,
    }

    impl<R: Renderer> Cache<R> {
        /// An empty cache.
        pub fn new() -> Self {
            Cache {
                drawn: RefCell::new(Vec::new()),
                renderer: PhantomData,
            }
        }

        /// Draws `label` into the cache.
        pub fn draw(&self, label: &str) {
            let _ = R::draw(label);
            self.drawn.borrow_mut().push(label.to_owned());
        }

        /// Draws `label` into the cache and gives what the renderer drew,
        /// as iced_graphics's `Cache::draw` gives `Renderer::Geometry`.
        pub fn sketch(&self, label: &str) -> R::Geometry {
            self.drawn.borrow_mut().push(label.to_owned());
            R::draw(label)
        }

        /// What the cache holds, in order.
        pub fn drawn(&self) -> String {
            self.drawn.borrow().join(",")
        }

        /// Empties the cache.
        pub fn clear(&self) {
            self.drawn.borrow_mut().clear();
        }
    }

    impl<R: Renderer> Default for Cache<R> {
        fn default() -> Self {
            Self::new()
        }
    }
}

/// Text at a theme, as iced_core's `widget::Text<'a, Theme>`.
pub struct Note<'a, Theme> {
    text: String,
    look: PhantomData<&'a Theme>,
}

impl<Theme> Note<'_, Theme> {
    /// A note showing `text`.
    pub fn new(text: impl ToString) -> Self {
        Note {
            text: text.to_string(),
            look: PhantomData,
        }
    }

    /// What it shows.
    pub fn text(&self) -> String {
        self.text.clone()
    }
}
