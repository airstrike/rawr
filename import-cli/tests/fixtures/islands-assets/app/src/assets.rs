//! Files the package reads at compile time, where Cargo's build sees them.
pub const LOGO: &[u8] = include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/logo.txt"));
pub const ICON: &str = include_str!("../assets/icon.txt");
pub const SHARED: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../shared/note.txt"));

include!(concat!(env!("OUT_DIR"), "/icons.rs"));
