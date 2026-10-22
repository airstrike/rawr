use std::borrow::Cow;

pub struct Case {
    pub id: &'static str,
    pub local_test: &'static str,
    pub purpose: &'static str,
    pub source: Cow<'static, str>,
    pub accepted: bool,
}

impl Case {
    fn literal(
        id: &'static str,
        local_test: &'static str,
        purpose: &'static str,
        source: &'static str,
        accepted: bool,
    ) -> Self {
        Self {
            id,
            local_test,
            purpose,
            source: Cow::Borrowed(source),
            accepted,
        }
    }
}

pub fn cases() -> Vec<Case> {
    let mut cases = vec![
        Case::literal(
            "full-document",
            "source_borrow_and_full_document",
            "Whole document includes unused nested values",
            "title = 'demo'\n[unused]\nvalue = [1, {x=true}]\n",
            true,
        ),
        Case::literal(
            "insertion-order",
            "implicit_table_shift_remove_reinsert_changes_order",
            "Preserve source insertion order, not alphabetical order",
            "z=0\na=1\nm=2\n",
            true,
        ),
        Case::literal(
            "implicit-reinsert-order",
            "implicit_table_shift_remove_reinsert_changes_order",
            "Explicit a removes and reinserts an implicit a after z",
            "[a.b]\nx=1\n[z]\ny=2\n[a]\nq=3\n",
            true,
        ),
        Case::literal(
            "escaped-duplicate-key",
            "decoded_escaped_keys_collide_with_context",
            "Decoded Unicode escape collides with the earlier bare key",
            "a = 1\n\"\\u0061\" = 2\n",
            false,
        ),
        Case::literal(
            "inline-dotted-siblings",
            "dotted_inline_flags_and_collision",
            "Implicit dotted inline parent accepts sibling insertion",
            "x = {a.b=1, a.c=2}\n",
            true,
        ),
        Case::literal(
            "inline-explicit-then-dotted-conflict",
            "dotted_inline_flags_and_collision",
            "Explicit inline child cannot be extended by a dotted sibling",
            "x = {a={b=1}, a.c=2}\n",
            false,
        ),
        Case::literal(
            "inline-dotted-then-explicit-conflict",
            "dotted_inline_flags_and_collision",
            "Dotted inline child cannot be replaced by an explicit child",
            "x = {a.b=1, a={c=2}}\n",
            false,
        ),
        Case::literal(
            "dotted-parent-header-conflict",
            "explicit_dotted_and_inline_extension_rules",
            "Dotted document parent cannot be redeclared by a header",
            "a.b=1\n[a]\nc=2\n",
            false,
        ),
        Case::literal(
            "extend-inline-header-conflict",
            "explicit_dotted_and_inline_extension_rules",
            "Header descent cannot extend an inline table",
            "a={b=1}\n[a.c]\nd=2\n",
            false,
        ),
        Case::literal(
            "extend-ordinary-array-conflict",
            "explicit_dotted_and_inline_extension_rules",
            "Header descent cannot treat an ordinary array as an AoT",
            "a=[]\n[a.b]\nx=1\n",
            false,
        ),
        Case::literal(
            "duplicate-explicit-header",
            "explicit_dotted_and_inline_extension_rules",
            "An explicit header cannot be declared twice",
            "[a]\nx=1\n[a]\ny=2\n",
            false,
        ),
        Case::literal(
            "latest-array-table-descent",
            "array_table_descent_selects_latest_instance",
            "Nested headers attach to the latest array-table instance",
            "[[a]]\nn=1\n[a.child]\nx=2\n[[a]]\nn=3\n[a.child]\nx=4\n",
            true,
        ),
        Case::literal(
            "ordinary-table-array-is-not-aot",
            "array_table_descent_selects_latest_instance",
            "A literal array of tables cannot be extended with an AoT header",
            "a=[{}]\n[[a]]\nx=1\n",
            false,
        ),
        Case::literal(
            "deferred-numeric-overflow",
            "scalar_spelling_and_conversion_stay_deferred",
            "DeTable retains huge integer and float spellings without coercion",
            "hex=0xD_EAD\nhuge=999999999999999999999999999999999\nf=1e9999\ni=+inf\n",
            true,
        ),
        Case::literal(
            "signed-radix-float-spelling",
            "scalar_spelling_and_conversion_stay_deferred",
            "Signs, underscore removal, radix and special floats remain observable",
            "dec=+1_234\nneg=-9_876\nbin=0b10_01\noct=0o7_1\nf=-1_2.3_4e+0_2\nn=-nan\n",
            true,
        ),
        Case::literal(
            "datetime-forms",
            "datetime_validation_and_error_precedence",
            "Date, time, UTC, local datetime and custom offset remain distinct",
            "date=2024-02-29\ntime=07:32:00\nstamp=1979-05-27T07:32:00Z\nlocal=1979-05-27T07:32:00.123\noffset=1979-05-27T07:32:00-07:30\n",
            true,
        ),
        Case::literal(
            "invalid-datetime",
            "datetime_validation_and_error_precedence",
            "Invalid calendar date is decoded before document acceptance",
            "date=2023-02-29\n",
            false,
        ),
        Case::literal(
            "datetime-before-duplicate",
            "datetime_validation_and_error_precedence",
            "Value decoding fails before insertion detects a duplicate",
            "a=1\na=2023-02-29\n",
            false,
        ),
        Case::literal(
            "duplicate-before-later-datetime",
            "datetime_validation_and_error_precedence",
            "An earlier semantic duplicate precedes a later date error",
            "a=1\na=2\nz=2023-02-29\n",
            false,
        ),
        Case::literal(
            "raw-syntax-before-semantic-duplicate",
            "datetime_validation_and_error_precedence",
            "Raw document diagnostics precede semantic diagnostics",
            "a=1\na=2\nz=[\n",
            false,
        ),
        Case::literal(
            "unused-joined-scalar",
            "ignored_values_and_joined_raw_scalar_are_fully_decoded",
            "An unused array still requires valid separators",
            "name='valid'\nunused=[1 2]\n",
            false,
        ),
        Case::literal(
            "unused-invalid-escape",
            "ignored_values_and_joined_raw_scalar_are_fully_decoded",
            "An unused string still requires valid escapes",
            "name='valid'\nunused=\"\\q\"\n",
            false,
        ),
        Case::literal(
            "unused-inline-duplicate",
            "ignored_values_and_joined_raw_scalar_are_fully_decoded",
            "An unused inline table still requires unique decoded keys",
            "name='valid'\nunused={x=1,x=2}\n",
            false,
        ),
        Case::literal(
            "unicode-multiline-escapes",
            "complete_string_forms_and_decoded_expansion",
            "Decode Unicode, literal multiline text and escaped line continuation",
            "a=\"\\u00e9\\U0001F600\"\nb='''\nraw\\value\n'''\nc=\"\"\"\nhello\\\n   world\"\"\"\n",
            true,
        ),
        Case::literal(
            "unicode-key-and-json-controls",
            "complete_string_forms_and_decoded_expansion",
            "Native Unicode keys and decoded controls exercise output escaping",
            "\"café😀\"=\"\\b\\t\\n\\f\\r\\\"\\\\\\u0000\"\n",
            true,
        ),
        Case::literal(
            "raw-malformed-recovery",
            "ignored_values_and_joined_raw_scalar_are_fully_decoded",
            "Bounded malformed scalar and array preserve raw recovery evidence",
            "a=[1 2]\nb=\"\\q\"\nc=3\n",
            false,
        ),
    ];

    for (id, depth, accepted) in [("array-depth-80", 80, true), ("array-depth-81", 81, false)] {
        cases.push(Case {
            id,
            local_test: "caller_and_pinned_depth_apply_to_containers_and_dotted_keys",
            purpose: "Pinned raw value-container recursion boundary",
            source: Cow::Owned(format!("a={}1{}\n", "[".repeat(depth), "]".repeat(depth))),
            accepted,
        });
    }
    for (id, depth, accepted) in [
        ("dotted-depth-80", 80, true),
        ("dotted-depth-81", 81, false),
    ] {
        cases.push(Case {
            id,
            local_test: "caller_and_pinned_depth_apply_to_containers_and_dotted_keys",
            purpose: "Pinned semantic dotted-key count boundary",
            source: Cow::Owned(format!("{}=1\n", vec!["k"; depth].join("."))),
            accepted,
        });
    }
    for (id, depth, accepted) in [
        ("header-depth-80", 80, true),
        ("header-depth-81", 81, false),
    ] {
        cases.push(Case {
            id,
            local_test: "caller_and_pinned_depth_apply_to_containers_and_dotted_keys",
            purpose: "Pinned semantic header-key count boundary",
            source: Cow::Owned(format!("[{}]\nx=1\n", vec!["k"; depth].join("."))),
            accepted,
        });
    }

    // Two unexpected closes, then 82 actual arrays: bounded reference evidence
    // for the recovery counter issue, never the local 10,000-container fixture.
    cases.push(Case {
        id: "bounded-negative-depth-recovery",
        local_test: "malformed_negative_depth_recovery_cannot_enter_deep_arrays",
        purpose: "Small malformed recovery sample; acceptance remains false",
        source: Cow::Owned(format!(
            "x={{[] [] }}\ny={}0{}\n",
            "[".repeat(82),
            "]".repeat(82)
        )),
        accepted: false,
    });

    // Retained depth exceeds syntactic inline depth, while staying small enough
    // for the upstream recursive representation and destructor.
    let dotted = vec!["k"; 8].join(".");
    let mut compound = String::from("root=");
    for _ in 0..4 {
        compound.push_str(&format!("{{{dotted}="));
    }
    compound.push('1');
    compound.push_str("}}}}\n");
    cases.push(Case {
        id: "compound-dotted-inline",
        local_test: "compounded_dotted_inline_depth_drops_without_recursive_ownership",
        purpose: "Four inline containers each add eight retained dotted tables",
        source: Cow::Owned(compound.clone()),
        accepted: true,
    });
    compound.push_str("root=2\n");
    cases.push(Case {
        id: "compound-dotted-inline-then-error",
        local_test: "compounded_dotted_inline_depth_drops_without_recursive_ownership",
        purpose: "Later duplicate leaves a modest recovered compound tree",
        source: Cow::Owned(compound),
        accepted: false,
    });
    cases
}
