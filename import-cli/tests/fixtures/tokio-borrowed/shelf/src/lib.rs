//! Async functions that borrow their arguments, as fount's
//! `async fn load(font: &Font) -> Result<Vec<u8>, Error>`: each future
//! holds its borrow across an await.

/// A font file, as fount's `system::Font`.
#[derive(Debug, Clone)]
pub struct Font {
    /// Its family's name.
    pub family: String,
    /// Its bytes.
    pub bytes: Vec<u8>,
}

/// A font of `family` with `size` bytes.
pub fn font(family: String, size: u8) -> Font {
    Font {
        family,
        bytes: (0..size).collect(),
    }
}

/// The font's size, read after a yield, through the borrow its future
/// holds.
pub async fn load(font: &Font) -> usize {
    tokio::task::yield_now().await;
    font.bytes.len()
}

/// The font renamed after a yield, through the mutable borrow its future
/// holds.
pub async fn rename(font: &mut Font, family: String) {
    tokio::task::yield_now().await;
    font.family = family;
}

/// A catalog of families, as fount's `google` module has.
pub mod google {
    /// The families a catalog lists, as fount's `google::Catalog`.
    #[derive(Debug, Clone)]
    pub struct Catalog {
        /// The families' names.
        pub families: Vec<String>,
    }

    /// A catalog of `families`.
    pub fn catalog(families: Vec<String>) -> Catalog {
        Catalog { families }
    }

    /// A family's name and its catalog's size after a yield, through the
    /// tuple of borrows its future holds.
    pub async fn pair(pair: (&str, &Catalog)) -> usize {
        tokio::task::yield_now().await;
        pair.0.len() + pair.1.families.len()
    }

    /// Where `family` comes from after a yield, as fount's `async fn
    /// load(family: &str, catalog: Option<&Catalog>)`: its future borrows
    /// the string and, when there is one, the catalog.
    pub async fn load(family: &str, catalog: Option<&Catalog>) -> String {
        tokio::task::yield_now().await;
        match catalog {
            Some(catalog) if catalog.families.iter().any(|listed| listed == family) => {
                format!("{family} listed")
            }
            Some(catalog) => format!("{family} unlisted of {}", catalog.families.len()),
            None => format!("{family} uncataloged"),
        }
    }
}
