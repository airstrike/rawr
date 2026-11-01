use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

use rawr_directory::{Admission as _, Work};

use super::{caller, names, observe, output, Host};
use crate::node::Error;

static NEXT: AtomicU64 = AtomicU64::new(0);

#[test]
fn pure_sort_and_temporary_union_retention_have_no_fictitious_probe_cost() {
    let mut host = Host { visible_limit: 1 };
    for entries in 1..=61 {
        host.admit(Work::Retain {
            entries,
            name_bytes: 4,
        })
        .expect("raw temporary count is not the visible-member limit");
    }
    for event in [
        Work::Name { bytes: 5 },
        Work::Copy { bytes: 8 },
        Work::Order,
        Work::Compare { bytes: 4 },
    ] {
        host.admit(event).expect("bounded pure work is admitted");
    }
    host.admit(Work::Visible { entries: 1 })
        .expect("61 raw entries can collapse to one visible member");
    assert!(matches!(
        host.admit(Work::Visible { entries: 2 }),
        Err(Error::Limit {
            resource: "runner bundle directory members",
            limit: 1,
            actual: 2
        })
    ));
}

#[test]
fn caller_ownership_and_terminal_reader_errors() {
    let path = std::path::Path::new("/directory-observation-must-not-run");
    let result = observe(path, 1, |_, _, _| Err(rawr_directory::Error::Terminal));
    assert!(matches!(
        result,
        Err(Error::State("native directory reader is terminal"))
    ));
    let (bytes, owned_path) = caller(path).expect("caller allocations succeed");
    assert_eq!(owned_path, path);
    assert_eq!(bytes.last(), Some(&0));
    assert_eq!(
        &bytes[..bytes.len() - 1],
        path.as_os_str().as_encoded_bytes()
    );
}

#[test]
fn missing_directory_reports_its_io_error() {
    let missing = std::path::Path::new("/rawr-directory-nonexistent-open-fixture");
    let result = names(missing, 1);
    assert!(
        matches!(result, Err(Error::Io { path, source, .. }) if path == missing && source.kind() == std::io::ErrorKind::NotFound)
    );
}

#[test]
fn output_container_and_joined_paths_are_built() {
    assert!(output(2).unwrap().capacity() >= 2);
    let parent = std::path::Path::new("/exact-parent");
    let child = std::path::Path::new("native-child");
    assert_eq!(
        super::super::path(parent, child).unwrap(),
        parent.join(child)
    );
}

#[test]
fn real_adapter_observation_preserves_names_and_rejects_visible_excess() {
    let fixture = std::env::temp_dir().join(format!(
        "rawr-directory-budget-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&fixture).expect("the inert fixture directory exists");
    fs::write(fixture.join("z"), []).expect("one inert member exists");
    fs::create_dir(fixture.join("a")).expect("an empty directory exists");
    assert_eq!(
        names(&fixture, 2).expect("the complete directory is observed"),
        [std::ffi::OsString::from("a"), std::ffi::OsString::from("z")]
    );
    assert!(matches!(
        names(&fixture, 1),
        Err(Error::Limit {
            resource: "runner bundle directory members",
            limit: 1,
            actual: 2
        })
    ));
    fs::remove_dir_all(&fixture).expect("the inert fixture retires");
}

#[test]
fn production_bundle_names_and_paths_preserve_complete_sorted_members() {
    let fixture = std::env::temp_dir().join(format!(
        "rawr-directory-entry-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir(&fixture).unwrap();
    fs::write(fixture.join("z"), []).unwrap();
    fs::create_dir(fixture.join(".fingerprint")).unwrap();
    fs::create_dir(fixture.join("empty")).unwrap();
    let expected = [".fingerprint", "empty", "z"];
    assert_eq!(
        super::super::names(&fixture, 3).unwrap(),
        expected.map(std::ffi::OsString::from)
    );
    assert_eq!(
        super::super::paths(&fixture, 3).unwrap(),
        expected.map(|name| fixture.join(name))
    );
    for result in [
        super::super::names(&fixture, 2).map(|_| ()),
        super::super::paths(&fixture, 2).map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(Error::Limit {
                resource: "runner bundle directory members",
                limit: 2,
                actual: 3
            })
        ));
    }
    fs::remove_dir_all(fixture).unwrap();
}

#[test]
fn production_bundle_entry_points_report_io_errors() {
    let missing = std::path::Path::new("/rawr-directory-missing-production-entry");
    for result in [
        super::super::names(missing, 1).map(|_| ()),
        super::super::paths(missing, 1).map(|_| ()),
    ] {
        assert!(
            matches!(result, Err(Error::Io { path, source, .. }) if path == missing && source.kind() == std::io::ErrorKind::NotFound)
        );
    }
}
