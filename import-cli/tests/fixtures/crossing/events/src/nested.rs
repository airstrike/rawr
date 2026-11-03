//! Functions in modules nested under a module the program imports (#851),
//! as iced's `operation::scrollable::snap_to` is under `widget::operation`,
//! and reached through a re-export of that module.

pub mod b {
    pub mod c {
        /// One more than `x`.
        pub fn f(x: u32) -> u32 {
            x + 1
        }

        /// A type in a nested module.
        pub struct Count(u32);

        impl Count {
            pub fn new(start: u32) -> Self {
                Count(start)
            }

            pub fn next(&mut self) -> u32 {
                self.0 += 1;
                self.0
            }
        }

        pub mod d {
            /// A name from a module three levels down.
            pub fn g() -> String {
                "deep".to_string()
            }
        }
    }
}

/// The nested modules under another name, as iced's `widget` re-exports
/// `operation` from another crate.
pub mod widget {
    pub use super::b as operation;
}
