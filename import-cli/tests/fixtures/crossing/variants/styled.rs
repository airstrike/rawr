// rawr
use events::styled::{self, Palette, Styled};

struct Viewer<'a> {
    palette: &'a Palette,
}

impl<'a> Styled for Viewer<'a> {
    fn palette(&self) -> &Palette {
        self.palette
    }
}

struct Owner {
    palette: Palette,
}

impl Styled for Owner {
    fn palette(&self) -> &Palette {
        &self.palette
    }
}

fn main() {
    let mut palette = Palette::new("dusk");
    println!("lent {}", styled::describe(&Viewer { palette: &palette }));
    palette.rename("noon");
    println!("again {}", styled::describe(&Viewer { palette: &palette }));
    let owner = Owner {
        palette: Palette::new("dawn"),
    };
    println!("owned {}", styled::describe(&owner));
}
