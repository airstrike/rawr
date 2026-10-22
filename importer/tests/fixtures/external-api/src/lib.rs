pub trait ForeignTrait {
    type Associated;
    const MARKER: usize;

    fn foreign(&self) -> Self::Associated;
}
