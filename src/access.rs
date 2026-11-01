//! What a program's own code may reach outside the engine: the files it
//! may read and write, the environment, the process and the network
//! (D683.4). An [`Engine`](crate::Engine) starts with none of it, as a
//! WebAssembly module under WASI starts with no preopened directory. The
//! embedder grants each with [`Engine::set_access`](crate::Engine::set_access),
//! and a file grant may name directories, as Deno's `--allow-read=dir` does.
//!
//! Every std function that reaches the operating system consults it at the
//! call: `std::fs`, `std::env`, `std::process`, and the compile-time
//! `include_str!`, `include_bytes!`, `env!` and `option_env!`. A call
//! without its grant fails as the operating system would refuse it: an
//! `io::Error` of kind `PermissionDenied`, an undefined variable, a panic.
//!
//! This gates the program's own code only. A native crate the program
//! imports calls the operating system directly, as it would under Cargo.

use std::path::{Component, Path, PathBuf};

/// The files, environment, process and network a program may reach.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Access {
    read: Paths,
    write: Paths,
    env: bool,
    process: bool,
    net: bool,
}

/// The files one kind of file access reaches.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Paths {
    #[default]
    None,
    All,
    /// Each directory, with everything under it, canonical.
    Within(Vec<PathBuf>),
}

/// No access, borrowed by a call that has no program.
pub(crate) static NONE: Access = Access::none();

impl Access {
    /// No access: what every engine starts with.
    pub const NONE: Self = Self::none();

    /// No access: what every engine starts with.
    #[must_use]
    pub const fn none() -> Self {
        Self {
            read: Paths::None,
            write: Paths::None,
            env: false,
            process: false,
            net: false,
        }
    }

    /// Everything, as a program built by Cargo has: what `rawr run` and
    /// `rawr check` grant.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            read: Paths::All,
            write: Paths::All,
            env: true,
            process: true,
            net: true,
        }
    }

    /// Also let the program read every file.
    #[must_use]
    pub fn allow_read_all(mut self) -> Self {
        self.read = Paths::All;
        self
    }

    /// Also let the program write every file.
    #[must_use]
    pub fn allow_write_all(mut self) -> Self {
        self.write = Paths::All;
        self
    }

    /// Also let the program read the files under `directory`.
    #[must_use]
    pub fn allow_read(mut self, directory: impl AsRef<Path>) -> Self {
        self.read.add(directory.as_ref());
        self
    }

    /// Also let the program create, write and remove the files under
    /// `directory`.
    #[must_use]
    pub fn allow_write(mut self, directory: impl AsRef<Path>) -> Self {
        self.write.add(directory.as_ref());
        self
    }

    /// Also let the program read the environment: its variables and its
    /// arguments.
    #[must_use]
    pub const fn allow_env(mut self) -> Self {
        self.env = true;
        self
    }

    /// Also let the program reach its process.
    #[must_use]
    pub const fn allow_process(mut self) -> Self {
        self.process = true;
        self
    }

    /// Also let the program reach the network.
    #[must_use]
    pub const fn allow_net(mut self) -> Self {
        self.net = true;
        self
    }

    /// `Ok` when the program may read `path`, and otherwise the error the
    /// read fails with.
    pub fn read(&self, path: &Path) -> std::io::Result<()> {
        self.read.permit(path, "read")
    }

    /// `Ok` when the program may create, write or remove `path`, and
    /// otherwise the error the call fails with.
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        self.write.permit(path, "write")
    }

    /// Whether the program may read the environment.
    #[must_use]
    pub const fn env(&self) -> bool {
        self.env
    }

    /// Whether the program may reach its process.
    #[must_use]
    pub const fn process(&self) -> bool {
        self.process
    }

    /// Whether the program may reach the network.
    #[must_use]
    pub const fn net(&self) -> bool {
        self.net
    }
}

impl Paths {
    fn add(&mut self, directory: &Path) {
        let directory = resolved(directory).unwrap_or_else(|| absolute(directory));
        match self {
            Self::All => {}
            Self::None => *self = Self::Within(vec![directory]),
            Self::Within(directories) => directories.push(directory),
        }
    }

    fn permit(&self, path: &Path, operation: &str) -> std::io::Result<()> {
        let permitted = match self {
            Self::None => false,
            Self::All => true,
            Self::Within(directories) => resolved(path).is_some_and(|path| {
                directories
                    .iter()
                    .any(|directory| path.starts_with(directory))
            }),
        };
        if permitted {
            return Ok(());
        }
        // As the operating system's own refusal, it names no path: the
        // call that fails names it.
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!("permission denied: this program may not {operation} this path"),
        ))
    }
}

/// `path` made absolute against the working directory, as the operating
/// system reads a relative path.
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Where `path` leads once symbolic links and `..` are followed: its
/// nearest existing ancestor made canonical, with the rest of it, which
/// may not climb out with `..`. `None` when no ancestor exists or the rest
/// climbs.
fn resolved(path: &Path) -> Option<PathBuf> {
    let path = absolute(path);
    let mut rest = Vec::new();
    let mut ancestor = path.as_path();
    loop {
        if let Ok(canonical) = ancestor.canonicalize() {
            let mut resolved = canonical;
            for component in rest.iter().rev() {
                resolved.push(component);
            }
            return Some(resolved);
        }
        let name = ancestor.components().next_back()?;
        if !matches!(name, Component::Normal(_)) {
            return None;
        }
        rest.push(name.as_os_str().to_os_string());
        ancestor = ancestor.parent()?;
    }
}

#[cfg(test)]
mod tests {
    use super::Access;
    use std::io::ErrorKind;

    #[test]
    fn a_scoped_grant_reaches_only_its_directory() {
        let root = std::env::temp_dir().join(format!("rawr-access-{}", std::process::id()));
        let inside = root.join("inside");
        std::fs::create_dir_all(&inside).unwrap();
        let access = Access::none().allow_read(&inside).allow_write(&inside);

        assert!(access.read(&inside.join("new.txt")).is_ok());
        assert!(access.write(&inside.join("deep/new/file.txt")).is_ok());
        for outside in [
            root.join("other.txt"),
            inside.join("../other.txt"),
            inside.join("missing/../../other.txt"),
        ] {
            let error = access.read(&outside).unwrap_err();
            assert_eq!(error.kind(), ErrorKind::PermissionDenied, "{outside:?}");
        }
        assert_eq!(
            Access::NONE.read(&inside).unwrap_err().kind(),
            ErrorKind::PermissionDenied
        );
        assert!(Access::all().write(&root).is_ok());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
