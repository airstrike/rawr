//! Standalone observation harness for unmodified rustc-hash 2.1.2.
use std::hash::BuildHasher;

fn main() {
    for length in 0..=1024 {
        let key: String = (0..length).map(|i| char::from((i % 127) as u8)).collect();
        println!("{:016x}", rustc_hash::FxBuildHasher.hash_one(key.as_str()));
    }
    for key in ["path-bases", "credential-provider", "é", "ß", "🦀", "a\0b", "日本語", "e\u{301}"] {
        println!("{:016x}", rustc_hash::FxBuildHasher.hash_one(key));
    }
}
