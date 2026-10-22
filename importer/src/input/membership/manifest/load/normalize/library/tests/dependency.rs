//! Actual Reader/normalized Host pipeline through retained dependency ownership.
use super::*;
use crate::input::membership::manifest::load::{
    convert::{dependency, platform, source},
    normalize::{dependencies, tail, validate::Errors},
};

fn run(
    items: usize,
    bytes: usize,
    artifact: &str,
) -> (
    Result<(), Error<crate::input::membership::Error>>,
    input::Usage,
    crate::exceedance::Record,
) {
    let text = format!("cargo-features=['public-dependency']\n[package]\nname='p'\nedition='2021'\nbuild=false\nreadme=false\nautolib=false\nautobins=false\nautoexamples=false\nautotests=false\nautobenches=false\n[dependencies]\na={{version='1.2.3-alpha',git='https://example.com/repo',package='actual',features=['one','two','one'],optional=true,default-features=false,public=true{artifact}}}");
    let image = image(vec![file("/p/Cargo.toml", &text)]);
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(items, bytes, usize::MAX);
    let mut host = match host::Host::from_image(
        &image,
        Budget {
            graph: &graph,
            allocation: &mut allocation,
        },
    ) {
        Ok(host) => host,
        Err(error) => {
            return (
                Err(Error::Host(error)),
                allocation.usage(),
                allocation.exceedances(),
            )
        }
    };
    let mut history = source::History::new();
    let mut paths = slots::Slots::new();
    let mut config_documents = slots::Slots::new();
    let files = Files::from_owners(paths.writer().unwrap(), config_documents.writer().unwrap());
    let mut bodies = slots::Slots::new();
    let mut documents = slots::Slots::new();
    let mut originals = slots::Slots::new();
    let mut reader = Reader {
        bodies: bodies.writer().unwrap(),
        documents: documents.writer().unwrap(),
        originals: originals.writer().unwrap(),
    };
    let mut roots = Roots::new();
    let mut session = Session::new(&mut host);
    let result = (|| {
        let mut owner = Owner::from_files(files, context());
        let prepared = reader.prepare(
            Path::new("/p/Cargo.toml"),
            false,
            reader_context(),
            &mut roots,
            &limits,
            &mut session,
        )?;
        let normalize::Progress::Patches(patches) =
            prepared.normalize_patches(&mut owner, &limits, &mut session)?
        else {
            panic!("patches")
        };
        let Progress::Complete(package) = patches.normalize_package(&mut session)? else {
            panic!("package")
        };
        let targets = package
            .normalize_library(&mut session)?
            .normalize_targets(&mut Errors::new(), &mut session)?
            .unwrap();
        let (inputs, mut warnings) = targets.into_tail();
        let Some(tail::Progress::Complete(tail)) = inputs
            .normalize(false, &mut warnings, &mut owner, &limits, &mut session)?
            .unwrap()
        else {
            panic!("tail")
        };
        let mut sources = history.open(&mut session)?;
        let url =
            crate::input::membership::manifest::spec::url::Url::parse("file:///p", &mut session)?
                .unwrap();
        let parent = sources
            .intern(url, source::Origin::Path, None, None, &mut session)?
            .ok()
            .unwrap();
        let platform = platform::parse("cfg(any(unix, windows))", &mut session)?.unwrap();
        let dependency = &tail.maps().get(dependencies::Kind::Normal).unwrap()[0];
        let mut converted = dependency::convert(
            dependency,
            Path::new("/p/Cargo.toml"),
            parent,
            &mut sources,
            &mut owner,
            dependencies::Kind::Normal,
            Some(&platform),
            true,
            true,
            &limits,
            &mut warnings,
            &mut session,
        )?
        .ok()
        .unwrap();
        let record = match &converted {
            dependency::Progress::Complete(value) => {
                assert_eq!(value.has_artifact(), !artifact.is_empty());
                assert!(!artifact.contains(".json"));
                value.fields()
            }
            dependency::Progress::Canonical(value) => {
                assert!(artifact.contains(".json"));
                assert_eq!(value.spelling(), "relative/custom.json");
                assert_eq!(value.dependency_name(), "a");
                value.fields()
            }
        };
        assert_eq!(record.name(), "actual");
        assert_eq!(record.rename(), Some("a"));
        assert!(record.public());
        assert_eq!(record.features(), ["one", "two", "one"]);
        assert_eq!(
            record.requirement().unwrap().comparators()[0].pre(),
            "alpha"
        );
        assert_eq!(
            record.platform().unwrap().to_string(),
            "cfg(any(unix, windows))"
        );
        if let dependency::Progress::Complete(value) = &mut converted {
            use crate::input::membership::manifest::{scalar::requirement::Op, spec::Spec};
            let spec = Spec::parse("a@2.3.4-beta+build", None, &mut session)?;
            let full = spec.full_version(&mut session)?.unwrap();
            value.replace_version(&full, &mut session)?;
            let requirement = value.fields().requirement().unwrap();
            let [comparator] = requirement.comparators() else {
                panic!("one exact replacement comparator")
            };
            assert!(matches!(comparator.op, Op::Exact));
            assert_eq!(
                (
                    comparator.major,
                    comparator.minor,
                    comparator.patch,
                    comparator.pre()
                ),
                (2, Some(3), Some(4), "beta")
            );
            assert_eq!(value.fields().name(), "actual");
            assert_eq!(value.fields().features(), ["one", "two", "one"]);
            assert_eq!(value.has_artifact(), !artifact.is_empty());
        }
        Ok(())
    })();
    if result.is_err() {
        assert!(session.items(0).is_err());
    }
    let result = session.finish(result);
    drop(roots);
    let result = result.and_then(|()| host.finish().map(|_| ()).map_err(Error::Host));
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    (result, allocation.usage(), allocation.exceedances())
}

#[test]
fn composed_dependency_keeps_original_host_account_at_exact_and_one_short_limits() {
    use crate::input::membership::manifest::tests::one_short;
    for artifact in [
        "",
        ",artifact=['bin:tool'],lib=true,target='aarch64-apple-darwin'",
        ",artifact=['bin:tool'],lib=true,target=' relative/custom.json '",
    ] {
        let (result, usage, _) = run(usize::MAX, usize::MAX, artifact);
        result.unwrap();
        let (result, exact, exceeded) = run(usage.items, usage.bytes, artifact);
        result.unwrap();
        assert_eq!(exact, usage);
        assert!(!exceeded.is_exceeded());
        // One short of an aggregate target admits and records that axis.
        let (result, exact, exceeded) = run(usage.items - 1, usage.bytes, artifact);
        result.unwrap();
        assert_eq!(exact, usage);
        assert!(one_short(
            &exceeded,
            input::INPUT_ITEMS_RESOURCE,
            usage.items
        ));
        let (result, exact, exceeded) = run(usage.items, usage.bytes - 1, artifact);
        result.unwrap();
        assert_eq!(exact, usage);
        assert!(one_short(
            &exceeded,
            input::INPUT_BYTES_RESOURCE,
            usage.bytes
        ));
    }
}
