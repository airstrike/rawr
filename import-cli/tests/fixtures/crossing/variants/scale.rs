// rawr
use events::tuned::Tune;

/// The program's own source of notes, with inherent methods that share
/// the trait's names: a builder of another arity and a `name` of the
/// trait method's own shape.
struct Scale {
    notes: Vec<String>,
}

impl Scale {
    fn notes(self, more: &[&str]) -> Self {
        let mut notes = self.notes;
        notes.extend(more.iter().map(|note| (*note).to_owned()));
        Self { notes }
    }

    fn name(&self) -> String {
        "inherent".to_owned()
    }
}

impl Tune for Scale {
    fn notes(&self) -> &[String] {
        &self.notes
    }

    fn name(&self) -> String {
        format!("scale of {}", self.notes.len())
    }
}

/// The scale behind `impl Tune`: callers see only the trait's methods.
fn hidden() -> impl Tune {
    Scale { notes: Vec::new() }.notes(&["do", "re"])
}

fn main() {
    let tune = hidden();
    println!("hidden {:?} {}", tune.notes(), tune.name());
    let lent = &tune;
    println!("lent {} {}", lent.notes().len(), lent.name());
    let own = Scale { notes: Vec::new() }.notes(&["mi"]);
    println!("own {} {}", own.name(), Tune::name(&own));
}
