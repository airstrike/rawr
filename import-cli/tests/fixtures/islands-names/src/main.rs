//! Native Rust: `main` calls marked modules named as words the legacy
//! lexer reads as keywords, through `crate::`.
mod import;
mod switch;

fn main() {
    let mut lever = switch::Lever { on: false };
    lever.flip();
    println!(
        "{} {} {} {}",
        crate::switch::f(),
        import::g(),
        crate::import::print::h(),
        lever.on
    );
}
