//! The program's own `macro_rules!` macros across modules under `rawr
//! run` (#236): Rust's textual scope, `#[macro_use]` and rustc's errors.
//! Each expected output is the rustc build's.

use std::fs;

use super::{assert_success, stderr, stdout, Sandbox};

/// rustlings' macros3, solved.
const MACRO_USE: &str = "// rawr
#[macro_use]
mod macros {
    macro_rules! my_macro {
        () => {
            println!(\"Check out my macro!\");
        };
    }
}

fn main() {
    my_macro!();
}
";

#[test]
fn a_macro_use_module_lets_its_macros_into_its_parent() {
    let sandbox = Sandbox::new("macros-use");
    fs::write(sandbox.work().join("macros3.rs"), MACRO_USE).unwrap();
    let run = sandbox.rawr(&["run", "macros3.rs"]);
    assert_success(&run);
    assert_eq!(stdout(&run), "Check out my macro!\n");
}

#[test]
fn a_modules_macro_without_macro_use_is_rustcs_error() {
    let sandbox = Sandbox::new("macros-hidden");
    fs::write(
        sandbox.work().join("macros3.rs"),
        MACRO_USE.replace("#[macro_use]\n", ""),
    )
    .unwrap();
    let run = sandbox.rawr(&["run", "macros3.rs"]);
    assert_eq!(run.status.code(), Some(1));
    let text = stderr(&run);
    assert!(
        text.starts_with("error: cannot find macro `my_macro` in this scope\n"),
        "{text}"
    );
    assert!(
        text.contains("help: have you added the `#[macro_use]` on the module/import?"),
        "{text}"
    );
}

#[test]
fn file_modules_see_the_macros_defined_before_them() {
    let sandbox = Sandbox::new("macros-files");
    let work = sandbox.work();
    fs::write(
        work.join("main.rs"),
        "// rawr\nmacro_rules! twice { ($x:expr) => { $x * 2 }; }\n#[macro_use]\nmod helpers;\n\
         mod child;\nfn main() {\n    println!(\"{} {} {}\", twice!(4), triple!(2), child::six());\n}\n",
    )
    .unwrap();
    fs::write(
        work.join("helpers.rs"),
        "// rawr\nmacro_rules! triple { ($x:expr) => { $x * 3 }; }\n",
    )
    .unwrap();
    fs::write(
        work.join("child.rs"),
        "// rawr\npub fn six() -> i32 { twice!(triple!(1)) }\n",
    )
    .unwrap();
    let run = sandbox.rawr(&["run", "main.rs"]);
    assert_success(&run);
    assert_eq!(stdout(&run), "8 6 6\n");
}

#[test]
fn a_macro_called_before_its_definition_points_at_the_definition() {
    let sandbox = Sandbox::new("macros-later");
    fs::write(
        sandbox.work().join("macros2.rs"),
        "// rawr\nfn main() {\n    my_macro!();\n}\n\nmacro_rules! my_macro {\n    () => {};\n}\n",
    )
    .unwrap();
    let run = sandbox.rawr(&["run", "macros2.rs"]);
    assert_eq!(run.status.code(), Some(1));
    let text = stderr(&run);
    assert!(
        text.starts_with("error: cannot find macro `my_macro` in this scope\n"),
        "{text}"
    );
    assert!(
        text.contains("consider moving the definition of `my_macro` before this call"),
        "{text}"
    );
    assert!(
        text.contains("a macro with the same name exists, but it appears later"),
        "{text}"
    );
    assert!(text.contains("macros2.rs:6:14"), "{text}");
}
