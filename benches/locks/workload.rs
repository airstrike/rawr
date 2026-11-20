//! One workload: a prepared program path a lock fix changes, run again for
//! each count and each timed sample.

/// What a workload is, for the result and the comparison.
pub struct About {
    /// Its name in a result and in `--only`.
    pub name: &'static str,
    /// The smells of the locks review it covers.
    pub smells: &'static [&'static str],
    /// What one run does, in a program's terms.
    pub run: &'static str,
}

/// What one run reported beyond its time: values the program measured
/// itself, such as how long a waiting thread took to get a lock.
#[derive(Default)]
pub struct Ran {
    reported: Vec<Reported>,
}

/// One named series of values a run measured, in `unit`.
pub struct Reported {
    pub name: String,
    pub unit: &'static str,
    pub values: Vec<f64>,
}

/// A prepared workload.
pub struct Workload {
    about: About,
    fixture: String,
    run: Box<dyn FnMut() -> Ran>,
}

impl Workload {
    /// `about`'s workload, whose program text (or what stands for it) is
    /// `fixture` and whose one run is `run`. Everything `run` needs is
    /// prepared before, so a run is only the path it measures.
    pub fn new(about: About, fixture: String, run: impl FnMut() -> Ran + 'static) -> Self {
        Self {
            about,
            fixture,
            run: Box::new(run),
        }
    }

    pub fn about(&self) -> &About {
        &self.about
    }

    /// The text whose hash identifies the workload's fixture.
    pub fn fixture(&self) -> &str {
        &self.fixture
    }

    /// Run it once.
    pub fn run(&mut self) -> Ran {
        (self.run)()
    }
}

impl Ran {
    /// Keep `values`, in `unit`, under `name`. Only the handoff reports,
    /// and only a counters build has it.
    #[cfg_attr(not(feature = "counters"), allow(dead_code))]
    pub fn report(&mut self, name: impl Into<String>, unit: &'static str, values: Vec<f64>) {
        self.reported.push(Reported {
            name: name.into(),
            unit,
            values,
        });
    }

    pub fn into_reported(self) -> Vec<Reported> {
        self.reported
    }
}
