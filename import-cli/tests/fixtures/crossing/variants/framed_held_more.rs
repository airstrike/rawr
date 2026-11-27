// rawr
// A crate trait's provided methods on the program's types: `held(self)
// where Self: Sized`, as iced's `Widget::_boxed` that `column!` calls, by
// method and by path, and `is_void(&self)` of its supertrait, at a new
// program type and a new nesting.
use events::framed::{framed, Label, Meta, Shown};

/// A mood the program shows.
struct Mood(&'static str);

impl Meta for Mood {}

impl Shown for Mood {
    fn shown(&self) -> String {
        format!("~{}~", self.0)
    }
}

/// A cell that shows nothing.
struct Blank;

impl Meta for Blank {
    fn is_void(&self) -> bool {
        true
    }
}

impl Shown for Blank {
    fn shown(&self) -> String {
        String::new()
    }
}

/// A count the program shows.
struct Count(u8);

impl Meta for Count {}

impl Shown for Count {
    fn shown(&self) -> String {
        format!("{} items", self.0)
    }
}

fn main() {
    println!("{}", Label("calm".into()).held().shown());
    println!("{}", Mood("plain").held().shown());
    println!("{}", framed(Mood("deep")).held().shown());
    println!(
        "{}",
        Shown::held(framed(framed(Label("two".into()))).edge('!')).shown()
    );
    println!("{} {}", Mood("v").is_void(), Blank.is_void());
    println!("{}", framed(framed(Count(3)).edge('#')).held().shown());
    println!("{}", Count(4).is_void());
}
