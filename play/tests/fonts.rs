//! The container's fonts, on any machine: with `--fonts` (here
//! `fonts::use_only`) a render sees only iced's embedded faces and the
//! bundled Noto files, so a Mac's system fonts cannot hide tofu.
//!
//! The fonts come from `play/fetch-fonts.sh target/play/fonts` (or
//! `PLAY_TEST_FONTS`), the same pinned files the image ships.

use std::path::PathBuf;

use rawr_iced::snapshot::fonts;

fn dir() -> PathBuf {
    std::env::var_os("PLAY_TEST_FONTS").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/play/fonts"),
        PathBuf::from,
    )
}

#[test]
fn bundled_fonts_draw_cjk_symbols_and_emoji() {
    let dir = dir();
    assert!(
        dir.is_dir(),
        "no fonts at {}: run play/fetch-fonts.sh {}",
        dir.display(),
        dir.display()
    );
    let found = fonts::use_only(&dir).expect("bundled fonts");
    assert!(found >= 6, "{found} faces");
    // The detector sees tofu: no bundled font has a noncharacter.
    assert!(fonts::glyphs("\u{10FFFD}").contains(&0));
    for text in [
        "神奈川沖浪裏",
        "ひらがな カタカナ",
        "한국어",
        "简体中文 繁體中文",
        "★ → ✓ ∑",
        "🌊🦀",
        "Rawr kanagawa",
    ] {
        let glyphs = fonts::glyphs(text);
        assert!(!glyphs.is_empty() && !glyphs.contains(&0), "{text}: {glyphs:?}");
    }
    let png = rawr_play::render(
        "column![text(\"神奈川 kanagawa\").size(32), text(\"🌊 ★ 한국어\").size(32)]",
        &rawr_play::Options::default(),
    )
    .expect("render");
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/play/fonts-cjk.png");
    std::fs::write(out, &png.bytes).expect("write");
}
