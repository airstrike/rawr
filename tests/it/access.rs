//! What a program may reach outside its engine (D683.4): nothing until the
//! embedder grants it, and a file grant may name directories.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rawr::{Access, Engine};

/// A directory of its own under the temporary directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!("rawr-access-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Self(path.canonicalize().unwrap())
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// What `source` prints on an engine granted `access`, or its error.
fn printed(access: Access, source: &str) -> Result<String, String> {
    let lines = Arc::new(Mutex::new(String::new()));
    let mut engine = Engine::new();
    engine.set_access(access);
    let sink = lines.clone();
    engine.on_output(move |_, text| sink.lock().unwrap().push_str(text));
    engine.run(source).map_err(|error| error.to_string())?;
    Ok(lines.lock().unwrap().clone())
}

fn quoted(path: &Path) -> String {
    format!("{:?}", path.display().to_string())
}

#[test]
fn a_default_engine_refuses_files_the_environment_and_the_process_at_the_call() {
    let scratch = Scratch::new("default");
    let file = scratch.0.join("secret.txt");
    std::fs::write(&file, "secret").unwrap();
    let path = quoted(&file);

    let printed = printed(
        Access::none(),
        &format!(
            r#"
use std::fs;
fn main() {{
    println!("{{:?}}", fs::read_to_string({path}).map_err(|error| error.kind()));
    println!("{{:?}}", fs::write({path}, "x").map_err(|error| error.kind()));
    match fs::read({path}) {{
        Ok(_) => println!("read"),
        Err(error) => println!("{{error}}"),
    }}
    println!("{{}} {{:?}}", std::env::args().count(), option_env!("HOME"));
}}
main();
"#
        ),
    )
    .unwrap();
    assert_eq!(
        printed,
        "Err(PermissionDenied)\nErr(PermissionDenied)\n\
         permission denied: this program may not read this path\n0 None\n"
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "secret");

    // `env!` of a variable the program can't see is rustc's error at it.
    let error = printed_error("fn main() { let home = env!(\"HOME\"); }\nmain();");
    assert!(error.contains("environment variable `HOME` not defined at compile time"), "{error}");
    let error = printed_error("fn main() { println!(\"{}\", std::process::id()); }\nmain();");
    assert!(error.contains("no pids on this platform"), "{error}");
}

fn printed_error(source: &str) -> String {
    printed(Access::none(), source).unwrap_err()
}

#[test]
fn an_engine_granted_files_reads_and_writes_them() {
    let scratch = Scratch::new("granted");
    let directory = quoted(&scratch.0);

    let printed = printed(
        Access::none().allow_read_all().allow_write_all(),
        &format!(
            r#"
use std::fs;
use std::path::PathBuf;
fn main() -> Result<(), std::io::Error> {{
    let directory = PathBuf::from({directory});
    fs::create_dir_all(directory.join("sub"))?;
    let file = directory.join("sub").join("note.txt");
    fs::write(&file, "hello")?;
    let bytes: Vec<u8> = fs::read(&file)?;
    println!("{{}} {{}} {{}}", fs::read_to_string(&file)?, bytes.len(), fs::exists(&file)?);
    fs::copy(&file, directory.join("copy.txt"))?;
    fs::rename(directory.join("copy.txt"), directory.join("moved.txt"))?;
    fs::remove_file(&file)?;
    println!("{{:?}}", fs::read_to_string(&file).map_err(|error| error.kind()));
    Ok(())
}}
main().unwrap();
"#
        ),
    )
    .unwrap();
    assert_eq!(printed, "hello 5 true\nErr(NotFound)\n");
    assert_eq!(std::fs::read_to_string(scratch.0.join("moved.txt")).unwrap(), "hello");
}

#[test]
fn a_directory_grant_refuses_a_path_outside_it() {
    let scratch = Scratch::new("scoped");
    let inside = scratch.0.join("inside");
    std::fs::create_dir_all(&inside).unwrap();
    std::fs::write(scratch.0.join("outside.txt"), "outside").unwrap();
    let file = quoted(&inside.join("note.txt"));
    let outside = quoted(&scratch.0.join("outside.txt"));
    let climbing = quoted(&inside.join("../outside.txt"));

    let printed = printed(
        Access::none().allow_read(&inside).allow_write(&inside),
        &format!(
            r#"
use std::fs;
fn main() {{
    fs::write({file}, "inside").unwrap();
    println!("{{}}", fs::read_to_string({file}).unwrap());
    println!("{{:?}}", fs::read_to_string({outside}).map_err(|error| error.kind()));
    println!("{{:?}}", fs::read_to_string({climbing}).map_err(|error| error.kind()));
    println!("{{:?}}", fs::write({outside}, "x").map_err(|error| error.kind()));
}}
main();
"#
        ),
    )
    .unwrap();
    assert_eq!(printed, "inside\nErr(PermissionDenied)\nErr(PermissionDenied)\nErr(PermissionDenied)\n");
    assert_eq!(std::fs::read_to_string(scratch.0.join("outside.txt")).unwrap(), "outside");
}
