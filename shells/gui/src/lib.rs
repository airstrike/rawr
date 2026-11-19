//! The debugger window `rawr gui` opens: an iced shell, in a process of
//! its own, on a run's debugger session. Its own process keeps it drawing
//! while the program it debugs is paused.
//!
//! The session's protocol ([`session`]) and the values it reads
//! ([`value`]) know nothing of iced or of the socket that carries them;
//! the window ([`app`]) and its panels are iced cells.

pub mod action;
pub mod app;
pub mod bindings;
pub mod console;
pub mod session;
pub mod source;
pub mod value;

pub use app::App;
