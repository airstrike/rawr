//! A native module of the marked crate, as changelog's `changelog.rs` is:
//! its `split` gives a `Result` of a tuple holding a `Vec`, as
//! `Changelog::list` gives `Result<(Self, Vec<Contribution>), Error>`.

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub size: usize,
}

#[derive(Clone, Debug)]
pub enum Error {
    Empty,
}

#[derive(Clone, Debug)]
pub struct Listing {
    names: Vec<String>,
}

impl Listing {
    /// The listing of `text`'s words, and an entry for each.
    pub fn split(text: &str) -> Result<(Self, Vec<Entry>), Error> {
        let names = text
            .split_whitespace()
            .map(String::from)
            .collect::<Vec<_>>();
        if names.is_empty() {
            return Err(Error::Empty);
        }
        let entries = names
            .iter()
            .map(|name| Entry {
                name: name.clone(),
                size: name.len(),
            })
            .collect();
        Ok((Self { names }, entries))
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }
}
