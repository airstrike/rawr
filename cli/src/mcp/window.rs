//! A window-only capture of a program's window, found by the pids of the
//! process tree the server started (macOS). Any host's window counts, a
//! winit one included. macOS asks for Screen Recording permission for this
//! capture; an extension's own capture (iced's socket) needs none.

use std::path::Path;

/// One window: its number, its title and its owner's pid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Window {
    /// The window server's number for it.
    pub number: i64,
    /// Its title, when macOS shows it.
    pub title: String,
    /// The pid owning it.
    pub pid: u32,
}

/// The logical size of a window's capture: its width and height in points,
/// title bar included, the space the input tools take positions in. A
/// capture is at the display's backing scale, which the PNG's density
/// gives: 144 dots per inch at 2×, 72 at 1×.
#[must_use]
pub fn points(png: &[u8]) -> Option<(f64, f64)> {
    let number = |at: usize| {
        png.get(at..at + 4)
            .map(|bytes| f64::from(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])))
    };
    if png.get(12..16) != Some(b"IHDR") {
        return None;
    }
    let (width, height) = (number(16)?, number(20)?);
    // The chunks after IHDR, to the first image data: `pHYs` holds the
    // pixels per metre, 72 dpi's being 2835.
    let mut at = 33;
    let mut scale = 1.0;
    while let (Some(length), Some(kind)) = (number(at), png.get(at + 4..at + 8)) {
        match kind {
            b"pHYs" => {
                let density = number(at + 8)?;
                if png.get(at + 16) == Some(&1) && density > 0.0 {
                    scale = (density / 2835.0).round().max(1.0);
                }
                break;
            }
            b"IDAT" | b"IEND" => break,
            _ => at += 12 + length as usize,
        }
    }
    Some((width / scale, height / scale))
}

/// The on-screen windows owned by `pids`.
#[cfg(target_os = "macos")]
#[must_use]
pub fn windows(pids: &[u32]) -> Vec<Window> {
    use core_foundation::base::CFType;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use core_graphics::window::{
        create_description_from_array, create_window_list, kCGNullWindowID,
        kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly,
    };

    let Some(identifiers) = create_window_list(
        kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements,
        kCGNullWindowID,
    ) else {
        return Vec::new();
    };
    let Some(descriptions) = create_description_from_array(identifiers) else {
        return Vec::new();
    };
    let field = |window: &CFDictionary<CFString, CFType>, key: &str| {
        window
            .find(CFString::new(key))
            .map(|value| (*value).clone())
    };
    let mut found = Vec::new();
    for window in descriptions.iter() {
        let number = |key| {
            field(&window, key)
                .and_then(|value| value.downcast::<CFNumber>())
                .and_then(|value| value.to_i64())
        };
        let Some(pid) = number("kCGWindowOwnerPID").and_then(|pid| u32::try_from(pid).ok()) else {
            continue;
        };
        if number("kCGWindowLayer") != Some(0) || !pids.contains(&pid) {
            continue;
        }
        let Some(window_number) = number("kCGWindowNumber") else {
            continue;
        };
        let title = field(&window, "kCGWindowName")
            .and_then(|value| value.downcast::<CFString>())
            .map(|value| value.to_string())
            .unwrap_or_default();
        found.push(Window {
            number: window_number,
            title,
            pid,
        });
    }
    found
}

#[cfg(not(target_os = "macos"))]
#[must_use]
pub fn windows(_pids: &[u32]) -> Vec<Window> {
    Vec::new()
}

/// Capture `window` alone, without its shadow, into `path`, and return the
/// PNG.
#[cfg(target_os = "macos")]
pub fn capture(window: &Window, path: &Path) -> Result<Vec<u8>, String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let status = std::process::Command::new("screencapture")
        .args(["-x", "-o", &format!("-l{}", window.number)])
        .arg(path)
        .stdin(std::process::Stdio::null())
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!("screencapture failed: {status}"));
    }
    std::fs::read(path).map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn capture(_window: &Window, _path: &Path) -> Result<Vec<u8>, String> {
    Err("a window capture needs macOS here".to_owned())
}

#[cfg(test)]
mod tests {
    use super::points;

    /// A PNG's signature, its IHDR for `width` × `height`, an sRGB chunk
    /// and, at `ppm` pixels per metre, a pHYs chunk; the CRCs are zero.
    fn png(width: u32, height: u32, ppm: Option<u32>) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend(13_u32.to_be_bytes());
        bytes.extend(b"IHDR");
        bytes.extend(width.to_be_bytes());
        bytes.extend(height.to_be_bytes());
        bytes.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend(1_u32.to_be_bytes());
        bytes.extend(b"sRGB\0\0\0\0\0");
        if let Some(ppm) = ppm {
            bytes.extend(9_u32.to_be_bytes());
            bytes.extend(b"pHYs");
            bytes.extend(ppm.to_be_bytes());
            bytes.extend(ppm.to_be_bytes());
            bytes.extend([1, 0, 0, 0, 0]);
        }
        bytes.extend(0_u32.to_be_bytes());
        bytes.extend(b"IDAT\0\0\0\0");
        bytes
    }

    #[test]
    fn a_retina_capture_is_half_its_pixels_in_points() {
        assert_eq!(points(&png(1600, 1256, Some(5669))), Some((800.0, 628.0)));
        assert_eq!(points(&png(800, 628, Some(2835))), Some((800.0, 628.0)));
        assert_eq!(points(&png(800, 628, None)), Some((800.0, 628.0)));
        assert_eq!(points(b"not a png"), None);
    }
}
