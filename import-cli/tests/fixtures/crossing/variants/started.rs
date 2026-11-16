// rawr
// A function of `tester` generic over a trait of `parts`, at a wrapper
// of `events` around an opaque type: its optional start is of the
// trait's associated type, which the wrapper's impl forwards to the
// opaque type's bound, an impl `tester` can't name.

use events::piped;

fn main() {
    let stepper = piped::stepped(4);
    println!("started {}", tester::started(&stepper, None));
}
