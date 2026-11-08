// rawr
#[cfg_attr(feature = "y", derive(Debug))]
#[allow(dead_code)]
struct Point {
    x: i64,
}

#[cfg(feature = "x")]
fn item() -> &'static str {
    "item under x"
}

#[cfg(not(feature = "x"))]
fn item() -> &'static str {
    "item without x"
}

fn main() {
    println!("x: {}", cfg!(feature = "x"));
    println!("y: {}", cfg!(feature = "y"));
    #[cfg(feature = "x")]
    println!("statement under x");
    #[cfg(not(feature = "x"))]
    println!("statement without x");
    println!("{}", item());
    #[cfg(feature = "y")]
    println!("{:?}", Point { x: 1 });
}
