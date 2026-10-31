pub trait Base<U> {
    fn base(&self) -> U;
}
pub trait LeftMark {}
pub trait RightMark {}
pub trait Assoc {
    type Output;
    fn associated(&self) -> Self::Output;
}
pub trait Gate {
    type Mode;
}
pub struct Enabled;
pub struct Disabled;
