// rawr
//! std's `PathBuf`, `&Path` and `io::Error` are the same types in the
//! program and in the crate it uses: a crate struct's `PathBuf` field, a
//! crate function taking `&Path` or giving an `io::Error`, and `?` from an
//! `io::Result` into the crate's error, which converts as thiserror's
//! `#[from]` writes it. A crate function taking `&mut PathBuf` changes the
//! program's own path, and a crate method giving `&io::Error` lends the
//! error its value holds (#680).

use std::io;
use std::path::{Path, PathBuf};

use files::{Entry, Error, Failure};

struct Job {
    name: String,
    path: PathBuf,
}

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

    let mut grown = PathBuf::from("data");
    files::extend(&mut grown, "notes");
    files::extend(&mut grown, "b.txt");
    println!("extended {} {}", grown.display(), files::depth(&grown));

    let mut job = Job {
        name: String::from("job"),
        path: PathBuf::from("jobs"),
    };
    files::extend(&mut job.path, "first");
    println!("job {} {}", job.name, job.path.display());
    files::reserve(&mut grown, 200);
    files::reserve(&mut job.path, 200);
    println!(
        "roomy {} {}",
        files::roomy(&mut grown),
        files::roomy(&mut job.path)
    );

    let failure = Failure::new(PathBuf::from("lost.txt"));
    let error = failure.error();
    println!("error {} {:?}", error, error.kind());
    let cause = failure.error();
    println!("cause {:?} {}", files::kind(cause), failure.path.display());
    failure.append_to(&mut grown);
    failure.append_to(&mut job.path);
    println!("appended {} {}", grown.display(), job.path.display());
}
