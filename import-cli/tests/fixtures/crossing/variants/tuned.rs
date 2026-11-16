// rawr
use events::tuned::{self, Silence, Tune, Tuner};

/// A tuner whose type it hides behind `Tuner<impl Tune>`.
fn built() -> Tuner<impl Tune> {
    tuned::tuner(Silence).notes(["do".to_owned(), "re".to_owned()])
}

/// The same tuner, hidden behind `impl Tune`.
fn hidden() -> impl Tune + 'static {
    built()
}

fn main() {
    let tune = hidden();
    println!("by value {:?}", tune.notes());
    let lent = &tune;
    println!("lent {} {}", lent.notes().len(), lent.name());
    let rebuilt = built().notes(["mi".to_owned()]);
    println!("rebuilt {:?} {}", Tune::notes(&rebuilt), rebuilt.name());
}
