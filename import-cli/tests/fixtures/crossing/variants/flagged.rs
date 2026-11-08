// Native code of the program's own whose `cfg!` reads the package's
// features. The tests copy it to `src/`, and only `featured.rs` declares
// it.
#![allow(dead_code)]

pub fn x() -> bool {
    cfg!(feature = "x")
}
