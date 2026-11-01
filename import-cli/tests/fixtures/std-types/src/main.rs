// rawr
//! std's `PathBuf`, `&Path` and `io::Error` are the same types in the
//! program and in the crate it uses: a crate struct's `PathBuf` field, a
//! crate function taking `&Path` or giving an `io::Error`, and `?` from an
//! `io::Result` into the crate's error, which converts as thiserror's
//! `#[from]` writes it.

use std::io;
use std::path::{Path, PathBuf};

use files::{Entry, Error};

fn load(path: &Path) -> Result<String, Error> {
    let text = std::fs::read_to_string(path)?;
    Ok(text)
}

fn open(path: &Path) -> io::Result<usize> {
    let text = std::fs::read_to_string(path)?;
    Ok(text.len())
}

fn main() {
    let root: PathBuf = files::root();
    let joined = root.join("notes").join("a.txt");
    println!("joined {}", joined.display());
    println!("depth {}", files::depth(&joined));

    let entry = files::entry(&joined, 3);
    println!(
        "entry {} {} {}",
        entry.path.display(),
        entry.size,
        entry.name()
    );
    let made = Entry::new(PathBuf::from("x/y.rs"), 1);
    println!("made {:?} {}", made.path.extension(), made.name());
    println!(
        "lent {:?} {}",
        made.path().file_stem(),
        made.path().display()
    );
    let literal = Entry {
        path: root.join("b"),
        size: 2,
    };
    println!(
        "literal {} {}",
        literal.path.display(),
        files::depth(&literal.path)
    );

    let error = files::missing(Path::new("gone.txt"));
    println!("missing {} {:?}", error, files::kind(&error));
    let own = io::Error::new(io::ErrorKind::TimedOut, "slow");
    println!("own {:?}", files::kind(&own));

    match load(Path::new("no/such/file")) {
        Ok(text) => println!("loaded {}", text.len()),
        Err(error) => println!("load {}", error),
    }
    match files::under(&root, "") {
        Ok(path) => println!("under {}", path.display()),
        Err(error) => println!("under {}", error),
    }
    println!(
        "under {:?}",
        files::under(&root, "c")
            .map(|path| path.display().to_string())
            .ok()
    );
    match files::read(Path::new("no/such/file")) {
        Ok(text) => println!("read {}", text.len()),
        Err(error) => println!("read {}", error),
    }
    println!(
        "open {:?}",
        open(Path::new("no/such/file")).map_err(|error| error.kind())
    );
    let wrapped: Error = own.into();
    println!("wrapped {}", wrapped);
}
