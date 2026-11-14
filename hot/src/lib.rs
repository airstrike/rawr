//! Tells a program that `rawr run` runs when it takes a new generation of
//! its marked code, so a retained-mode framework can build its view again.
//! Under cargo, with no rawr, nothing is ever told and nothing runs.
//!
//! iced builds its view only after a message, so a save of code only `view`
//! calls shows at the next input. A subscription turns each generation into
//! a message, and the view that follows it runs the save:
//!
//! ```toml
//! [dependencies]
//! rawr-hot = { git = "https://github.com/airstrike/rawr" }
//! ```
//!
//! ```ignore
//! fn subscription(&self) -> Subscription<Message> {
//!     Subscription::run(rawr_hot::generations).map(|_| Message::Reloaded)
//! }
//! ```
//!
//! A framework that is not async takes a callback instead, with
//! [`on_generation`].
//!
//! Under `rawr run`, each generation is told on the thread that owns the
//! program, its main thread, right after the swap and before its windows
//! redraw. The numbers start at 1 and count the generations the program
//! took.

mod runtime;
mod stream;

pub use runtime::on_generation;
pub use stream::generations;
pub use stream::Generations;
