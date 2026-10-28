use super::{read, Lead};

/// The importer's recorded observations of cargo's own frontmatter reader:
/// each row is a source with cargo's expanded manifest, its error with the
/// span it points at, or a panic.
#[test]
fn reads_frontmatter_as_cargo_does() {
    let observations = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/importer/reference/manifest/frontmatter-panic/expansion.jsonl"
    ));
    let mut rows = 0;
    for line in observations.lines() {
        rows += 1;
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let raw = row["source"].as_str().unwrap();
        let result = read(raw);
        if let Some(output) = row["output"].as_str() {
            let lead = result.unwrap_or_else(|error| panic!("{raw:?}: {error}"));
            // Cargo's expansion: the text before the block, then the block
            // up to its closing fence with the opening fence commented out.
            let expanded = match &lead.frontmatter {
                Some(frontmatter) => format!(
                    "{}#{}",
                    &raw[..frontmatter.open.start],
                    &raw[frontmatter.open.start..frontmatter.close.start]
                ),
                None => raw[..lead.source].to_owned(),
            };
            assert_eq!(expanded, output, "{raw:?}");
        } else if let Some(message) = row["error"].as_str() {
            let error = result.expect_err(raw);
            assert_eq!(error.message, message, "{raw:?}");
            let primary = &row["primary"];
            assert_eq!(
                (error.span.start, error.span.end),
                (
                    primary["start"].as_u64().unwrap() as usize,
                    primary["end"].as_u64().unwrap() as usize
                ),
                "{raw:?}"
            );
        } else {
            assert_eq!(row["panic"], true, "{raw:?}");
            assert!(result.is_err(), "{raw:?}");
        }
    }
    assert_eq!(rows, 335);
}

#[test]
fn the_source_starts_after_the_bom_shebang_and_block() {
    let source = |text: &str| read(text).map(|lead| lead.source);
    assert_eq!(source("fn main() {}"), Ok(0));
    assert_eq!(source("\u{feff}fn main() {}"), Ok(3));
    assert_eq!(source("#!/usr/bin/env rawr\nfn main() {}"), Ok(20));
    assert_eq!(source("#![allow(dead_code)]\n"), Ok(0));
    assert_eq!(source("---cargo\n[dependencies]\n---\n// rawr\n"), Ok(28));
    assert_eq!(source("\u{feff}#!/x\n\n---\n---\nfn"), Ok(17));
    let text = "#!/x\n---cargo\nname = 1\n---\n";
    let Ok(Lead {
        frontmatter: Some(frontmatter),
        ..
    }) = read(text)
    else {
        panic!("{text:?}");
    };
    assert_eq!(&text[frontmatter.info], "cargo");
    assert_eq!(&text[frontmatter.manifest], "name = 1\n");
}

#[test]
fn rustc_takes_three_dashes_or_more_as_a_block() {
    assert!(!read("-1").unwrap_err().opens());
    assert!(!read("--x").unwrap_err().opens());
    assert!(read("---\n").unwrap_err().opens());
}
