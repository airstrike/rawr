// rawr
// Literals passed as `impl Into<T>` take the type rustc infers from `T`'s
// `From` impls. iced's `.id(SCROLLABLE)` and `operation::focus("new-task")`
// pass a literal or a `&'static str` const as `impl Into<Tag>`, which
// converts from `&'static str` alone, so the string lives for the program
// (#726). iced's `window_size((500.0, 800.0))` passes a float pair as
// `impl Into<Size>`, which `impl<T> From<(T, T)> for Size<T>` makes
// `(f32, f32)` (#1941).

use events::controls::{area, caption, tag_length, Caption};

const LIST: &str = "list";

fn main() {
    let shown: Caption<'_> = caption("tasks").tagged(LIST);
    let owned: Caption<'_> = caption("owned").tagged(String::from("kept"));
    println!("{} | {}", shown.shown(), owned.shown());
    println!("{} {}", tag_length("new-task"), tag_length(LIST));
    println!("{} {}", area((2.5, 4.0)), area((3, 5)));
}
