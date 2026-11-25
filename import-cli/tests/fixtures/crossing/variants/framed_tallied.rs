// rawr
// A crate module's generic code that calls another crate's generic code
// through its crate's re-export of that crate's module, as iced_widget's
// `Container::diff` calls iced_core's `Tree::diff_children` through
// `crate::core::widget::tree`.
use events::framed::tallied;

fn main() {
    println!("{}", tallied(&[3_u32, 5, 8]));
    println!("{}", tallied(&["calm", "deep"]));
}
