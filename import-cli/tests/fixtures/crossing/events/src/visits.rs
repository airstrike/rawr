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

/// Where a holder's `publish` notes what it shows, as iced's `Shell<'a,
/// Message>`: it borrows the notes native code owns, so native code lends
/// it for the call.
#[derive(Debug)]
pub struct Sink<'a> {
    notes: &'a mut Vec<String>,
}

impl Sink<'_> {
    /// Notes `label`.
    pub fn note(&mut self, label: &str) {
        self.notes.push(label.to_owned());
    }
}

/// A label the crate's own concrete code visits, as iced_core's `Text`,
/// whose `Widget` impl the crate compiles: its `operate` takes the visit
/// a program's impl was lent and passes on.
#[derive(Debug, Clone)]
pub struct Mark(pub String);

impl<M> crate::tinted::Diffed<M> for Mark {
    fn label(&mut self) -> String {
        self.0.clone()
    }

    fn operate(&mut self, visit: &mut dyn Visit) {
        visit.visit(&format!("{}!", self.0));
    }

    fn seen(&self, _spot: crate::walks::Spot, node: &crate::walks::Node) -> String {
        format!("{}!{}", self.0, node.marked())
    }

    fn publish(&mut self, sink: &mut Sink<'_>) {
        sink.note(&format!("{}!", self.0));
    }
}

/// A label whose own `lifted` lifts the node it is given out, borrowed
/// for as long as the label and the node, as a native iced widget with an
/// overlay gives an `overlay::Element<'b, ..>` that borrows it and its
/// tree. The crate compiles its impl.
#[derive(Debug, Clone)]
pub struct Lift(pub String);

impl<M> crate::tinted::Diffed<M> for Lift {
    fn label(&mut self) -> String {
        self.0.clone()
    }

    fn lifted<'b>(&'b mut self, node: &'b mut crate::walks::Node) -> Vec<crate::walks::Lifted<'b>> {
        node.mark(&format!("{}^", self.0));
        vec![crate::walks::Lifted { node }]
    }
}

impl Lift {
    /// `node` lifted out at `depth`, borrowed for as long as the label and
    /// the node, beside a depth it reads for the call alone, as iced's
    /// `overlay(&'b mut self, tree: &'b mut Tree, .., renderer: &Renderer,
    /// ..)` is.
    pub fn lifted_at<'b>(
        &'b mut self,
        node: &'b mut crate::walks::Node,
        depth: &usize,
    ) -> Vec<crate::walks::Lifted<'b>> {
        node.mark(&format!("{}^{depth}", self.0));
        vec![crate::walks::Lifted { node }]
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

/// What `item`'s `publish` notes in a sink of notes it owns, as iced's
/// `UserInterface::update` lends the root widget a `&mut Shell<'_,
/// Message>`. The crate compiles it.
pub fn published<M>(mut item: impl crate::tinted::Diffed<M>) -> Vec<String> {
    let mut notes = Vec::new();
    item.publish(&mut Sink { notes: &mut notes });
    notes
}
