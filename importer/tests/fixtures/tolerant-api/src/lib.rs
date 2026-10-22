pub struct Lanes<const N: u32>;

pub struct Switch<const ON: bool>;

pub fn lanes() -> Lanes<{ 2 + 2 }> {
    Lanes
}

pub fn switch() -> Switch<{ !false }> {
    Switch
}

pub fn plain() -> u8 {
    1
}
