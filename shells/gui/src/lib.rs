//! The debugger window `rawr gui` opens: an iced shell, in a process of
//! its own, on a run's debugger session. Its own process keeps it drawing
//! while the program it debugs is paused.
//!
//! The session's protocol ([`session`]) knows nothing of iced or of the
//! socket that carries it; the window ([`app`]) and its panels are iced
//! cells, drawn as [`theme`] says.

pub mod action;
pub mod app;
pub mod bindings;
pub mod console;
pub mod control;
pub mod icon;
pub mod session;
pub mod source;
pub mod theme;
pub mod tree;

pub use app::App;
