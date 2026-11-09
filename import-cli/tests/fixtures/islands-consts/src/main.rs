//! Native `main`, as inboard's: the marked `theme` module names the
//! constants of a native workspace crate's module and gives them to that
//! crate's functions.
mod theme;

fn main() {
    let font = theme::font();
    println!("font {font:?} {}", font.family());
    println!("code {:?}", theme::code());
    println!("bytes {}", theme::bytes());
    println!("weights {:?}", theme::weights());
    println!("family {}", theme::family());
    println!("{}", theme::version());
}
