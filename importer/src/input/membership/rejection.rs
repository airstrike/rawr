//! Exact-call-site classification, never a subsystem-wide exemption.
//!
//! The full cold command still executes every operation. At replay, shared
//! inputs remain required wherever selection, archive assembly, source contents,
//! or the enclosing native proof consumes them. This ledger classifies uses,
//! not filenames or libraries.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Operation {
    SourceDiscovery,
    SourceIndexAndIgnore,
    SourceTraversal,
    ArchiveAssembly,
    ArchiveLockfileValue,
    VcsDiscoveryAndManifestGate,
    VcsSymlinkWarning,
    VcsStatusConfiguration,
    VcsStatusCollection,
    VcsSubmoduleStatus,
    VcsOutsideRootStatus,
    VcsDirtyIntersection,
    VcsHeadPeeling,
    VcsHeadValue,
    VcsPathFormatting,
    ArbitraryFilter,
    ArchiveDiagnostics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Treatment {
    Retained,
    RetainedForArchiveOccupancy,
    VirtualComputation,
    ResidualProcessTrust,
}

pub(super) struct Classification {
    pub(super) treatment: Treatment,
    pub(super) source: &'static str,
    pub(super) argument: &'static str,
}

pub(super) const fn classify(operation: Operation) -> Classification {
    match operation {
        Operation::SourceDiscovery => Classification {
            treatment: Treatment::Retained,
            source: "cargo/sources/path.rs:641-680",
            argument: "Discovery, canonical prefix, bare-repository rejection and tracked-manifest index lookup choose the physical source walker; their exact failure and fallback branches remain.",
        },
        Operation::SourceIndexAndIgnore => Classification {
            treatment: Treatment::Retained,
            source: "cargo/sources/path.rs:689-850; gix-worktree-0.54.0/src/stack/state/ignore.rs:164-233",
            argument: "Configuration, index constituents, ignore-file absence and requested object-backed ignore data affect physical selection. Any overlap with VCS use remains retained here.",
        },
        Operation::SourceTraversal => Classification {
            treatment: Treatment::Retained,
            source: "cargo/sources/path.rs:570-635,689-957",
            argument: "Include/exclude, nested-package/repository pruning, tracked target entries and symlink traversal determine membership. Every required frontier and branch remains.",
        },
        Operation::ArchiveAssembly => Classification {
            treatment: Treatment::Retained,
            source: "cargo/ops/cargo_package/mod.rs:497-687,1091-1117",
            argument: "Filename/reserved-name rejection, manifest-origin replacement, README/license precedence and build-script source validation remain, including required positive and negative probes.",
        },
        Operation::ArchiveLockfileValue => Classification {
            treatment: Treatment::VirtualComputation,
            source: "cargo/ops/cargo_package/mod.rs:555-571",
            argument: "Once include_lockfile is true, the Cargo.lock row is generated unconditionally. lockfile_path.exists() selects only Some(path) versus None inside GeneratedFile::Lockfile; neither Human output nor metadata-file collision precedence consumes that optional value. Lockfile bytes, source resolution and independent manifest/configuration dependencies remain required wherever their own proof consumes them.",
        },
        Operation::VcsDiscoveryAndManifestGate => Classification {
            treatment: Treatment::RetainedForArchiveOccupancy,
            source: "cargo/ops/cargo_package/vcs.rs:51-107; cargo/ops/cargo_package/mod.rs:488-492,573-583",
            argument: "This second discovery/open/index/excludes invocation gates Option<VcsInfo>. Generated(VcsInfo) occupancy suppresses a metadata file whose archive path is .cargo_vcs_info.json, either an external basename or an excluded root-relative file. Retain this availability closure whenever that physical branch consumes it; earlier source discovery and selected-file content remain independently retained.",
        },
        Operation::VcsSymlinkWarning => Classification {
            treatment: Treatment::VirtualComputation,
            source: "cargo/ops/cargo_package/vcs.rs:108,151-184",
            argument: "This core.symlinks read and maybe_plain_text_symlink predicate feed a warning report only. They do not rewrite PathEntry or archive inputs. Source-walk symlink interpretation and physical symlink contents remain required.",
        },
        Operation::VcsStatusConfiguration => Classification {
            treatment: Treatment::VirtualComputation,
            source: "cargo/ops/cargo_package/vcs.rs:185-190",
            argument: "The in-memory index.threads=false mutation configures only this VCS repository's status work; it is not a disk/configuration mutation. It grants no exemption to config consumed by source discovery or walking.",
        },
        Operation::VcsStatusCollection => Classification {
            treatment: Treatment::VirtualComputation,
            source: "cargo/ops/cargo_package/vcs.rs:198-203,268-311",
            argument: "Statuses, tree/index comparisons, built-in attribute conversions and their pure errors flow only into dirty_files, then the dirty bit. Arbitrary filter execution is separately residual trust. Files/config/index/attributes/objects independently selected or consumed by physical selection remain required.",
        },
        Operation::VcsSubmoduleStatus => Classification {
            treatment: Treatment::VirtualComputation,
            source: "cargo/ops/cargo_package/vcs.rs:206,315-331",
            argument: "This recursion only appends dirty paths used in VCS metadata. It does not supply src_files. Nested repositories, gitlinks, .gitmodules and their inputs remain wherever the physical walker consumes or selects them.",
        },
        Operation::VcsOutsideRootStatus => Classification {
            treatment: Treatment::VirtualComputation,
            source: "cargo/ops/cargo_package/vcs.rs:230,365-433",
            argument: "Canonicalization and status of README/license/symlink/workspace-manifest paths here produce dirty paths only. The separate physical origin, bytes, archive probes and enclosing manifest proof for those same paths remain mandatory.",
        },
        Operation::VcsDirtyIntersection => Classification {
            treatment: Treatment::VirtualComputation,
            source: "cargo/ops/cargo_package/vcs.rs:211-240,245-258",
            argument: "The src_files intersection is read-only. With the exact governed allow_dirty=true flag, successful dirty changes only GitVcsInfo.dirty and cannot take the dirty-source rejection branch. No arbitrary side-effect noninterference is claimed.",
        },
        Operation::VcsHeadPeeling => Classification {
            treatment: Treatment::RetainedForArchiveOccupancy,
            source: "cargo/ops/cargo_package/vcs.rs:241-246; cargo/ops/cargo_package/mod.rs:573-583,664-687",
            argument: "HEAD peeling yields Option<Id>, whose presence controls Generated(VcsInfo) occupancy. Retain the optionality and required deterministic dependencies when collision precedence can suppress a physical .cargo_vcs_info.json metadata input, including excluded root-relative files. Other discovery/config/object/source-OID dependencies remain independently retained.",
        },
        Operation::VcsHeadValue => Classification {
            treatment: Treatment::VirtualComputation,
            source: "cargo/ops/cargo_package/vcs.rs:241-246; cargo/ops/cargo_package/mod.rs:573-583",
            argument: "Once presence is established where needed, the successful HEAD object's hex value populates only GitVcsInfo.sha1. That value is separate from the retained optionality and from independently required Git source/OID identity.",
        },
        Operation::VcsPathFormatting => Classification {
            treatment: Treatment::VirtualComputation,
            source: "cargo/ops/cargo_package/vcs.rs:129-135",
            argument: "The formatted path_in_vcs string appears only in Generated(VcsInfo); physical origins use the separately retained source paths.",
        },
        Operation::ArbitraryFilter => Classification {
            treatment: Treatment::ResidualProcessTrust,
            source: "gix-filter-0.32.0/src/driver/init.rs:32-110; src/driver/apply.rs:111-130,305-318",
            argument: "Clean/process execution, protocol, outcome and hypothetical undeclared effects are not executed or predicted. Residual process trust, not noninterference, permits exclusion. Every observable retained before/after or current-source change rejects.",
        },
        Operation::ArchiveDiagnostics => Classification {
            treatment: Treatment::VirtualComputation,
            source: "cargo/ops/cargo_package/mod.rs:547-553,664-677,1109-1115",
            argument: "Warning formatting/writing for absent canonical-case manifest, external basename collision and Windows reserved names does not choose physical rows. The underlying manifest replacement, collision precedence and filename errors remain retained.",
        },
    }
}
