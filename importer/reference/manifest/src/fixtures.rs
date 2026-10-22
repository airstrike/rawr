//! Fixed, modest compatibility inputs; never product parser or budget tests.

pub(crate) struct Case {
    pub(crate) id: String,
    pub(crate) purpose: &'static str,
    pub(crate) source: String,
    // None means capture evidence without guessing an unexecuted outcome.
    pub(crate) accepted: Option<bool>,
}

fn push(
    cases: &mut Vec<Case>,
    id: impl Into<String>,
    purpose: &'static str,
    source: impl Into<String>,
    accepted: Option<bool>,
) {
    cases.push(Case {
        id: id.into(),
        purpose,
        source: source.into(),
        accepted,
    });
}

pub(crate) fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    push(&mut cases, "complete-families", "Every manifest field family, aliases, typed map ordering, generic values and dependency extras", include_str!("../fixtures/full.toml"), Some(true));
    push(
        &mut cases,
        "workspace-and-inheritance",
        "All inheritable package fields and workspace families",
        include_str!("../fixtures/workspace.toml"),
        Some(true),
    );
    for (id, purpose, source, accepted) in [
        ("empty", "Typed interpretation does not impose later normalized package requirements", "", true),
        ("package-empty", "Optional map fields default to None", "[package]\n", true),
        ("alias-shadow-invalid", "Unused project alias still converts before package preference", "package={name='ok'}\nproject={name=7}\n", false),
        ("alias-same-order-a", "Retain both alias slots independently", "dev_dependencies={a='2'}\ndev-dependencies={a='1'}\n", true),
        ("alias-same-order-b", "Alias precedence does not depend on source order", "dev-dependencies={a='1'}\ndev_dependencies={a='2'}\n", true),
        ("unknown-order", "Ignored callback order differs from sorted final unused keys", "z={b=1,a=2}\na=[{q=false}]\n", true),
        ("unknown-delimited-key", "Cargo unused path rendering does not escape dots", "'x.y'={ 'z.w'=1 }\n", true),
        ("profile-positional-empty", "Profile's container default permits an empty positional array", "[profile]\ndev=[]\n", true),
        ("hints-positional", "Ordinary nested derived structs accept positional arrays", "hints=[true]\n", true),
        ("hints-positional-missing", "Option alone does not default a missing positional field", "hints=[]\n", false),
        ("hints-positional-extra", "Direct array adapter permits unconsumed invalid typed trailing values", "hints=[true,340282366920938463463374607431768211456]\n", true),
        ("hints-extra-syntax", "Syntax parsing still rejects an unconsumed invalid escape", "hints=[true,'ok',\"\\q\"]\n", false),
        ("target-positional-empty", "Target has field Options but no container positional defaults", "lib=[]\n", false),
        ("flatten-positional-rejected", "Flattened dependency structs do not have derived positional visitors", "dependencies={a=[]}\n", false),
        ("profile-unknown-overflow", "Unknown struct fields still dispatch scalar conversion", "[profile.dev]\nunknown=340282366920938463463374607431768211456\n", false),
        ("unknown-u128", "IgnoredAny accepts the direct TOML u128 domain", "unused=340282366920938463463374607431768211455\n", true),
        ("unknown-over-u128", "IgnoredAny rejects beyond the direct TOML integer domain", "unused=340282366920938463463374607431768211456\n", false),
        ("metadata-u64-over-i64", "Generic TOML value rejects beyond i64", "package.metadata=9223372036854775808\n", false),
        ("metadata-i64-min", "Generic TOML signed lower endpoint", "package.metadata=-9223372036854775808\n", true),
        ("metadata-float-special", "Preserve NaN, infinities and zero sign in the full typed projection", "package.metadata=[nan,+nan,-nan,inf,+inf,-inf,0.0,-0.0,1.5,1e-9999]\n", true),
        ("metadata-float-overflow", "Non-explicit float overflow is a conversion error", "package.metadata=1e9999\n", false),
        ("ignored-float-overflow", "IgnoredAny also performs float conversion", "unused=1e9999\n", false),
        ("dependency-stage-u128", "Inherited dependency initial serde-value capture lacks u128", "dependencies.a={unused=18446744073709551616}\n", false),
        ("dependency-extra-u64", "Staging may accept u64 before generic flattened extras reject it", "dependencies.a={unused=9223372036854775808}\n", false),
        ("dependency-sorted-stage", "Dependency extras retain BTree-sorted intermediate key order", "dependencies.a={z={z=1,a=2},a=3}\n", true),
        ("missing-before-flatten", "Missing lint level precedes delayed generic extra overflow", "lints.rust.a={extra=9223372036854775808}\n", false),
        ("ordinary-before-missing", "Known field conversion precedes declaration-order missing level", "lints.rust.a={priority=128,extra=9223372036854775808}\n", false),
        ("flatten-after-known", "Deferred flattened extras convert after known fields", "lints.rust.a={extra=9223372036854775808,priority=128,level='warn'}\n", false),
        ("inherit-field-false", "Successful inherited field false rejects", "package.version={workspace=false}\n", false),
        ("inherit-dependency-false", "Successful inherited dependency false rejects without fallback", "dependencies.a={workspace=false}\n", false),
        ("inherit-trial-fallback", "Failed inherited trial can fall back to detailed dependency, retaining workspace as extra", "dependencies.a={workspace='not-bool',version='1'}\n", true),
        ("inherit-false-trial-fallback", "False is terminal only after the whole inherited trial succeeds", "dependencies.a={workspace=false,features=1}\n", false),
        ("inherit-unknown-retained", "Inherited dependency retains converted unknown fields", "dependencies.a={workspace=true,z=1,a=2}\n", true),
        ("inherit-lints-false", "Explicit false differs from absent lints workspace", "lints={workspace=false}\n", false),
        ("inherit-lints-empty", "Absent lints workspace defaults false", "lints={}\n", true),
        ("lint-unit-table", "A unit enum may accept an empty table payload", "lints.rust.a={level={warn={}}}\n", true),
        ("lint-unit-array", "TOML enum unit accepts an empty array payload", "lints.rust.a={level={warn=[]}}\n", true),
        ("lint-unit-nonempty", "Nonempty unit payload rejects", "lints.rust.a={level={warn=[0]}}\n", false),
        ("trimpaths-staged-error", "All sequence elements become String before enum validation", "profile.dev.trim-paths=['invalid',123]\n", false),
        ("trimpaths-semantic-error", "All-string sequence reaches trim-path enum validation", "profile.dev.trim-paths=['invalid','macro']\n", false),
        ("typed-equal-spec-keys", "Distinct raw spec spellings replace one typed BTreeMap key", "[profile.dev.package]\n'foo:1.2'={opt-level=1}\n'foo@1.2'={opt-level=2}\n'*'={opt-level=3}\n", true),
        ("typed-equal-url-keys", "URL normalization creates typed equal keys", "[profile.dev.package]\n'https://EXAMPLE.com:443/foo'={opt-level=1}\n'https://example.com/foo'={opt-level=2}\n", true),
        ("partial-build-presence", "Partial and full version components, including build absence/presence", "[profile.dev.package]\n'foo@1'={}\n'foo@1.2'={}\n'foo@1.2.3'={}\n'foo@1.2.3-alpha+build'={}\n", true),
        ("package-trims-version", "Inheritable package SemVer trims surrounding whitespace", "package.version=' 1.2.3 '\n", true),
        ("workspace-keeps-version-space", "Workspace SemVer uses direct conversion without trim", "workspace.package.version=' 1.2.3 '\n", false),
        ("invalid-cargo-features", "Special package cargo-features rejects before consuming its value", "package.cargo-features=340282366920938463463374607431768211456\n", false),
        ("input-order-error-a", "Ordinary known field errors follow input order", "package={name=123,version=false}\n", false),
        ("input-order-error-b", "Reversing ordinary fields reverses selected failure", "package={version=false,name=123}\n", false),
        ("names-unicode-namespace", "Typed schemas accept Unicode and namespaced package names", "package.name='café::outil'\ndependencies.'café::outil'='1'\nfeatures.'é+x'= []\n", true),
        ("profile-name-reserved", "Profile name validation includes case folding", "profile.'DEBUG'={}\n", false),
        ("root-syntax-first", "Complete document failure precedes any typed interpretation", "package.name=7\nunused=\"\\q\"\n", false),
    ] {
        push(&mut cases, id, purpose, source, Some(accepted));
    }

    // A full positional profile plus one unconsumed overflowing integer.
    push(&mut cases, "profile-positional-extra", "All 19 positional fields plus unvisited extra; preserves raw sequence order",
        "profile.dev=[0,false,'llvm',1,false,'off',true,false,'unwind',true,false,'dir','dev',false,[],{},[],false,true,340282366920938463463374607431768211456]\n", Some(true));

    for (id, literal) in [
        ("true", "true"),
        ("false", "false"),
        ("none", "'none'"),
        ("all", "'all'"),
        ("single", "'macro'"),
        ("empty", "[]"),
        ("ordered-duplicates", "['object','macro','object']"),
    ] {
        push(
            &mut cases,
            format!("trimpaths-{id}"),
            "Exact trim-path variant and retained sequence",
            format!("profile.dev.trim-paths={literal}\n"),
            Some(true),
        );
    }
    for (id, literal) in [
        ("false", "false"),
        ("true", "true"),
        ("zero", "0"),
        ("one", "1"),
        ("two", "2"),
        ("none", "'none'"),
        ("limited", "'limited'"),
        ("full", "'full'"),
        ("lines", "'line-directives-only'"),
        ("tables", "'line-tables-only'"),
    ] {
        push(
            &mut cases,
            format!("debug-{id}"),
            "Debug enum conversion and alias collapse",
            format!("profile.dev.debug={literal}\n"),
            Some(true),
        );
    }
    for (id, source) in [
        ("build-auto", "package.build=true"),
        ("build-disabled", "package.build=false"),
        ("build-single", "package.build='build.rs'"),
        ("build-multiple", "package.build=['build.rs']"),
        ("metabuild-single", "package.metabuild='builder'"),
        ("metabuild-array", "package.metabuild=['builder']"),
        ("publish-true", "package.publish=true"),
        ("publish-false", "package.publish=false"),
        ("publish-empty", "package.publish=[]"),
        ("readme-true", "package.readme=true"),
        ("readme-path", "package.readme='README.md'"),
        ("lto-bool", "profile.dev.lto=true"),
        ("lto-string", "profile.dev.lto='false'"),
        ("strip-string", "profile.dev.strip='symbols'"),
    ] {
        push(
            &mut cases,
            id,
            "Preserve selected schema union or wrapper, not only its Serialize result",
            format!("{source}\n"),
            Some(true),
        );
    }
    for (id, value, accepted) in [
        ("u32-max", "codegen-units=4294967295", true),
        ("u32-over", "codegen-units=4294967296", false),
        ("u32-negative", "codegen-units=-1", false),
        ("opt-i64-min", "opt-level=-9223372036854775808", true),
        ("opt-i64-max", "opt-level=9223372036854775807", true),
        ("opt-u64", "opt-level=9223372036854775808", false),
        ("opt-string-number", "opt-level='3'", false),
        ("debug-three", "debug=3", false),
        ("debug-float", "debug=1.0", false),
    ] {
        push(
            &mut cases,
            id,
            "Distinct typed scalar domain and later normalization boundary",
            format!("[profile.dev]\n{value}\n"),
            Some(accepted),
        );
    }

    datetime(&mut cases);
    specs(&mut cases);
    nested(&mut cases);
    cases
}

fn datetime(cases: &mut Vec<Case>) {
    for (id, source) in [
        ("datetime-native", "package.metadata=2026-09-05T01:02:03.123456789-04:30\n"),
        ("datetime-marker-first", "package.metadata={'$__toml_private_datetime'='2026-09-05', trailing=9223372036854775808}\n"),
        ("datetime-marker-later", "package.metadata={first=1,'$__toml_private_datetime'='2026-09-05'}\n"),
        ("datetime-marker-invalid", "package.metadata={'$__toml_private_datetime'='invalid', trailing=9223372036854775808}\n"),
        ("datetime-marker-sequence", "package.metadata={'$__toml_private_datetime'='2026-09-05', trailing=[340282366920938463463374607431768211456]}\n"),
        ("datetime-staged-extra", "dependencies.a={extra={'$__toml_private_datetime'='2026-09-05', trailing=9223372036854775808}}\n"),
        ("datetime-staged-native", "dependencies.a={extra=2026-09-05T01:02:03Z}\n"),
        ("datetime-flattened", "lints.rust.a={level='warn',extra={'$__toml_private_datetime'='2026-09-05',trailing=9223372036854775808}}\n"),
    ] {
        push(cases, id, "Synthetic datetime map, first-key early return and staged-map exhaustion; capture actual outcome", source, None);
    }
}

fn specs(cases: &mut Vec<Case>) {
    for (id, spec, accepted) in [
        ("plain", "foo", true),
        ("namespace", "foo::bar@1.2.3", true),
        ("unicode", "café@1.2.3", true),
        ("https", "https://example.com/foo#1.2.3", true),
        ("ssh", "ssh://git@example.com/repo.git#foo@1.2.3", true),
        ("file", "file:///path/to/foo", true),
        ("path", "path+file:///path/to/foo#bar@1.2.3", true),
        (
            "registry",
            "registry+https://example.com/index#foo@1.2.3",
            true,
        ),
        ("sparse", "sparse+https://example.com/index#foo@1.2.3", true),
        (
            "git-branch",
            "git+https://example.com/repo?branch=a%2Fb#foo@1.2.3",
            true,
        ),
        (
            "git-last-query",
            "git+https://example.com/repo?branch=a&tag=t&rev=123#foo",
            true,
        ),
        (
            "git-default",
            "git+https://example.com/repo?unknown=1#foo",
            true,
        ),
        (
            "git-ref-alias",
            "git+https://example.com/repo?ref=main#foo",
            true,
        ),
        ("url-namespace", "file:///path/foo#foo::bar@1.2.3", true),
        ("url-unicode-fragment", "file:///path/foo#café@1.2.3", false),
        ("query-rejected", "https://example.com/foo?x=1", false),
        ("bad-protocol", "other+https://example.com/foo", false),
        ("bad-path-protocol", "path+https://example.com/foo", false),
        ("empty-fragment", "https://example.com/foo#", false),
        ("empty-build", "foo@1.2.3+", false),
        ("partial-prerelease", "foo@1.2-alpha", false),
        ("bad-operator", "foo@^1.2", false),
        ("diagnostic-existing", "probe/existing", false),
        ("diagnostic-missing", "probe/missing", false),
        ("diagnostic-backslash", "probe\\existing", false),
    ] {
        // Literal quoted keys retain backslashes without TOML escape handling.
        push(
            cases,
            format!("spec-{id}"),
            "Actual PackageIdSpec parse; no command execution or selected-owner proof",
            format!("[profile.dev.package]\n'{spec}'={{opt-level=1}}\n"),
            Some(accepted),
        );
    }
    push(
        cases,
        "spec-key-before-value",
        "Reject profile key before an invalid typed profile value",
        "[profile.dev.package]\n'probe/missing'=340282366920938463463374607431768211456\n",
        Some(false),
    );
}

fn nested(cases: &mut Vec<Case>) {
    let mut nested = "{opt-level=1}".to_owned();
    for _ in 0..12 {
        nested = format!("{{build-override={nested}}}");
    }
    push(
        cases,
        "recursive-profile",
        "Modest actual recursive schema value",
        format!("profile.dev={nested}\n"),
        Some(true),
    );
    let mut nested = "{debug=7}".to_owned();
    for _ in 0..12 {
        nested = format!("{{build-override={nested}}}");
    }
    push(
        cases,
        "recursive-profile-failure",
        "Modest nested typed error with complete document syntax",
        format!("profile.dev={nested}\n"),
        Some(false),
    );
}
