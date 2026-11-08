//! Native Rust: the root keeps a marked page as a trait object whose trait
//! needs `Debug`, which the marked type doesn't implement.
pub mod badge;
pub mod page;

fn main() {
    let page = badge::new();
    println!("{} {page:?}", page.title());
}
