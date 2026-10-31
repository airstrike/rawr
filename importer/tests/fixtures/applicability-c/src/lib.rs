use facts_b::{Assoc, Base, Disabled, Enabled, Gate, LeftMark, RightMark};
pub trait Extension<U> {
    fn extension(&self) -> U;
}
impl<T, U> Extension<U> for T
where
    T: Base<U>,
{
    fn extension(&self) -> U {
        self.base()
    }
}
pub trait Left {
    fn choose(&self) -> i64;
}
impl<T: Base<i64> + LeftMark> Left for T {
    fn choose(&self) -> i64 {
        11
    }
}
pub trait Right {
    fn choose(&self) -> bool;
}
impl<T: Base<i64> + RightMark> Right for T {
    fn choose(&self) -> bool {
        true
    }
}
pub trait LeftSame {
    fn same(&self) -> i64;
}
impl<T: Base<i64> + LeftMark> LeftSame for T {
    fn same(&self) -> i64 {
        11
    }
}
pub trait RightSame {
    fn same(&self) -> i64;
}
impl<T: Base<i64> + RightMark> RightSame for T {
    fn same(&self) -> i64 {
        22
    }
}
pub trait Projection: Assoc {
    fn projected(&self) -> Self::Output {
        self.associated()
    }
}
impl<T: Assoc> Projection for T {}
pub trait EqualityLeft {
    fn equal_choose(&self) -> i64;
}
impl<T: Base<i64> + Gate<Mode = Enabled>> EqualityLeft for T {
    fn equal_choose(&self) -> i64 {
        31
    }
}
pub trait EqualityRight {
    fn equal_choose(&self) -> bool;
}
impl<T: Base<i64> + Gate<Mode = Disabled>> EqualityRight for T {
    fn equal_choose(&self) -> bool {
        false
    }
}
