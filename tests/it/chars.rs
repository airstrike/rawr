use rawr::Engine;

#[test]
fn test_chars() {
    let engine = Engine::new();

    assert_eq!(engine.eval::<char>("'y'").unwrap(), 'y');
    assert_eq!(engine.eval::<char>(r"'\''").unwrap(), '\'');
    assert_eq!(engine.eval::<char>(r#"'"'"#).unwrap(), '"');
    assert_eq!(engine.eval::<char>(r"'\u2764'").unwrap(), '❤');
    // Rust's braced form, as in `'\u{e901}'`.
    assert_eq!(engine.eval::<char>(r"'\u{e901}'").unwrap(), '\u{e901}');
    assert_eq!(engine.eval::<String>(r#""a\u{1F600}b""#).unwrap(), "a\u{1F600}b");
    let _ = engine.eval::<char>(r"'\u{}'").unwrap_err();
    let _ = engine.eval::<char>(r"'\u{1234567}'").unwrap_err();

    #[cfg(not(feature = "no_index"))]
    assert_eq!(engine.eval::<char>(r#"let x="hello"; x[2]"#).unwrap(), 'l');
    #[cfg(not(feature = "no_index"))]
    assert_eq!(engine.eval::<String>(r#"let y="hello"; y[2]='$'; y"#).unwrap(), "he$lo");

    let _ = engine.eval::<char>(r"'\uhello'").unwrap_err();
    let _ = engine.eval::<char>("''").unwrap_err();
}
