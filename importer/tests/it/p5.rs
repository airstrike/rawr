#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::classify::representability::Representability;
use rawr_import::classify::Policy;
use rawr_import::extract;
use rawr_import::process::Native;
use rawr_import::profile;
use rawr_import::surface::{Origin, Snapshot};

const ENTRY: &str = "application.rawr";

/// A P5 project. A DAG project carries the shared node context the product
/// always has, which hot admission requires, and keeps its snapshot on disk.
pub struct Project(
    rawr_import::store::TestProject,
    Option<rawr_import::node::Chain>,
);

impl Project {
    pub fn new(label: &str) -> Self {
        Self(
            rawr_import::store::TestProject::new(&format!("rawr-p5-{label}")),
            None,
        )
    }

    pub fn dag(label: &str) -> Self {
        let project = rawr_import::store::TestProject::new(&format!("rawr-p5-{label}"));
        rawr_import::store::isolate_for_tests();
        let context = rawr_import::context::Context::capture(
            project.path(),
            Environment::capture().expect("the P5 DAG environment captures"),
            "aarch64-apple-darwin",
            &rawr_import::cargo::Limits::standard(),
        )
        .expect("the P5 DAG context captures");
        let chain =
            rawr_import::node::Chain::fixture(context).expect("the P5 DAG parent validates");
        Self(project, Some(chain))
    }

    pub fn path(&self) -> &Path {
        self.0.path()
    }

    /// The snapshot entry: on disk for a DAG project, as the supervisor
    /// revalidates it there.
    fn entry(&self, source: &[u8]) -> String {
        if self.1.is_none() {
            return ENTRY.to_owned();
        }
        let path = self.path().join(ENTRY);
        fs::write(&path, source).expect("the P5 snapshot source writes");
        path.to_str()
            .expect("the P5 snapshot path is Unicode")
            .to_owned()
    }
}

#[derive(Clone, Copy)]
pub struct Checked([u8; 32]);

impl profile::Checked for Checked {
    fn profile_fingerprint(&self) -> [u8; 32] {
        self.0
    }

    // A hooked profile carries the 32-byte application semantic.
    fn profile_payload(&self) -> &[u8] {
        &self.0
    }
}

#[derive(Clone, Copy)]
pub enum Mode {
    Success,
    FailActivate,
    Exclusive,
}

pub struct Definition {
    identity: profile::Identity,
    version: profile::Version,
    mode: Mode,
    lease: Option<PathBuf>,
}

impl Definition {
    pub fn new(mode: Mode, lease: Option<PathBuf>, limits: &rawr_import::cargo::Limits) -> Self {
        Self {
            identity: profile::Identity::new("rawr.test.profile-clock", limits)
                .expect("the synthetic identity is bounded"),
            version: profile::Version::new(1).expect("the synthetic version is nonzero"),
            mode,
            lease,
        }
    }
}

impl profile::Definition<Checked> for Definition {
    fn identity(&self) -> profile::Identity {
        self.identity
    }

    fn version(&self) -> profile::Version {
        self.version
    }

    fn classify(
        &self,
        graph: &rawr_import::api::Graph<rawr_import::api::Extracted>,
        output: &mut profile::contract::Builder<'_>,
    ) -> Result<(), profile::Error> {
        let anchor = graph
            .definitions()
            .find(|definition| {
                matches!(
                    definition.kind(),
                    rawr_import::api::model::Kind::Struct { .. }
                ) && definition.display_path().last() == Some("Clock")
            })
            .ok_or(profile::Error::ForeignEvidence)?;
        output.definition(anchor)?;
        Ok(())
    }

    fn demand(
        &self,
        contract: &profile::Contract,
        _checked: &Checked,
        output: &mut profile::demand::Builder<'_>,
    ) -> Result<(), profile::Error> {
        let evidence = contract
            .evidence()
            .next()
            .copied()
            .ok_or(profile::Error::ForeignEvidence)?;
        output.hook(evidence, "runner-entry")?;
        Ok(())
    }

    fn generate(
        &self,
        demand: &profile::Demand,
        output: &mut profile::demand::Generator<'_>,
    ) -> Result<(), profile::Error> {
        let hook = demand
            .hooks()
            .next()
            .copied()
            .ok_or(profile::Error::ForeignEvidence)?;
        let source = match self.mode {
            Mode::Success => success_source(),
            Mode::FailActivate => failure_source(),
            Mode::Exclusive => exclusive_source(
                self.lease
                    .as_deref()
                    .ok_or(profile::Error::ForeignEvidence)?,
            ),
        };
        output.projection(hook, source.as_bytes())
    }
}

pub fn profile_contract(
    project: &Project,
    mode: Mode,
    lease: Option<PathBuf>,
) -> (Definition, profile::Contract, rawr_import::cargo::Limits) {
    let limits = rawr_import::cargo::Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let graph = request(project.path(), &limits)
        .resolve(&native, &limits)
        .and_then(|resolution| extract::from_resolution(resolution, &native, &limits))
        .expect("real profile anchor evidence canonicalizes");
    let definition = Definition::new(mode, lease, &limits);
    let contract = profile::classify(&definition, &graph, &limits)
        .expect("the exact synthetic profile contract seals");
    (definition, contract, limits)
}

pub fn generated(
    project: &Project,
    mode: Mode,
    lease: Option<PathBuf>,
    source: &[u8],
    revision: rawr_import::supervisor::Revision,
) -> (
    rawr_import::runner::Manifest<rawr_import::runner::Generated, ()>,
    rawr_import::supervisor::snapshot::Bytes,
) {
    let (request, snapshot) = generation_request(project, mode, lease, source, revision);
    let manifest = request
        .generate(&rawr_import::cargo::Limits::standard())
        .expect("the profile runner files generate");
    (manifest, snapshot)
}

pub fn generation_request(
    project: &Project,
    mode: Mode,
    lease: Option<PathBuf>,
    source: &[u8],
    revision: rawr_import::supervisor::Revision,
) -> (
    rawr_import::bridge::GenerationRequestKey<()>,
    rawr_import::supervisor::snapshot::Bytes,
) {
    let limits = rawr_import::cargo::Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let graph = request(project.path(), &limits)
        .resolve(&native, &limits)
        .and_then(|resolution| extract::from_resolution(resolution, &native, &limits))
        .expect("real profile anchor evidence canonicalizes");
    let definition = Definition::new(mode, lease, &limits);
    let profile_contract = profile::classify(&definition, &graph, &limits)
        .expect("the exact synthetic profile contract seals");
    let contract = graph
        .classify(&Policy::standard(), &limits)
        .expect("the profile anchor graph classifies");
    let entry = project.entry(source);
    let snapshot = snapshot(&limits, &entry, source);
    let mut compiler = contract
        .compiler((), snapshot, &limits)
        .expect("the synthetic demand compiler starts");
    let mut position = 0_u32;
    for outcome in contract.outcomes() {
        require_outcome(&mut compiler, outcome, &entry, &limits, &mut position);
        if let Representability::Struct(plan) = outcome.representability()
            && let Some(selector) = plan.constructor()
        {
            compiler
                .require_selected(
                    outcome.identity(),
                    selector
                        .try_clone()
                        .expect("the anchor constructor selector duplicates"),
                    &entry,
                    position,
                    position + 1,
                )
                .expect("the anchor constructor is demanded");
            position += 1;
        }
        for implementation in outcome.implementations() {
            for associated in implementation.associated() {
                require_outcome(&mut compiler, associated, &entry, &limits, &mut position);
            }
        }
    }
    let demand = compiler.seal().expect("the synthetic surface demand seals");
    let native = contract.into_native_proof();
    let checked = Checked([0x5a; 32]);
    let profile_demand = profile::demand(&definition, &profile_contract, &checked, &limits)
        .expect("the checked profile demand seals");
    let profile = profile::Proof::generate::<Checked, _>(&definition, &profile_demand, &limits)
        .expect("the demanded profile projections generate");
    let profile = match &project.1 {
        Some(chain) => profile.with_chain(chain.clone()),
        None => profile,
    };
    let request = demand
        .request(native, profile, revision)
        .expect("the profile generation request seals");
    (request, snapshot_transport(&limits, &entry, source))
}

fn require_outcome<B>(
    compiler: &mut rawr_import::surface::Compiler<'_, B>,
    outcome: rawr_import::classify::Outcome<'_>,
    entry: &str,
    limits: &rawr_import::cargo::Limits,
    position: &mut u32,
) {
    if !outcome.representability().is_demandable() {
        return;
    }
    compiler
        .require(
            outcome.identity().clone(),
            Origin::checked(entry, *position, *position + 1, limits)
                .expect("the synthetic Rawr origin is bounded"),
        )
        .expect("the synthetic outcome is demanded");
    *position += 1;
}

fn snapshot(limits: &rawr_import::cargo::Limits, entry: &str, source: &[u8]) -> Snapshot {
    let mut snapshot = Snapshot::builder(limits);
    snapshot.entry(entry).expect("the entry path is bounded");
    snapshot
        .file(entry, source)
        .expect("the synthetic source is bounded");
    snapshot.seal().expect("the synthetic snapshot seals")
}

fn snapshot_transport(
    limits: &rawr_import::cargo::Limits,
    entry: &str,
    source: &[u8],
) -> rawr_import::supervisor::snapshot::Bytes {
    let expected = snapshot(limits, entry, source);
    let mut capture = rawr_import::supervisor::snapshot::Bytes::capture(limits);
    capture
        .entry(entry)
        .expect("the transport entry is bounded");
    capture
        .file(entry, source)
        .expect("the transport source is bounded");
    capture
        .bind(expected, limits)
        .expect("the profile transport matches its snapshot")
}

fn request(root: &Path, limits: &rawr_import::cargo::Limits) -> rawr_import::cargo::Request {
    rawr_import::store::isolate_for_tests();
    let environment = Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .expect("the P5 environment is sealable");
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let importer = if manifest.ends_with("importer") {
        manifest
    } else {
        manifest.join("../importer")
    };
    let fixture = fs::canonicalize(importer.join("tests/fixtures/profile-clock"))
        .expect("the unchanged profile fixture exists");
    rawr_import::cargo::Request::builder(root, &root.join(".rawr/cache"), limits)
        .expect("the P5 request builder seals")
        .environment(environment)
        .policy(extract::Policy::manifest(limits).seal())
        .expect("the manifest policy fits")
        .dependency(
            Dependency::path("clock", &fixture, limits)
                .and_then(|dependency| dependency.package("profile-clock"))
                .expect("the profile anchor alias is exact"),
        )
        .expect("the profile dependency fits")
        .build()
        .expect("the P5 request seals")
}

fn success_source() -> String {
    r#"mod __rawr_profile {
    #[derive(Clone)]
    pub struct ApplicationContract;
    pub fn compile_initial(
        engine: &__rawr_internal::Engine,
        snapshot: &__rawr_internal::package::snapshot::Snapshot,
        _contract: &ApplicationContract,
        _expected: &[u8; 32],
    ) -> std::io::Result<(Option<__rawr_internal::reload::Candidate>, __rawr_internal::AST)> {
        engine.compile_package_snapshot(snapshot)
            .map(|ast| (None, ast))
            .map_err(|error| std::io::Error::other(error.to_string()))
    }
    pub fn compile_generation(
        _engine: &__rawr_internal::Engine,
        _snapshot: &__rawr_internal::package::snapshot::Snapshot,
        _contract: &ApplicationContract,
        _expected: &[u8; 32],
    ) -> std::io::Result<Option<__rawr_internal::reload::Candidate>> { Ok(None) }
    pub fn bind_runtime(
        _definitions: &[__rawr_internal::host::facade::Definition],
        _builders: &mut [__rawr_internal::host::runtime::Builder],
    ) -> Result<(), &'static str> { Ok(()) }
    pub fn install_compiler_facades(
        engine: &mut __rawr_internal::Engine,
        native: &mut __rawr_internal::package::import::runner::account::Native,
    ) -> std::io::Result<(Box<[__rawr_internal::host::facade::Definition]>, ApplicationContract)> {
        if super::IMPORTED_FACADES.is_empty() { return Ok((Box::default(), ApplicationContract)); }
        __rawr_internal::package::import::runner::install_retained(engine, super::IMPORTED_FACADES, native)
            .map(|definitions| (definitions, ApplicationContract))
            .map_err(|error| std::io::Error::other(error.to_string()))
    }
    pub struct Runtime { stage: u8, messages: u64 }
    impl Runtime {
        pub fn new() -> Self { Self { stage: 0, messages: 0 } }
        pub fn prepare(&mut self) -> Result<(), &'static str> {
            let _anchor = clock::Clock;
            if self.stage != 0 { return Err("prepare order"); }
            self.stage = 1;
            Ok(())
        }
        pub fn activate(&mut self) -> Result<(), &'static str> {
            if self.stage != 1 { return Err("activate order"); }
            self.stage = 2;
            Ok(())
        }
        pub fn admit(&mut self, _program: Option<&mut super::Program>) -> Result<(), &'static str> {
            if self.stage != 2 { return Err("admit order"); }
            self.stage = 3;
            Ok(())
        }
        pub fn message(&mut self, input: &[u8]) -> Result<Vec<u8>, &'static str> {
            if self.stage != 3 { return Err("message before admission"); }
            let value = u64::from_le_bytes(input.try_into().map_err(|_| "message shape")?);
            self.messages = self.messages.checked_add(1).ok_or("message exhaustion")?;
            Ok(value.wrapping_add(self.messages).to_le_bytes().to_vec())
        }
        pub fn retire(&mut self) { self.stage = 4; }
    }
}"#
    .to_owned()
}

fn failure_source() -> String {
    success_source().replace(
        "self.stage = 2;\n            Ok(())",
        "return Err(\"exclusive resource unavailable\");",
    )
}

fn exclusive_source(path: &Path) -> String {
    let path = path
        .to_str()
        .expect("the temporary lease path is UTF-8")
        .escape_default()
        .to_string();
    format!(
        r#"mod __rawr_profile {{
    #[derive(Clone)]
    pub struct ApplicationContract;
    pub fn compile_initial(
        engine: &__rawr_internal::Engine,
        snapshot: &__rawr_internal::package::snapshot::Snapshot,
        _contract: &ApplicationContract,
        _expected: &[u8; 32],
    ) -> std::io::Result<(Option<__rawr_internal::reload::Candidate>, __rawr_internal::AST)> {{
        engine.compile_package_snapshot(snapshot)
            .map(|ast| (None, ast))
            .map_err(|error| std::io::Error::other(error.to_string()))
    }}
    pub fn compile_generation(
        _engine: &__rawr_internal::Engine,
        _snapshot: &__rawr_internal::package::snapshot::Snapshot,
        _contract: &ApplicationContract,
        _expected: &[u8; 32],
    ) -> std::io::Result<Option<__rawr_internal::reload::Candidate>> {{ Ok(None) }}
    pub fn bind_runtime(
        _definitions: &[__rawr_internal::host::facade::Definition],
        _builders: &mut [__rawr_internal::host::runtime::Builder],
    ) -> Result<(), &'static str> {{ Ok(()) }}
    pub fn install_compiler_facades(
        engine: &mut __rawr_internal::Engine,
        native: &mut __rawr_internal::package::import::runner::account::Native,
    ) -> std::io::Result<(Box<[__rawr_internal::host::facade::Definition]>, ApplicationContract)> {{
        if super::IMPORTED_FACADES.is_empty() {{ return Ok((Box::default(), ApplicationContract)); }}
        __rawr_internal::package::import::runner::install_retained(engine, super::IMPORTED_FACADES, native)
            .map(|definitions| (definitions, ApplicationContract))
            .map_err(|error| std::io::Error::other(error.to_string()))
    }}
    pub struct Runtime {{ stage: u8, messages: u64, lease: Option<std::fs::File> }}
    impl Runtime {{
        pub fn new() -> Self {{ Self {{ stage: 0, messages: 0, lease: None }} }}
        pub fn prepare(&mut self) -> Result<(), &'static str> {{
            let _anchor = clock::Clock;
            if self.stage != 0 {{ return Err("prepare order"); }}
            self.stage = 1;
            Ok(())
        }}
        pub fn activate(&mut self) -> Result<(), &'static str> {{
            if self.stage != 1 {{ return Err("activate order"); }}
            let lease = std::fs::OpenOptions::new().write(true).create_new(true)
                .open("{path}").map_err(|_| "exclusive resource unavailable")?;
            self.lease = Some(lease);
            self.stage = 2;
            Ok(())
        }}
        pub fn admit(&mut self, _program: Option<&mut super::Program>) -> Result<(), &'static str> {{
            if self.stage != 2 {{ return Err("admit order"); }}
            self.stage = 3;
            Ok(())
        }}
        pub fn message(&mut self, input: &[u8]) -> Result<Vec<u8>, &'static str> {{
            if self.stage != 3 {{ return Err("message before admission"); }}
            let value = u64::from_le_bytes(input.try_into().map_err(|_| "message shape")?);
            self.messages = self.messages.checked_add(1).ok_or("message exhaustion")?;
            Ok(value.wrapping_add(self.messages).to_le_bytes().to_vec())
        }}
        pub fn retire(&mut self) {{
            self.lease.take();
            let _ = std::fs::remove_file("{path}");
            self.stage = 4;
        }}
    }}
}}"#
    )
}
