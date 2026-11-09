//! A crate that re-exports another crate's module and names its types by
//! aliases.
pub mod extension {
    pub mod registry {
        pub use shapes::check;

        /// An alias of a type another crate declares.
        pub type Update = check::Update;

        /// An alias of a std type of a type another crate declares.
        pub type Updates = Vec<check::Update>;

        /// An alias of a std type.
        pub type Names = Vec<String>;

        pub fn names() -> Names {
            vec!["mail".to_string(), "notes".to_string()]
        }
    }
}
