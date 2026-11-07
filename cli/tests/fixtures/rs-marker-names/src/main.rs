// rawr
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
    println!("{}", import::print::debug(41));
}
