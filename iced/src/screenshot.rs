//! Interactive window screenshots for the live `rawr run` / `rawr preview` hosts.
//!
//! This captures the live renderer through iced's window screenshot action and
//! writes `<project>/screenshots/<name>-<UTC timestamp>.png`. It is unrelated to
//! `rawr snapshot`, which renders pinned headless baselines.

use std::path::{Path, PathBuf};

/// Outcome of one capture: the written path, or a readable failure.
pub(crate) type Saved = Result<PathBuf, String>;

/// Capture the newest window, drop `chrome` logical pixels from its bottom edge,
/// and save it under the project's `screenshots/` folder.
pub(crate) fn capture<Message: Send + 'static>(
    name: String,
    chrome: f32,
    map: fn(Saved) -> Message,
) -> iced::Task<Message> {
    iced::window::latest().then(move |window| {
        let name = name.clone();
        match window {
            Some(window) => iced::window::screenshot(window).then(move |shot| {
                let name = name.clone();
                iced::Task::perform(async move { save(&root(), &name, &shot, chrome) }, map)
            }),
            None => iced::Task::done(map(Err("no window to capture".to_owned()))),
        }
    })
}

/// Capture the newest window whole into `path` (`rawr mcp`'s input tools).
pub(crate) fn capture_into<Message: Send + 'static>(
    path: PathBuf,
    map: impl Fn(Saved) -> Message + Clone + Send + 'static,
) -> iced::Task<Message> {
    iced::window::latest().then(move |window| {
        let (path, map) = (path.clone(), map.clone());
        match window {
            Some(window) => iced::window::screenshot(window).then(move |shot| {
                let path = path.clone();
                iced::Task::perform(async move { write(&path, &shot) }, map.clone())
            }),
            None => iced::Task::done(map(Err("no window to capture".to_owned()))),
        }
    })
}

pub(crate) fn root() -> PathBuf {
    // The generated runner receives its project root; in-process hosts run
    // from the project directory.
    std::env::var_os("RAWR_PROJECT_ROOT")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_default()
}

fn save(root: &Path, name: &str, shot: &iced::window::Screenshot, chrome: f32) -> Saved {
    let shot = crop(shot, chrome)?;
    let directory = root.join("screenshots");
    let path = unique(
        &directory,
        &slug(name),
        &timestamp(std::time::SystemTime::now()),
    );
    write(&path, &shot)
}

fn write(path: &Path, shot: &iced::window::Screenshot) -> Saved {
    if let Some(directory) = path.parent() {
        std::fs::create_dir_all(directory)
            .map_err(|error| format!("cannot create {}: {error}", directory.display()))?;
    }
    let file = std::fs::File::create(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(file),
        shot.size.width,
        shot.size.height,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&shot.rgba))
        .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(path.to_path_buf())
}

fn crop(shot: &iced::window::Screenshot, chrome: f32) -> Result<iced::window::Screenshot, String> {
    let trim = (chrome * shot.scale_factor).round().max(0.0) as u32;
    if trim == 0 {
        return Ok(shot.clone());
    }
    if trim >= shot.size.height {
        return Err("window is smaller than its chrome".to_owned());
    }
    shot.crop(iced::Rectangle {
        x: 0,
        y: 0,
        width: shot.size.width,
        height: shot.size.height - trim,
    })
    .map_err(|error| format!("cannot crop the screenshot: {error:?}"))
}

fn unique(directory: &Path, name: &str, stamp: &str) -> PathBuf {
    let mut path = directory.join(format!("{name}-{stamp}.png"));
    let mut index = 2;
    while path.exists() {
        path = directory.join(format!("{name}-{stamp}-{index}.png"));
        index += 1;
    }
    path
}

/// Lowercase ASCII words joined by `-`; never empty.
fn slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for character in name.chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "rawr".to_owned()
    } else {
        slug
    }
}

/// `YYYYMMDD-HHMMSS` in UTC.
fn timestamp(time: std::time::SystemTime) -> String {
    let seconds = time
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs());
    let (days, rest) = (seconds / 86_400, seconds % 86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}-{:02}{:02}{:02}",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, UNIX_EPOCH};

    #[test]
    fn screenshot_names_paths_and_crops_are_predictable() {
        assert_eq!(super::slug("kit / signal"), "kit-signal");
        assert_eq!(super::slug("  RawrGallery!  "), "rawrgallery");
        assert_eq!(super::slug("···"), "rawr");
        assert_eq!(super::timestamp(UNIX_EPOCH), "19700101-000000");
        assert_eq!(
            super::timestamp(UNIX_EPOCH + Duration::from_secs(1_792_400_005)),
            "20261019-085325"
        );
        assert_eq!(
            super::timestamp(UNIX_EPOCH + Duration::from_secs(951_782_400)),
            "20000229-000000"
        );

        let root = std::env::temp_dir().join(format!("rawr-screenshot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let rgba: Vec<u8> = (0..4 * 4 * 6).map(|index| index as u8).collect();
        let shot = iced::window::Screenshot::new(rgba, iced::Size::new(4, 6), 2.0);
        let first = super::save(&root, "kit / signal", &shot, 1.0).unwrap();
        let second = super::save(&root, "kit / signal", &shot, 1.0).unwrap();
        assert_eq!(first.parent(), Some(root.join("screenshots").as_path()));
        let name = first.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.starts_with("kit-signal-") && name.ends_with(".png"),
            "{name}"
        );
        assert_ne!(first, second);

        let decoder = png::Decoder::new(std::io::BufReader::new(
            std::fs::File::open(&first).unwrap(),
        ));
        let reader = decoder.read_info().unwrap();
        let info = reader.info();
        assert_eq!((info.width, info.height), (4, 4));
        assert!(super::save(&root, "tiny", &shot, 3.0).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
