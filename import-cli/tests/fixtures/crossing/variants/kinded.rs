// rawr
// A range of an integer other than `i64` crosses into a crate as a range
// of its own type: by value where a crate's type parameter takes it, so
// rustc fills the parameter with it, and as an iterator of its type where
// a crate takes an iterator. A crate's type parameter gives it back as the
// program's range of that type.

use events::paired::Paired;

fn main() {
    let low: isize = -1;
    let columns = low..=1;
    let pairs = (0u8..2).cartesian_product(columns).collect::<Vec<_>>();
    println!("{pairs:?}");

    let rows = low.saturating_sub(1)..=low.saturating_add(1);
    println!(
        "{} {}",
        events::paired::length(rows),
        events::paired::length(3u16..7)
    );

    let wide: Vec<u32> = events::paired::gathered(10u32..=12);
    let narrow: Vec<i16> = events::paired::gathered(-2i16..1);
    println!("{wide:?} {narrow:?}");

    let back = events::paired::same(2usize..=4);
    let total: usize = back.clone().sum();
    println!("{total} {back:?} {}", back.end());
    let half = events::paired::same(-3i8..0);
    println!("{half:?} {}", half.clone().count());
}
