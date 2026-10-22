pub struct Deep(pub u8);

impl Deep {
    pub fn value(&self) -> u8 {
        self.0
    }
}

pub mod nested {
    pub struct Deeper;

    pub fn deeper() -> Deeper {
        Deeper
    }
}
