//! A crate that declares the type another crate re-exports.
pub mod check {
    /// An update one extension has.
    #[derive(Clone, Debug)]
    pub struct Update {
        pub name: String,
        pub version: u64,
    }
}

pub fn sample() -> Vec<check::Update> {
    vec![
        check::Update {
            name: "mail".to_string(),
            version: 2,
        },
        check::Update {
            name: "notes".to_string(),
            version: 7,
        },
    ]
}
