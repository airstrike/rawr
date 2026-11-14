// rawr
// A `&self` method's opaque output keeps its receiver borrowed while it
// lives, as integration's `controls.view()` does, and the receiver is
// changed again once the output has dropped or moved into a call.
use natives::panels::{framed, Panel, Show};

fn main() {
    let mut panel = Panel::new("sky");
    {
        let shown = panel.view();
        println!("{}", shown.show());
    }
    panel.rename("sea");
    println!("{}", framed(panel.view()));
    panel.rename("land");
    println!("{}", panel.view().show());
}
