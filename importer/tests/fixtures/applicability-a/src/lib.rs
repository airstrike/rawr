use facts_b::{Assoc, Base, Disabled, Enabled, Gate, LeftMark, RightMark};
#[derive(Clone, Copy)]
pub struct LeftOnly;
#[derive(Clone, Copy)]
pub struct RightOnly;
#[derive(Clone, Copy)]
pub struct Both;
#[derive(Clone, Copy)]
pub struct Neither;
impl Base<i64> for LeftOnly {
    fn base(&self) -> i64 {
        101
    }
}
impl Base<i64> for RightOnly {
    fn base(&self) -> i64 {
        202
    }
}
impl Base<i64> for Both {
    fn base(&self) -> i64 {
        303
    }
}
impl Base<i64> for Neither {
    fn base(&self) -> i64 {
        404
    }
}
impl LeftMark for LeftOnly {}
impl RightMark for RightOnly {}
impl LeftMark for Both {}
impl RightMark for Both {}
impl Assoc for LeftOnly {
    type Output = i64;
    fn associated(&self) -> i64 {
        111
    }
}
impl Assoc for RightOnly {
    type Output = bool;
    fn associated(&self) -> bool {
        true
    }
}
impl Gate for LeftOnly {
    type Mode = Enabled;
}
impl Gate for RightOnly {
    type Mode = Disabled;
}
mod hidden {
    #[derive(Clone, Copy)]
    pub struct Carrier<T>(pub T);
}
pub type Alias = hidden::Carrier<i64>;
pub fn alias() -> Alias {
    hidden::Carrier(707)
}
impl<T: Copy> Base<T> for hidden::Carrier<T> {
    fn base(&self) -> T {
        self.0
    }
}
impl<T: Copy> Assoc for hidden::Carrier<T> {
    type Output = T;
    fn associated(&self) -> T {
        self.0
    }
}
impl<T: Copy> LeftMark for hidden::Carrier<T> {}

pub fn left() -> LeftOnly {
    LeftOnly
}
pub fn right() -> RightOnly {
    RightOnly
}
pub fn both() -> Both {
    Both
}
pub fn neither() -> Neither {
    Neither
}

// Intrinsic inventories retain owned private facts even when public demand has
// no route to their declarations. This is deliberately absent from the facade.
#[allow(dead_code)]
mod retained_only {
    trait Hidden {
        fn retained(&self) -> i64;
    }
    struct Unused;
    impl Hidden for Unused {
        fn retained(&self) -> i64 {
            909
        }
    }
}
