use std::fs::{self, File};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::{symlink, MetadataExt as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::input::membership::live;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rawr-live-discovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("fixture directory");
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Opened {
    _file: File,
    identity: Identity,
}

/// Test-only source-shaped same-file open followed by descriptor metadata.
/// The File remains owned on success, including through the second scope.
fn opened(path: &Path) -> std::result::Result<Opened, Attempt> {
    let file = fs::OpenOptions::new()
        .read(true)
        .open(path)
        .map_err(|error| {
            Attempt::OpenFailed(NativeError::Os(
                error.raw_os_error().expect("native open errno"),
            ))
        })?;
    let metadata = file
        .metadata()
        .map_err(|error| Attempt::StatFailed(error.raw_os_error().expect("native fstat errno")))?;
    Ok(Opened {
        _file: file,
        identity: Identity {
            device: metadata.dev(),
            inode: metadata.ino(),
        },
    })
}

/// Source-shaped physical projection, not a full Cargo differential: no shell
/// is created and no warning delivery/success is asserted by this reference.
fn reference(directory: &Path, filename: Filename, warn: bool) -> Transcript {
    let legacy_path = directory.join(filename.legacy());
    let toml_path = directory.join(filename.toml());
    let (legacy, after) = match opened(&legacy_path) {
        Ok(first) => {
            let after = if warn {
                match opened(&toml_path) {
                    Ok(second) => {
                        let after = After::Second(Attempt::Acquired(second.identity));
                        // Evaluate the same equality while both Files live.
                        let _same = first.identity == second.identity;
                        after
                    }
                    Err(second) => After::Second(second),
                }
            } else {
                After::None
            };
            (Attempt::Acquired(first.identity), after)
        }
        Err(first) => {
            let exists = match fs::metadata(&toml_path) {
                Ok(_) => Exists::Present,
                Err(error) => Exists::Failed(NativeError::Os(
                    error.raw_os_error().expect("native stat errno"),
                )),
            };
            (first, After::Fallback(exists))
        }
    };
    Transcript::checked(filename, warn, legacy, after).expect("source-shaped transcript")
}

fn compare(directory: &Path, filename: Filename) {
    for warn in [false, true] {
        let limits = cargo::Limits::standard();
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::new(&limits);
        let record = live::discovery(
            directory.as_os_str().as_bytes(),
            filename,
            warn,
            &mut Budget {
                graph: &graph,
                allocation: &mut allocation,
            },
        )
        .expect("live discovery");
        let observed::Query::Discovery {
            directory: retained,
            filename: family,
            warn: mode,
        } = &record.query
        else {
            panic!("discovery query")
        };
        assert_eq!(retained.bytes(), directory.as_os_str().as_bytes());
        assert_eq!((*family, *mode), (filename, warn));
        assert_eq!(transcript(record), reference(directory, filename, warn));
    }
}

#[test]
fn native_projection_matches_source_shaped_std_acquisition() {
    let fixture = Fixture::new();
    for filename in [Filename::Config, Filename::Credentials] {
        for case in 0..9 {
            let directory = fixture.0.join(format!("{}-{case}", filename.legacy()));
            fs::create_dir(&directory).expect("case directory");
            let legacy = directory.join(filename.legacy());
            let toml = directory.join(filename.toml());
            match case {
                0 => {} // Both absent.
                1 => fs::write(&toml, b"toml only").expect("toml"),
                2 => fs::write(&legacy, b"legacy only").expect("legacy"),
                3 => {
                    fs::write(&legacy, b"legacy").expect("legacy");
                    fs::write(&toml, b"toml").expect("toml");
                }
                4 => {
                    fs::write(&legacy, b"same inode").expect("legacy");
                    fs::hard_link(&legacy, &toml).expect("same identity hard link");
                }
                5 => {
                    fs::write(&toml, b"symlink target").expect("toml");
                    symlink(filename.toml(), &legacy).expect("legacy symlink");
                }
                6 => {
                    fs::create_dir(&legacy).expect("openable legacy directory");
                    fs::create_dir(&toml).expect("openable toml directory");
                }
                7 => {
                    // Missing legacy, nonregular followed fallback metadata.
                    symlink("/dev/null", &toml).expect("character-device fallback");
                }
                8 => {
                    symlink("/dev/null", &legacy).expect("openable character-device legacy");
                    symlink("absent", &toml).expect("dangling toml");
                }
                _ => unreachable!(),
            }
            compare(&directory, filename);
        }
    }
    let nondirectory = fixture.0.join("not-a-directory");
    fs::write(&nondirectory, b"file").expect("nondirectory fixture");
    compare(&nondirectory, Filename::Config);
    compare(&fixture.0.join("missing-parent"), Filename::Config);
    let native = fixture.0.join(std::ffi::OsStr::from_bytes(b"native-\xff"));
    match fs::create_dir(&native) {
        Ok(()) => fs::write(native.join("config"), b"native directory").expect("native config"),
        Err(error)
            if matches!(
                error.raw_os_error(),
                Some(nix::libc::EPERM | nix::libc::EILSEQ)
            ) =>
        {
            eprintln!("non-UTF8 discovery directory creation was refused; native error path exercised, existing-directory success NOT covered");
        }
        Err(error) => panic!("native directory fixture: {error}"),
    }
    compare(&native, Filename::Config);
}
