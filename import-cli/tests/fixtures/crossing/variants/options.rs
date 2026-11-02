// rawr
// A method that lends a slice it holds, as iced's `combo_box::State::
// options(&self) -> &[T]`: the slice is borrowed from the state for as
// long as the state's borrow lasts, as rustc lends it, so `.len()`,
// indexing, a `for` loop and `.iter().position(..)` read the options in
// place and no option is copied.

use events::choices::{clones, Choices, Family};

fn main() {
    let names = Choices::new(vec![
        String::from("Mono"),
        String::from("Serif"),
        String::from("Sans"),
    ]);
    println!(
        "{} names, first {}",
        names.options().len(),
        names.options()[0]
    );
    for name in names.options() {
        println!("name {name} {}", name.len());
    }
    let serif = names.options().iter().position(|name| name == "Serif");
    println!("serif at {serif:?}");

    let families = Choices::new(vec![Family::new("Quartz", 400), Family::new("Basalt", 700)]);
    // The vector moves its families into the call, as Rust moves them.
    println!("{} clones making the choices", clones());
    let granite = Family::new("Granite", 500);
    let more = Choices::new(vec![granite]);
    println!(
        "{} options, {} clones moving a local",
        more.options().len(),
        clones()
    );
    // What the borrows below clone: nothing.
    let made = clones();
    let options = families.options();
    println!(
        "{} families, last {}",
        options.len(),
        options[options.len() - 1]
    );
    for family in families.options() {
        println!("family {} {} {family}", family.name, family.weight);
    }
    let heavy = families
        .options()
        .iter()
        .position(|family| family.weight > 500);
    println!("heavy at {heavy:?}, {} clones", clones() - made);
}
