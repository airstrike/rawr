//! Native Rust whose modules sit under `cfg`s, as Cargo builds them for
//! this target (#1337). The test writes this host's `target_os` for `OS`
//! before it builds: `here` is the marked module Cargo compiles, and
//! `elsewhere` is one it never compiles, nor `moved.rs`, which `#[path]`
//! passes over.
#[cfg(not(target_os = "OS"))]
mod elsewhere;
#[cfg(target_os = "OS")]
mod here;
#[cfg_attr(unix, path = "moved_unix.rs")]
#[cfg_attr(not(unix), path = "moved_other.rs")]
mod moved;

fn main() {
    println!("{}", here::twice(21));
    println!("{}", moved::WHERE);
}
