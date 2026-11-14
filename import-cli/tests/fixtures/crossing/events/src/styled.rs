//! A trait whose method gives a reference to a crate value its implementor
//! holds, in the shape of iced markdown's `Viewer::theme(&self) -> &Theme`.

/// A crate value a program holds and lends by reference.
#[derive(Debug)]
pub struct Palette {
    name: String,
}

impl Palette {
    /// A palette named `name`.
    #[must_use]
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
        }
    }

    /// Name it `name`, which takes it `&mut`.
    pub fn rename(&mut self, name: &str) {
        name.clone_into(&mut self.name);
    }
}

/// What gives the palette it shows with.
pub trait Styled {
    /// The palette, borrowed from `self`.
    fn palette(&self) -> &Palette;
}

/// The palette's name, and whether two calls gave the one palette.
pub fn describe(styled: &impl Styled) -> String {
    let first = styled.palette();
    let second = styled.palette();
    format!("{} {}", first.name, std::ptr::eq(first, second))
}
