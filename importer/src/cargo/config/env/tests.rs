use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;

use toml_edit::DocumentMut;

use super::{Rank, Source, Table};
use crate::Error;

fn source<'path>(rank: Rank, path: &'path Path, text: &str) -> Source<'path> {
    let mut document = text.parse::<DocumentMut>().expect("fixture config parses");
    Source {
        rank,
        path,
        item: document.remove("env").expect("fixture config has [env]"),
    }
}

fn table(sources: &mut [Source<'_>]) -> Table {
    Table::merge(sources).expect("fixture [env] merges")
}

fn process(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).into(), (*value).into()))
        .collect()
}

#[test]
fn a_config_value_reaches_the_script_only_when_the_process_leaves_it_unset() {
    let path = Path::new("/project/.cargo/config.toml");
    let table = table(&mut [source(
        Rank::Hierarchy(2),
        path,
        "[env]\nCC = 'clang'\nFORCED = { value = 'config', force = true }\n",
    )]);
    assert_eq!(table.value("CC", &process(&[])), Some("clang"));
    assert_eq!(table.value("CC", &process(&[("CC", "gcc")])), Some("gcc"));
    assert_eq!(
        table.value("FORCED", &process(&[("FORCED", "process")])),
        Some("config")
    );
    assert_eq!(table.value("UNSET", &process(&[])), None);
    assert_eq!(table.value("SET", &process(&[("SET", "x")])), Some("x"));
}

#[test]
fn a_relative_value_joins_the_directory_above_its_defining_cargo_directory() {
    let near = Path::new("/work/project/.cargo/config.toml");
    let far = Path::new("/work/.cargo/config.toml");
    let table = table(&mut [
        source(
            Rank::Hierarchy(3),
            near,
            "[env]\nTOOL = { value = 'bin/tool', relative = true }\n",
        ),
        source(Rank::Hierarchy(2), far, "[env.TOOL]\nforce = true\n"),
        source(
            Rank::Hierarchy(2),
            far,
            "[env]\nPLAIN = { value = 'bin/tool', relative = false }\n",
        ),
    ]);
    let environment = process(&[("TOOL", "process")]);
    assert_eq!(
        table.value("TOOL", &environment),
        Some("/work/project/bin/tool")
    );
    assert_eq!(table.value("PLAIN", &environment), Some("bin/tool"));
}

#[test]
fn the_nearer_file_wins_each_field_and_cli_files_win_over_both() {
    let home = Path::new("/home/.cargo/config.toml");
    let near = Path::new("/a/b/.cargo/config.toml");
    let far = Path::new("/a/.cargo/config.toml");
    let cli = Path::new("/elsewhere/.cargo/config.toml");
    let later = Path::new("/later/.cargo/config.toml");
    let table = table(&mut [
        source(
            Rank::Home,
            home,
            "[env]\nA = 'home'\nB = 'home'\nC = 'home'\nD = 'home'\n",
        ),
        source(Rank::Hierarchy(2), far, "[env]\nA = 'far'\nB = 'far'\n"),
        source(Rank::Hierarchy(3), near, "[env]\nA = 'near'\n"),
        source(Rank::Cli(1), later, "[env]\nD = 'later'\n"),
        source(Rank::Cli(0), cli, "[env]\nC = 'cli'\nD = 'cli'\n"),
    ]);
    let environment = process(&[]);
    assert_eq!(table.value("A", &environment), Some("near"));
    assert_eq!(table.value("B", &environment), Some("far"));
    assert_eq!(table.value("C", &environment), Some("cli"));
    assert_eq!(table.value("D", &environment), Some("later"));
}

#[test]
fn a_string_and_a_table_for_one_key_do_not_merge() {
    let near = Path::new("/a/b/.cargo/config.toml");
    let far = Path::new("/a/.cargo/config.toml");
    let result = Table::merge(&mut [
        source(Rank::Hierarchy(3), near, "[env]\nA = 'near'\n"),
        source(Rank::Hierarchy(2), far, "[env]\nA = { value = 'far' }\n"),
    ]);
    assert!(matches!(
        result,
        Err(Error::Request { reason }) if reason.contains("expected a string, but found table")
    ));
}

#[test]
fn a_table_without_a_value_and_cargo_home_are_refused_as_cargo_refuses_them() {
    let path = Path::new("/a/.cargo/config.toml");
    let missing = Table::merge(&mut [source(
        Rank::Hierarchy(2),
        path,
        "[env]\nA = { force = true }\n",
    )]);
    assert!(matches!(
        missing,
        Err(Error::Request { reason }) if reason.contains("missing field `value`")
    ));
    let home = Table::merge(&mut [source(
        Rank::Hierarchy(2),
        path,
        "[env]\nCARGO_HOME = '/x'\n",
    )]);
    assert!(matches!(
        home,
        Err(Error::Request { reason }) if reason.contains("`CARGO_HOME` environment variable")
    ));
}

#[test]
fn apply_gives_a_child_environment_the_values_cargo_would() {
    let path = Path::new("/project/.cargo/config.toml");
    let table = table(&mut [source(
        Rank::Hierarchy(2),
        path,
        "[env]\nCC = 'clang'\nKEPT = 'config'\nFORCED = { value = 'config', force = true }\n",
    )]);
    let mut environment = BTreeMap::from([
        ("KEPT".to_owned(), "process".to_owned()),
        ("FORCED".to_owned(), "process".to_owned()),
    ]);
    table.apply(&mut environment);
    assert_eq!(
        environment,
        BTreeMap::from([
            ("CC".to_owned(), "clang".to_owned()),
            ("FORCED".to_owned(), "config".to_owned()),
            ("KEPT".to_owned(), "process".to_owned()),
        ])
    );
}
