//! Bundled fonts instead of the system's, so a render is the same on any
//! machine: `rawr-play --fonts DIR` and the `?play` container use this.
//!
//! iced's global font system starts with its embedded faces (Fira Sans,
//! the icon font) plus whatever the system has: on macOS that covers CJK and
//! emoji, in a bare container nothing does, so a Mac could hide missing
//! glyphs. [`use_only`] rebuilds it from the embedded faces and the font
//! files in one directory, with Linux's Noto fallback lists on every
//! platform. Files are memory-mapped and a face's data is read only when a
//! glyph falls back to it, so a render that needs no fallback pays for the
//! directory scan alone.

use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use iced::advanced::graphics::text::{cosmic_text, font_system};
use unicode_script::Script;

use cosmic_text::fontdb;

/// Linux's Noto fallbacks (cosmic-text's Unix list, cut to what is bundled).
#[derive(Debug)]
struct Bundled;

impl cosmic_text::Fallback for Bundled {
    fn common_fallback(&self) -> &[&'static str] {
        &[
            "Noto Sans",
            "Noto Sans Mono",
            "Noto Sans Symbols",
            "Noto Sans Symbols2",
            "Noto Color Emoji",
        ]
    }

    fn forbidden_fallback(&self) -> &[&'static str] {
        &[]
    }

    fn script_fallback(&self, script: Script, _locale: &str) -> &[&'static str] {
        match script {
            // Han without a locale reads as Simplified Chinese, as on Linux.
            Script::Han | Script::Bopomofo | Script::Yi => &["Noto Sans CJK SC"],
            Script::Hiragana | Script::Katakana => &["Noto Sans CJK JP"],
            Script::Hangul => &["Noto Sans CJK KR"],
            _ => &[],
        }
    }
}

/// Use only iced's embedded faces and the fonts in `dir`; the number of
/// faces found there.
pub fn use_only(dir: &Path) -> Result<usize, String> {
    let mut system = font_system()
        .write()
        .map_err(|_| "the font system is poisoned".to_owned())?;
    let raw = system.raw();
    let mut db = fontdb::Database::new();
    let mut embedded = HashSet::new();
    for face in raw.db().faces() {
        if let fontdb::Source::Binary(data) = &face.source
            && embedded.insert(Arc::as_ptr(data).cast::<()>() as usize)
        {
            db.load_font_source(fontdb::Source::Binary(Arc::clone(data)));
        }
    }
    let before = db.len();
    db.load_fonts_dir(dir);
    let found = db.len() - before;
    if found == 0 {
        return Err(format!("no fonts in {}", dir.display()));
    }
    db.set_sans_serif_family("Fira Sans");
    db.set_serif_family("Noto Sans");
    db.set_monospace_family("Noto Sans Mono");
    *raw = cosmic_text::FontSystem::new_with_locale_and_db_and_fallback(
        "en-US".to_owned(),
        db,
        Bundled,
    );
    Ok(found)
}

/// Glyph ids `text` shapes to with the current font system; 0 is a
/// missing glyph (tofu).
pub fn glyphs(text: &str) -> Vec<u16> {
    let mut system = font_system().write().expect("font system");
    let raw = system.raw();
    let mut buffer = cosmic_text::Buffer::new(raw, cosmic_text::Metrics::new(16.0, 20.0));
    buffer.set_size(Some(10_000.0), Some(100.0));
    buffer.set_text(
        text,
        &cosmic_text::Attrs::new(),
        cosmic_text::Shaping::Advanced,
        None,
    );
    buffer.shape_until_scroll(raw, false);
    buffer
        .layout_runs()
        .flat_map(|run| run.glyphs.iter().map(|glyph| glyph.glyph_id))
        .collect()
}
