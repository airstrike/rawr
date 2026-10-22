use std::hash::BuildHasher;
mod scenario {
    include!("path_scenario.rs");
}
fn main() {
    for path in scenario::paths() {
        println!(
            "{:016x}",
            rustc_hash::FxBuildHasher.hash_one(path.as_path())
        );
    }
}
