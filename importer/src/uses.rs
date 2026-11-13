//! What a generated crate's native code uses of its marked modules (D935.3,
//! D936.5), as rawr's own workspace wrapper records it beside the unit's
//! artifact when it checks the crate, next to its instances
//! (`crate::record`). A save that changes what native code can see of a
//! marked module needs a new native build only for a change one of these
//! uses reaches, so the record goes with the runner built from the check,
//! and a save is judged against the binary that runs.
//!
//! Each item is named by its path from the crate root, as the shadow
//! crate's shape names it (`physics::Counter`, `physics::Counter::new`).
//! The driver writes the record ([`Recorder::finish`], [`Uses::write`]);
//! rawr reads it ([`Uses::read`]).
//!
//! The driver and the library both compile this file.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The record's layout. A record of another is read as none.
pub const SCHEMA: u32 = 1;

/// The extension of a record beside its unit's `.rmeta`.
const EXTENSION: &str = "rawr-uses";

/// One check's uses of the marked modules by native code.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Uses {
    schema: u32,
    /// The marked items native code names: functions, constants, types,
    /// traits and modules, and inherent members as `physics::Counter::new`.
    named: BTreeSet<String>,
    /// The marked types native code holds a value of, or whose layout a
    /// held type or a named function's signature reaches: native code is
    /// built against their fields, their drop and their trait impls.
    held: BTreeSet<String>,
    /// Each member name native code looked up on a marked type, by a method
    /// call or a type-relative path, as `physics::Counter::describe`,
    /// whatever it found: a new member of that name may be found instead.
    probed: BTreeSet<String>,
    /// Each name native code resolved in a scope that glob-imports a marked
    /// module, as that module's member, `physics::Result`, whether the
    /// glob, the prelude or nothing gave it: a new item of that name in the
    /// module shadows what native code found.
    scoped: BTreeSet<String>,
    /// The traits native code holds trait objects of, by name, which a
    /// marked type's stand-in implements (`Page` of `dyn page::Page`).
    objects: BTreeSet<String>,
    /// Every trait native code names, calls a method of or bounds a type
    /// by, of any crate, by name (`Clone`, `Display`): a new impl of one
    /// for a type native code holds can change what rustc selects.
    traits: BTreeSet<String>,
}

/// The marked functions whose trampolines rawr wrote a generated crate
/// with bodies for (#1807): every one, before any check recorded what
/// native code names, or those named, with the marked types it declared
/// as the handle of the program value (D961).
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Bodies {
    Every,
    Named {
        named: BTreeSet<String>,
        opaque: BTreeSet<String>,
    },
}

impl Bodies {
    /// Whether rawr writes the crate again for `uses`, a check's of it:
    /// with the bodies of the functions it names alone, or with one it
    /// names that the crate lacks the body of, or with the types it holds
    /// but never opens when they changed.
    #[must_use]
    pub fn rewritten(&self, uses: &Uses) -> bool {
        match self {
            Self::Every => true,
            Self::Named { named, opaque } => {
                !named.is_superset(&uses.named) || *opaque != uses.opaque()
            }
        }
    }
}

/// The uses one check finds, as it finds them.
#[derive(Default)]
pub struct Recorder {
    uses: Uses,
}

impl Recorder {
    /// Native code names the marked item at `path`.
    pub fn name(&mut self, path: String) {
        self.uses.named.insert(path);
    }

    /// Native code holds, or is built against, the marked type at `path`.
    /// Whether it was new.
    pub fn hold(&mut self, path: String) -> bool {
        self.uses.held.insert(path)
    }

    /// Native code looked up `member` on the marked type at `owner`.
    pub fn probe(&mut self, owner: &str, member: &str) {
        self.uses.probed.insert(format!("{owner}::{member}"));
    }

    /// Native code resolved `name` in a scope that glob-imports the marked
    /// module at `module`.
    pub fn scope(&mut self, module: &str, name: &str) {
        let path = if module.is_empty() {
            name.to_owned()
        } else {
            format!("{module}::{name}")
        };
        self.uses.scoped.insert(path);
    }

    /// Native code holds trait objects of the trait named `name`.
    pub fn object(&mut self, name: String) {
        self.uses.objects.insert(name);
    }

    /// Native code names, calls or bounds by the trait named `name`.
    pub fn trait_(&mut self, name: String) {
        self.uses.traits.insert(name);
    }

    /// The record of what was found.
    #[must_use]
    pub fn finish(mut self) -> Uses {
        self.uses.schema = SCHEMA;
        self.uses
    }
}

impl Uses {
    /// The marked items native code names, by their paths.
    #[must_use]
    pub fn named(&self) -> &BTreeSet<String> {
        &self.named
    }

    /// Whether native code names the marked item at `path`.
    #[must_use]
    pub fn names(&self, path: &str) -> bool {
        self.named.contains(path)
    }

    /// Whether native code names something inside the marked item at
    /// `path`, as a module's item or a type's member.
    #[must_use]
    pub fn names_within(&self, path: &str) -> bool {
        self.named
            .iter()
            .chain(&self.held)
            .any(|named| within(named, path))
    }

    /// Whether native code is built against the marked type at `path`.
    #[must_use]
    pub fn holds(&self, path: &str) -> bool {
        self.held.contains(path)
    }

    /// The marked types native code holds, which rawr's copy of the crate
    /// may declare as the handle of the program value when native code
    /// can't open them (D961).
    #[must_use]
    pub fn opaque(&self) -> BTreeSet<String> {
        self.held.clone()
    }

    /// The held type that `path` is a member of (`physics::Counter` of
    /// `physics::Counter::Default::default` or of `physics::Counter as dyn
    /// page::Page`), when there is one.
    #[must_use]
    pub fn holder(&self, path: &str) -> Option<&str> {
        self.held
            .iter()
            .filter(|held| member(path, held))
            .max_by_key(|held| held.len())
            .map(String::as_str)
    }

    /// Whether native code looked up `member` on the marked type at
    /// `owner`.
    #[must_use]
    pub fn probes(&self, owner: &str, member: &str) -> bool {
        self.probed.contains(&format!("{owner}::{member}"))
    }

    /// Whether native code resolved the name `path`'s last segment names in
    /// a scope that glob-imports the module `path` is in.
    #[must_use]
    pub fn scopes(&self, path: &str) -> bool {
        self.scoped.contains(path)
    }

    /// Whether native code globs the marked module at `module`, so any
    /// name it gains may shadow one native code resolved.
    #[must_use]
    pub fn globs(&self, module: &str) -> bool {
        self.scoped.iter().any(|scoped| {
            scoped
                .rsplit_once("::")
                .map_or(module.is_empty(), |(owner, _)| owner == module)
        })
    }

    /// Whether native code holds trait objects of a trait named `name`.
    #[must_use]
    pub fn objects(&self, name: &str) -> bool {
        self.objects.contains(name)
    }

    /// Whether native code names, calls or bounds by a trait named `name`.
    #[must_use]
    pub fn uses_trait(&self, name: &str) -> bool {
        self.traits.contains(name)
    }

    /// These uses with `other`'s, as two generated crates' checks of one
    /// program record them.
    #[must_use]
    pub fn with(mut self, other: Self) -> Self {
        self.schema = SCHEMA;
        self.named.extend(other.named);
        self.held.extend(other.held);
        self.probed.extend(other.probed);
        self.scoped.extend(other.scoped);
        self.objects.extend(other.objects);
        self.traits.extend(other.traits);
        self
    }

    /// The uses the generated crates among `artifacts` recorded, together:
    /// a generated crate's check writes its instances (`crate::record`)
    /// beside its artifact, and its uses with them. `None` when there is no
    /// generated crate, or one has no record of its uses, as a check by an
    /// older driver has none.
    ///
    /// # Errors
    ///
    /// A record can't be read.
    pub fn recorded<'a>(artifacts: impl IntoIterator<Item = &'a Path>) -> io::Result<Option<Self>> {
        let mut recorded: Option<Self> = None;
        for artifact in artifacts {
            if !crate::record::Record::beside(artifact).is_file() {
                continue;
            }
            let Some(uses) = Self::read(artifact)? else {
                return Ok(None);
            };
            recorded = Some(match recorded {
                Some(earlier) => earlier.with(uses),
                None => uses,
            });
        }
        Ok(recorded)
    }

    /// Where the record of the unit whose artifact is `artifact` is.
    #[must_use]
    pub fn beside(artifact: &Path) -> PathBuf {
        artifact.with_extension(EXTENSION)
    }

    /// The record beside `artifact`: `None` when there is none, or it is
    /// of another layout.
    ///
    /// # Errors
    ///
    /// The record can't be read, or isn't a record.
    pub fn read(artifact: &Path) -> io::Result<Option<Self>> {
        let bytes = match fs::read(Self::beside(artifact)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let Ok(uses) = serde_json::from_slice::<Self>(&bytes) else {
            return Ok(None);
        };
        Ok((uses.schema == SCHEMA).then_some(uses))
    }

    /// Write the record beside `artifact`, replacing the one there.
    ///
    /// # Errors
    ///
    /// The record can't be written.
    pub fn write(&self, artifact: &Path) -> io::Result<()> {
        let bytes = serde_json::to_vec(self).map_err(io::Error::other)?;
        fs::write(Self::beside(artifact), bytes)
    }
}

/// Whether `path` is a member of the item at `owner`: its impl's function
/// (`physics::Ball::fall`), a trait impl's (`physics::Ball::Default::default`)
/// or a stand-in for it (`physics::Ball as dyn page::Page`).
fn member(path: &str, owner: &str) -> bool {
    path.strip_prefix(owner)
        .is_some_and(|rest| rest.starts_with("::") || rest.starts_with(' '))
}

/// Whether `path` is inside the item at `outer`, or is it.
fn within(path: &str, outer: &str) -> bool {
    path == outer || member(path, outer)
}

#[cfg(test)]
mod tests {
    use super::{Recorder, Uses};

    #[test]
    fn a_record_answers_for_what_its_check_found() {
        let mut recorder = Recorder::default();
        recorder.name("physics::label".to_owned());
        recorder.hold("physics::Counter".to_owned());
        recorder.probe("physics::Counter", "step");
        recorder.scope("physics", "Result");
        recorder.object("Page".to_owned());
        recorder.trait_("Clone".to_owned());
        let uses = recorder.finish();
        assert!(uses.names("physics::label"));
        assert!(!uses.names("physics::inset"));
        assert!(uses.names_within("physics"));
        assert!(!uses.names_within("theme"));
        assert_eq!(
            uses.holder("physics::Counter::Default::default"),
            Some("physics::Counter")
        );
        assert_eq!(uses.holder("physics::Counters"), None);
        assert!(uses.probes("physics::Counter", "step"));
        assert!(uses.scopes("physics::Result"));
        assert!(uses.globs("physics"));
        assert!(!uses.globs("theme"));
        assert!(uses.objects("Page"));
        assert!(uses.uses_trait("Clone") && !uses.uses_trait("Debug"));
        let other = Uses::default().with(uses.clone());
        assert!(other.names("physics::label"));
    }

    /// A crate written with every body is written again for its first
    /// record, and one written with named bodies only for a name it lacks
    /// or for a change to the types native code holds.
    #[test]
    fn bodies_are_written_again_only_for_a_name_they_lack() {
        use super::Bodies;
        let mut recorder = Recorder::default();
        recorder.name("physics::label".to_owned());
        recorder.hold("physics::Counter".to_owned());
        let uses = recorder.finish();
        let paths = |paths: &[&str]| paths.iter().map(|path| (*path).to_owned()).collect();
        let named = |names: &[&str]| Bodies::Named {
            named: paths(names),
            opaque: paths(&["physics::Counter"]),
        };
        assert_eq!(uses.opaque(), paths(&["physics::Counter"]));
        assert!(Bodies::Every.rewritten(&uses));
        assert!(named(&[]).rewritten(&uses));
        assert!(!named(&["physics::label"]).rewritten(&uses));
        assert!(!named(&["physics::label", "physics::inset"]).rewritten(&uses));
        let opened = Bodies::Named {
            named: paths(&["physics::label"]),
            opaque: paths(&["physics::Counter", "physics::Ball"]),
        };
        assert!(opened.rewritten(&uses));
    }
}
