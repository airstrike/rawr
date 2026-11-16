// rawr
use events::tuned::{self, Open, Sheet, Silence, Tune, Tuner, Written};

/// Notes native code writes through its own `Open`, as a derived
/// serializer is: each call names its trait, and the sheet's type is the
/// one the impl of `open` gives.
struct Notes(Vec<String>);

impl Written for Notes {
    fn write_on<O: Open>(&self, open: O) -> Result<String, String> {
        let mut sheet = Open::open(open, "notes")?;
        for note in &self.0 {
            Sheet::write(&mut sheet, note)?;
        }
        Sheet::close(sheet)
    }
}

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
    let notes = |notes: &[&str]| Notes(notes.iter().map(|note| (*note).to_owned()).collect());
    println!(
        "{:?} {:?}",
        tuned::on_pad(&notes(&["do", "re"])),
        tuned::on_pad(&notes(&["do", ""]))
    );
}
