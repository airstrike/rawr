//! A visit of what a holder shows, as iced's `widget::Operation`: native
//! code lends a holder's `operate` the visit's object for the call.

/// What a visit of a holder collects, as iced's `widget::Operation<T = ()>`,
/// whose object native code lends a widget's `operate` for the call.
pub trait Visit<T = ()> {
    /// Notes `label` at the visit's depth.
    fn visit(&mut self, label: &str);

    /// Runs `each` one level deeper.
    fn traverse(&mut self, each: &mut dyn FnMut(&mut dyn Visit<T>));
}

/// The labels a visit noted, each after a `-` for each level it was in.
#[derive(Debug, Default)]
pub struct Collect {
    labels: Vec<String>,
    depth: usize,
}

impl Collect {
    /// The labels noted so far.
    #[must_use]
    pub fn labels(&self) -> Vec<String> {
        self.labels.clone()
    }
}

impl Visit for Collect {
    fn visit(&mut self, label: &str) {
        self.labels
            .push(format!("{}{label}", "-".repeat(self.depth)));
    }

    fn traverse(&mut self, each: &mut dyn FnMut(&mut dyn Visit)) {
        self.depth += 1;
        each(self);
        self.depth -= 1;
    }
}

/// The labels `item`'s `operate` notes, as iced's `UserInterface::operate`
/// lends the root widget it owns a `&mut dyn Operation`. The crate
/// compiles it.
pub fn visited<M>(mut item: impl crate::tinted::Diffed<M>) -> Vec<String> {
    let mut collect = Collect::default();
    item.operate(&mut collect);
    collect.labels
}
