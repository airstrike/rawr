// rawr
// iced's editor: `load_file(picked_file)` passes rfd's `FileHandle` to an
// `impl Into<PathBuf>`, which rfd's `impl From<FileHandle> for PathBuf`
// satisfies. A crate's impl of a std trait for a std type Rawr's std holds
// is seen as rustc sees it: through the program's function's, its
// method's and the crate's `impl Into<PathBuf>`, by `.into()` into a
// `PathBuf` and by `PathBuf::from`. A value of the generic's type that
// reaches `.into()` by another binding or a tuple converts by the bound
// too (#1720).

use std::path::PathBuf;

use events::named::{owned, picked};

fn loaded(path: impl Into<PathBuf>) -> String {
    let path = path.into();
    format!("loaded {}", path.display())
}

fn moved<P: Into<PathBuf>>(path: P) -> String {
    let held = path;
    let pair = (held, 1);
    let path = pair.0.into();
    format!("moved {}", path.display())
}

struct Folder {
    name: &'static str,
}

impl Folder {
    fn opened(&self, path: impl Into<PathBuf>) -> String {
        let path = path.into();
        format!("{} opened {}", self.name, path.display())
    }
}

fn main() {
    println!("{}", loaded(picked("a.txt")));
    println!("{}", loaded("b.txt"));
    let path: PathBuf = picked("c.txt").into();
    println!("{}", path.display());
    println!("{}", PathBuf::from(picked("d.txt")).display());
    println!("{}", owned(picked("e")));
    let folder = Folder { name: "docs" };
    println!("{}", folder.opened(picked("f.txt")));
    println!("{}", folder.opened("g.txt"));
    println!("{}", moved(picked("h.txt")));
    println!("{}", moved("i.txt"));
}
