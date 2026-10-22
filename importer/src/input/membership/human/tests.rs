use super::{mode, Expected, Mismatch};
use crate::input::membership::{archive, host, stored, Budget, Error};
use crate::{cargo, input, node};

fn explicit(color: &[u8]) -> mode::Mode {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = stored::Stored::from_records(&[], &mut budget).expect("empty mode image");
    let mut host = host::Host::from_image(&image, budget).expect("mode host");
    let mode = mode::Mode::from_inputs(
        mode::Inputs {
            cli: Some(color),
            term: mode::Term::Loaded { color: None },
        },
        &mut host,
    )
    .expect("explicit mode needs no environment");
    host.finish().expect("no environment observations");
    mode
}

fn expected(
    projection: &archive::Projection,
    limit: usize,
) -> crate::input::membership::Result<Expected<'_>> {
    Expected::from_projection(projection, explicit(b"always"), limit)
}

fn inputs<'input>(selected: &'input [&'input [u8]]) -> archive::Inputs<'input> {
    archive::Inputs {
        root: b"/pkg",
        manifest: b"/pkg/Cargo.toml",
        selected,
        license: None,
        readme: None,
        build_scripts: &[],
        include_lockfile: false,
        vcs: false,
    }
}

fn project(input: archive::Inputs<'_>) -> archive::Projection {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = stored::Stored::from_records(&[], &mut budget).expect("empty observation image");
    let mut host = host::Host::from_image(&image, budget).expect("empty image host");
    let projection =
        archive::Projection::from_inputs(input, &mut host).expect("archive projection");
    host.finish().expect("no ambient observations needed");
    projection
}

#[test]
fn passthrough_rendering_keeps_generated_rows_and_manifest_copy_origin() {
    let projection = project(archive::Inputs {
        include_lockfile: true,
        vcs: true,
        ..inputs(&[b"/pkg/src/lib.rs", b"/pkg/cARGo.ToML", b"/pkg/Cargo.lock"])
    });
    let output = b".cargo_vcs_info.json\nCargo.lock\nCargo.toml\nCargo.toml.orig\nsrc/lib.rs\n";
    let compared = expected(&projection, output.len())
        .expect("exact output allowance")
        .compare(output)
        .expect("complete human representation");
    assert_eq!(compared.output(), output);
    assert_eq!(compared.mode(), explicit(b"always"));
    assert!(std::ptr::eq(compared.projection(), &projection));
    assert_eq!(
        projection
            .rows()
            .iter()
            .map(|row| (row.archive(), row.generated()))
            .collect::<Vec<_>>(),
        vec![
            (
                b".cargo_vcs_info.json".as_slice(),
                Some(archive::Generated::VcsInfo)
            ),
            (b"Cargo.lock".as_slice(), Some(archive::Generated::Lockfile)),
            (b"Cargo.toml".as_slice(), Some(archive::Generated::Manifest)),
            (b"Cargo.toml.orig".as_slice(), None),
            (b"src/lib.rs".as_slice(), None),
        ]
    );
    assert_eq!(
        projection.physical().collect::<Vec<_>>(),
        vec![
            (b"Cargo.toml.orig".as_slice(), b"/pkg/Cargo.toml".as_slice()),
            (b"src/lib.rs".as_slice(), b"/pkg/src/lib.rs".as_slice()),
        ]
    );
}

#[test]
fn names_keep_spaces_newlines_carriage_returns_and_virtual_looking_bytes() {
    let projection = project(inputs(&[
        b"/pkg/b\nc",
        b"/pkg/ leading and trailing ",
        b"/pkg/d\r\tz",
        b"/pkg/.cargo_vcs_info.json\nlookalike",
        b"/pkg/Cargo.toml.orig\nlookalike",
    ]));
    let output = b" leading and trailing \n.cargo_vcs_info.json\nlookalike\nCargo.toml.orig\nlookalike\nb\nc\nd\r\tz\n";
    expected(&projection, output.len())
        .expect("bounded expected bytes")
        .compare(output)
        .expect("names are not split, trimmed, or classified from output");
    assert_eq!(projection.physical().count(), 5);
    assert!(projection
        .rows()
        .iter()
        .all(|row| row.generated().is_none()));
}

#[test]
fn identical_human_bytes_do_not_identify_the_archive_mapping() {
    let separate = project(inputs(&[b"/pkg/a", b"/pkg/b"]));
    let embedded = project(inputs(&[b"/pkg/a\nb"]));
    let output = b"a\nb\n";
    let left = expected(&separate, output.len())
        .expect("separate rendering")
        .compare(output)
        .expect("two rows agree");
    let right = expected(&embedded, output.len())
        .expect("embedded rendering")
        .compare(output)
        .expect("one row agrees");
    assert_eq!(left.output(), right.output());
    assert_ne!(left.projection(), right.projection());
    assert_eq!(left.projection().physical().count(), 2);
    assert_eq!(right.projection().physical().count(), 1);
    // Compared retains its own borrowed mapping. It cannot stand in for the
    // missing complete selector, actual Cargo success, or cold before/after bind.
}

#[test]
fn rendering_uses_native_path_order_and_preserves_case_distinct_rows() {
    let projection = project(inputs(&[
        b"/pkg/a.b",
        b"/pkg/a/z",
        b"/pkg/README",
        b"/pkg/readme",
        b"/pkg/a/b",
    ]));
    let output = b"README\na/b\na/z\na.b\nreadme\n";
    expected(&projection, output.len())
        .expect("bounded expected bytes")
        .compare(output)
        .expect("Cargo Path ordering");
    assert_eq!(projection.rows().len(), 5);
    let raw_byte_order = b"README\na.b\na/b\na/z\nreadme\n";
    assert!(matches!(
        expected(&projection, output.len())
            .expect("bounded expected bytes")
            .compare(raw_byte_order),
        Err(Error::Output(Mismatch::Byte { .. }))
    ));
}

#[test]
fn every_truncated_prefix_extra_byte_and_wrong_separator_rejects() {
    let projection = project(inputs(&[b"/pkg/a", b"/pkg/b\nc"]));
    let output = b"a\nb\nc\n";
    for end in 0..output.len() {
        assert!(matches!(
            expected(&projection, output.len())
                .expect("bounded expected bytes")
                .compare(&output[..end]),
            Err(Error::Output(Mismatch::Length { .. }))
        ));
    }
    for changed in [
        b"a\nb\nc\n\n".as_slice(),
        b"a\nb\nc\nx",
        b"a\r\nb\r\nc\r\n",
        b"a\nb\nc",
    ] {
        assert!(matches!(
            expected(&projection, output.len())
                .expect("bounded expected bytes")
                .compare(changed),
            Err(Error::Output(Mismatch::Length { .. }))
        ));
    }
    for (changed, offset) in [
        (b"a b\nc\n".as_slice(), 1),
        (b"a\nb\ncX", 5),
        (b"b\na\nc\n", 0),
    ] {
        assert!(matches!(expected(&projection, output.len())
            .expect("bounded expected bytes").compare(changed),
            Err(Error::Output(Mismatch::Byte { offset: actual })) if actual == offset));
    }
}

#[test]
fn rendering_is_borrowed_and_checks_exact_output_bound_with_prior_usage() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    allocation.items(7).expect("nonzero prior items");
    allocation.bytes(19).expect("nonzero prior bytes");
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = stored::Stored::from_records(&[], &mut budget).expect("empty image");
    let mut host = host::Host::from_image(&image, budget).expect("image host");
    let projection = archive::Projection::from_inputs(inputs(&[b"/pkg/a"]), &mut host)
        .expect("charged projection");
    let mode = explicit(b"always");
    let usage = host.budget().allocation.usage;
    let owned = (
        super::super::OWNED_BYTES.with(std::cell::Cell::get),
        super::super::OWNED_VECTORS.with(std::cell::Cell::get),
    );
    Expected::from_projection(&projection, mode, 2)
        .expect("exact allowance")
        .compare(b"a\n")
        .expect("borrowed complete match");
    assert!(matches!(
        Expected::from_projection(&projection, mode, 1),
        Err(Error::Input(_))
    ));
    assert_eq!(host.budget().allocation.usage, usage);
    assert_eq!(
        owned,
        (
            super::super::OWNED_BYTES.with(std::cell::Cell::get),
            super::super::OWNED_VECTORS.with(std::cell::Cell::get)
        )
    );
    host.finish().expect("no observations");
}

#[test]
fn empty_projection_has_exact_empty_output_without_proof_authority() {
    let projection = project(inputs(&[]));
    expected(&projection, 0)
        .expect("empty expected output")
        .compare(b"")
        .expect("empty bytes agree");
    assert!(matches!(
        expected(&projection, 0)
            .expect("empty expected output")
            .compare(b"\n"),
        Err(Error::Output(Mismatch::Length {
            expected: 0,
            actual: 1
        }))
    ));
}

#[test]
fn stripping_removes_controls_without_changing_retained_origins() {
    let projection = project(inputs(&[
        b"/pkg/\x1b[31mred\x1b[0m",
        b"/pkg/a\x07b\x7fc\x01d",
        b"/pkg/utf8-\xc3\xa9-\xc2\x9b-\xf0\x9f\x98\x80",
    ]));
    let output = b"red\nabcd\nutf8-\xc3\xa9-\xc2\x9b-\xf0\x9f\x98\x80\n";
    let compared = Expected::from_projection(&projection, explicit(b"never"), output.len())
        .expect("stripped rendering fits")
        .compare(output)
        .expect("exact pinned stripped bytes");
    assert_eq!(compared.projection().physical().count(), 3);
    assert_eq!(
        compared.projection().rows()[0].origin(),
        Some(b"/pkg/\x1b[31mred\x1b[0m".as_slice())
    );
    assert!(expected(&projection, 1000)
        .expect("pass-through rendering")
        .compare(output)
        .is_err());
}

#[test]
fn control_state_persists_across_filename_and_separator_writes() {
    // OSC consumes the LF and next row prefix until BEL. Resetting the parser
    // at either the separator or next row would expose bytes Cargo strips.
    let osc = project(inputs(&[b"/pkg/a\x1b]unclosed", b"/pkg/b\x07tail"]));
    Expected::from_projection(&osc, explicit(b"never"), 6)
        .expect("OSC rendering fits")
        .compare(b"atail\n")
        .expect("one stream state crosses both row boundaries");

    // CSI permits LF but keeps its state until the next row's `m` dispatch.
    let csi = project(inputs(&[b"/pkg/a\x1b[31", b"/pkg/mred"]));
    Expected::from_projection(&csi, explicit(b"never"), 6)
        .expect("CSI rendering fits")
        .compare(b"a\nred\n")
        .expect("separator does not reset CSI state");
}

#[test]
fn stripped_output_can_be_empty_or_smaller_than_the_raw_limit() {
    let hidden = project(inputs(&[b"/pkg/\x1b]all rows hidden", b"/pkg/z"]));
    let compared = Expected::from_projection(&hidden, explicit(b"never"), 0)
        .expect("only rendered bytes use the stdout limit")
        .compare(b"")
        .expect("persistent OSC can hide all output");
    assert_eq!(compared.projection().physical().count(), 2);
    assert!(Expected::from_projection(&hidden, explicit(b"never"), 0)
        .expect("empty rendering")
        .compare(b"\n")
        .is_err());

    let short = project(inputs(&[b"/pkg/a\x1b]hidden until stream ends"]));
    Expected::from_projection(&short, explicit(b"never"), 1)
        .expect("exact rendered allowance")
        .compare(b"a")
        .expect("complete rendered byte");
    assert!(matches!(
        Expected::from_projection(&short, explicit(b"never"), 0),
        Err(Error::Input(_))
    ));
}

#[test]
fn stripping_adds_no_owned_output_even_with_prior_allocation_usage() {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::new(&limits);
    allocation.items(7).expect("nonzero prior items");
    allocation.bytes(19).expect("nonzero prior bytes");
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let image = stored::Stored::from_records(&[], &mut budget).expect("empty image");
    let mut host = host::Host::from_image(&image, budget).expect("image host");
    let projection = archive::Projection::from_inputs(inputs(&[b"/pkg/a\x7fb"]), &mut host)
        .expect("charged projection");
    let mode = mode::Mode::from_inputs(
        mode::Inputs {
            cli: Some(b"never"),
            term: mode::Term::Loaded { color: None },
        },
        &mut host,
    )
    .expect("explicit rendering mode");
    let usage = host.budget().allocation.usage;
    let owned = (
        super::super::OWNED_BYTES.with(std::cell::Cell::get),
        super::super::OWNED_VECTORS.with(std::cell::Cell::get),
    );
    Expected::from_projection(&projection, mode, 3)
        .expect("exact rendered allowance")
        .compare(b"ab\n")
        .expect("complete output");
    assert!(matches!(
        Expected::from_projection(&projection, mode, 2),
        Err(Error::Input(_))
    ));
    assert_eq!(host.budget().allocation.usage, usage);
    assert_eq!(
        owned,
        (
            super::super::OWNED_BYTES.with(std::cell::Cell::get),
            super::super::OWNED_VECTORS.with(std::cell::Cell::get)
        )
    );
    host.finish().expect("no ambient observations");
}
