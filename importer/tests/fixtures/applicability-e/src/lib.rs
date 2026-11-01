//! An implementation of a trait a dependency re-exports from its own
//! dependency: its members' declarations are that crate's.

pub struct Counted;

impl facts_d::Assoc for Counted {
    type Output = u8;
    fn associated(&self) -> u8 {
        3
    }
}
