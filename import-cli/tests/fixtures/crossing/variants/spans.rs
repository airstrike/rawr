// rawr
// A crate generic's parameter that appears only inside an argument's bound
// is the one the bound's impl for the argument's type gives, as rustc
// infers it: `L` of `rich(impl AsRef<[Span<'a, L>]>)` given a
// `Vec<Span<'static, ()>>` is `()`, from std's `impl<T> AsRef<[T]> for
// Vec<T>`, and likewise for an array, a slice, `Borrow`, `IntoIterator`
// and a crate's own trait, for a function and a method, and a function a
// `use` names called with a turbofish.

use events::spans::{borrowed, first_link, gathered, rich, span, Numbers, Span};

fn main() {
    let spans: Vec<Span<'static, ()>> = vec![span("a"), span("b")];
    println!("{}", rich(spans).size(13).shown());

    let array: [Span<'static, u8>; 2] = [span("c"), span("d")];
    println!("{}", rich(array.clone()).shown());
    println!("{}", rich(&array[..]).shown());
    println!("{}", borrowed(array.clone()).shown());
    println!("{}", borrowed(vec![span::<()>("e")]).shown());

    let listed: Vec<Span<'static, bool>> = vec![span("f")];
    println!("{}", gathered(listed).shown());
    println!("{}", gathered(array).extended([span("g")]).shown());

    let shown = rich(vec![span::<()>("h")]);
    let links: Vec<u8> = vec![1, 2];
    println!("{}", shown.beside(links));
    println!("{}", shown.beside(["x", "y"]));

    println!("{}", first_link(Numbers(vec![4, 5])));
}
