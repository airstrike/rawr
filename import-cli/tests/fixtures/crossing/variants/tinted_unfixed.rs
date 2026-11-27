// rawr
// A crate trait's method called through the trait at the program's `Note`
// with the trait argument written only in the qualified path, which rawr
// doesn't carry to the call yet: the native call's `M` stays open, and the
// check says so at the call, where the runner's build once failed.

use events::framed::Label;
use events::tinted::Diffed;

struct Note;

fn main() {
    let mut calm = Label("calm".into());
    println!("{}", <Label as Diffed<Note>>::label(&mut calm));
}
