//! Fonts fetched on demand. The module embeds only iced's Fira Sans, so
//! Fira Mono (for `Font::MONOSPACE`) and the Noto fallbacks `?play` renders
//! with are fetched next to it, from `fonts/`, when a snippet needs them:
//! Fira Mono for any app, and the symbol, emoji and CJK faces only for a
//! snippet whose source has such characters. Each file is fetched once and
//! given to iced with `iced::font::load`.

use iced::Task;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// A font file next to the module, and when a snippet needs it.
pub struct Face {
    pub file: &'static str,
    needed: fn(char) -> bool,
}

/// The faces, in the order they are fetched.
pub const FACES: &[Face] = &[
    Face {
        file: "FiraMono-Regular.ttf",
        needed: |_| true,
    },
    Face {
        file: "NotoSansSymbols-Regular.ttf",
        needed: symbol,
    },
    Face {
        file: "NotoSansSymbols2-Regular.ttf",
        needed: symbol,
    },
    Face {
        file: "NotoColorEmoji.ttf",
        needed: emoji,
    },
    Face {
        file: "NotoSansCJK-Regular.ttc",
        needed: cjk,
    },
];

/// The family `Font::MONOSPACE` resolves to once Fira Mono is in.
pub const MONOSPACE: &str = "Fira Mono";

/// The faces `source` needs, by index in [`FACES`].
pub fn needed(source: &str) -> impl Iterator<Item = usize> + '_ {
    FACES
        .iter()
        .enumerate()
        .filter(move |(_, face)| source.chars().any(face.needed))
        .map(|(index, _)| index)
}

fn emoji(c: char) -> bool {
    matches!(c, '\u{1F000}'..='\u{1FAFF}' | '\u{2600}'..='\u{27BF}' | '\u{FE0F}')
}

fn cjk(c: char) -> bool {
    matches!(
        c,
        '\u{1100}'..='\u{11FF}'
            | '\u{2E80}'..='\u{9FFF}'
            | '\u{AC00}'..='\u{D7AF}'
            | '\u{F900}'..='\u{FAFF}'
            | '\u{FF00}'..='\u{FFEF}'
            | '\u{20000}'..='\u{3FFFF}'
    )
}

/// Arrows, math, technical, box drawing, shapes, dingbats and the like,
/// past Fira Sans. A dingbat such as ★ may also be an emoji.
fn symbol(c: char) -> bool {
    matches!(c, '\u{2000}'..='\u{2BFF}' | '\u{1F000}'..='\u{1FBFF}')
}

#[wasm_bindgen]
extern "C" {
    /// The URL this module was loaded from.
    #[wasm_bindgen(thread_local_v2, js_namespace = ["import", "meta"], js_name = url)]
    static MODULE_URL: String;
}

/// Fetch face `index` and load it into iced's font system. The task ends
/// with a message, so the app lays its text out again with the new face.
pub fn load(index: usize) -> Task<()> {
    let file = FACES[index].file;
    Task::future(fetch(file)).then(move |bytes| match bytes {
        Ok(bytes) => {
            if file.starts_with("FiraMono") {
                use iced::advanced::graphics::text::font_system;
                if let Ok(mut system) = font_system().write() {
                    system.raw().db_mut().set_monospace_family(MONOSPACE);
                }
            }
            let size = bytes.len();
            iced::font::load(bytes).map(move |loaded| {
                crate::log(&format!("rawr-web: font {file} ({size} bytes): {loaded:?}"));
            })
        }
        Err(error) => {
            crate::log(&format!("rawr-web: font {file} did not load: {error}"));
            Task::none()
        }
    })
}

async fn fetch(file: &str) -> Result<Vec<u8>, String> {
    let error = |error: JsValue| format!("{error:?}");
    let url = MODULE_URL.with(|base| web_sys::Url::new_with_base(&format!("fonts/{file}"), base));
    let url = url.map_err(error)?.href();
    let window = web_sys::window().ok_or("no window")?;
    let response: web_sys::Response = JsFuture::from(window.fetch_with_str(&url))
        .await
        .map_err(error)?
        .dyn_into()
        .map_err(error)?;
    if !response.ok() {
        return Err(format!("{url}: HTTP {}", response.status()));
    }
    let buffer = JsFuture::from(response.array_buffer().map_err(error)?)
        .await
        .map_err(error)?;
    Ok(js_sys::Uint8Array::new(&buffer).to_vec())
}
