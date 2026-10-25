//! Animated renders through the `rawr-play` CLI: a full app runs on a
//! simulated clock, and a view that changes becomes a GIF.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// What the CLI wrote for one fixture.
struct Rendered {
    bytes: Vec<u8>,
    stdout: String,
}

/// Render `tests/fixtures/{name}.rs` to `target/play/motion-test/{output}`,
/// with a time budget a debug build can meet.
fn render(name: &str, output: &str, args: &[&str]) -> Rendered {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("../target/play/motion-test");
    std::fs::create_dir_all(&dir).unwrap();
    let stem = output.rsplit_once('.').map_or(output, |(stem, _)| stem);
    for extension in ["png", "gif"] {
        let _ = std::fs::remove_file(dir.join(format!("{stem}.{extension}")));
    }
    let result = Command::new(env!("CARGO_BIN_EXE_rawr-play"))
        .arg(root.join(format!("tests/fixtures/{name}.rs")))
        .args(["--time-ms", "120000"])
        .args(args)
        .arg("-o")
        .arg(dir.join(output))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&result.stdout).into_owned();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let written = stdout.split(" (").next().unwrap().trim();
    Rendered {
        bytes: std::fs::read(written).unwrap(),
        stdout,
    }
}

/// A GIF's frames and their delays, and whether it loops.
struct Decoded {
    delays: Vec<Duration>,
    looping: bool,
}

fn decode(bytes: &[u8]) -> Decoded {
    assert!(bytes.starts_with(b"GIF89a"), "not a GIF");
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(bytes).unwrap();
    let mut delays = Vec::new();
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        delays.push(Duration::from_millis(u64::from(frame.delay) * 10));
    }
    let looping = bytes.windows(11).any(|window| window == b"NETSCAPE2.0");
    Decoded { delays, looping }
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.starts_with(b"\x89PNG\r\n\x1a\n")
}

#[test]
fn motion_an_easing_plays_once_and_holds_its_last_frame() {
    let rendered = render("ease", "ease.png", &[]);
    let gif = decode(&rendered.bytes);
    assert!(!gif.looping, "a settled animation plays once");
    assert!(gif.delays.len() > 10, "{} frames", gif.delays.len());
    assert!(
        *gif.delays.last().unwrap() >= Duration::from_secs(3),
        "the last frame holds"
    );
    // One second of easing at 20 frames a second, then the hold.
    let total: Duration = gif.delays.iter().sum();
    assert_eq!(total, Duration::from_millis(4000), "{:?}", gif.delays);
    assert!(!rendered.stdout.contains("note:"), "{}", rendered.stdout);
}

#[test]
fn motion_a_timed_easing_moves_by_the_frame_times_and_renders_the_same_twice() {
    // `application::timed`'s update takes each frame's Instant from the
    // recording's clock.
    let args = ["--message", "Message::Toggle"];
    let first = render("timed", "timed.png", &args);
    let gif = decode(&first.bytes);
    assert!(!gif.looping, "a settled animation plays once");
    // The boot's frame before the message, one second of easing at 20
    // frames a second, then the hold.
    assert!(gif.delays.len() > 10, "{} frames", gif.delays.len());
    assert!(gif.delays[1..gif.delays.len() - 1]
        .iter()
        .all(|delay| *delay == Duration::from_millis(50)));
    let total: Duration = gif.delays.iter().sum();
    assert_eq!(total, Duration::from_millis(5000), "{:?}", gif.delays);
    let second = render("timed", "timed.png", &args);
    assert_eq!(first.bytes, second.bytes);
}

#[test]
fn motion_a_spinner_loops_once_around() {
    let rendered = render("spinner", "spinner.png", &[]);
    let gif = decode(&rendered.bytes);
    assert!(gif.looping, "a periodic animation loops");
    // Twelve positions, a tenth of a second each: one turn, no hold.
    assert_eq!(gif.delays, vec![Duration::from_millis(100); 12]);
    assert!(!rendered.stdout.contains("note:"), "{}", rendered.stdout);
}

#[test]
fn motion_a_loop_lasts_one_period() {
    // The dots' view first changes at 250 ms. That frame is drawn at the
    // GIF's scale too, so the loop starts there and loses no step.
    let rendered = render("dots", "dots.png", &[]);
    let gif = decode(&rendered.bytes);
    assert!(gif.looping, "a periodic animation loops");
    let total: Duration = gif.delays.iter().sum();
    assert_eq!(total, Duration::from_secs(1), "{:?}", gif.delays);
    assert_eq!(gif.delays, vec![Duration::from_millis(250); 4]);
}

#[test]
fn motion_a_stopwatch_plays_once_with_a_note() {
    let rendered = render("tenths", "tenths.png", &[]);
    let gif = decode(&rendered.bytes);
    assert!(!gif.looping, "an animation with no loop plays once");
    assert!(*gif.delays.last().unwrap() >= Duration::from_secs(3));
    // 100 frames 50 ms apart, the last one held.
    let total: Duration = gif.delays.iter().sum();
    assert_eq!(
        total,
        Duration::from_millis(4950 + 3000),
        "{:?}",
        gif.delays
    );
    assert!(
        rendered
            .stdout
            .contains("note: Animation shown for the first 5 s."),
        "{}",
        rendered.stdout
    );
}

#[test]
fn motion_a_counter_stays_a_png_until_its_messages_change_it() {
    let still = render("counter", "counter.png", &[]);
    assert!(is_png(&still.bytes), "{}", still.stdout);
    let increments = [
        "--message",
        "Message::Increment",
        "--message",
        "Message::Increment",
    ];
    let stepped = render("counter", "counter_steps.png", &increments);
    let gif = decode(&stepped.bytes);
    assert!(!gif.looping);
    // The boot, then each message's view, a second each, and the hold.
    assert_eq!(
        gif.delays,
        [
            Duration::from_secs(1),
            Duration::from_secs(1),
            Duration::from_secs(3)
        ]
    );
    let mut args = increments.to_vec();
    args.push("--still");
    assert!(is_png(&render("counter", "counter_still.png", &args).bytes));
}

#[test]
fn motion_the_playgrounds_animated_iced_examples_are_gifs() {
    // iced's arc and sandpiles move on their own; its stopwatch moves once
    // the playground's `Message::Toggle` starts it.
    for (name, args) in [
        ("arc", &[][..]),
        ("sandpiles", &[]),
        ("stopwatch", &["--message", "Message::Toggle"]),
    ] {
        let rendered = render(name, &format!("example_{name}.png"), args);
        let gif = decode(&rendered.bytes);
        assert!(gif.delays.len() > 10, "{name}: {} frames", gif.delays.len());
    }
}

/// A GIF's frames as whole RGBA images, each one's change drawn over the
/// frame before, also written as PNGs to `target/play-fix/{name}-{i}.png`
/// to look at.
fn pictures(name: &str, bytes: &[u8]) -> Vec<Vec<u8>> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options.read_info(bytes).unwrap();
    let (width, height) = (usize::from(decoder.width()), usize::from(decoder.height()));
    let mut canvas = vec![0_u8; width * height * 4];
    let mut pictures = Vec::new();
    while let Some(frame) = decoder.read_next_frame().unwrap() {
        let (left, top) = (usize::from(frame.left), usize::from(frame.top));
        for (at, pixel) in frame.buffer.as_chunks::<4>().0.iter().enumerate() {
            if pixel[3] == 0 {
                continue;
            }
            let (x, y) = (
                left + at % usize::from(frame.width),
                top + at / usize::from(frame.width),
            );
            let to = (y * width + x) * 4;
            canvas[to..to + 4].copy_from_slice(pixel);
        }
        pictures.push(canvas.clone());
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/play-fix");
    std::fs::create_dir_all(&dir).unwrap();
    for (at, picture) in pictures.iter().enumerate() {
        let file = std::fs::File::create(dir.join(format!("{name}-{at}.png"))).unwrap();
        let mut encoder = png::Encoder::new(file, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(picture)
            .unwrap();
    }
    pictures
}

/// How many pixels of `picture` are `color`.
fn count(picture: &[u8], color: &[u8]) -> usize {
    picture
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| *pixel == color)
        .count()
}

#[test]
fn motion_a_counters_buttons_stay_enabled_in_every_frame() {
    // The frame where the view first changes is drawn again at the GIF's
    // scale. Drawn from a fresh interface, its buttons showed disabled.
    let increments = [
        "--message",
        "Message::Increment",
        "--message",
        "Message::Increment",
    ];
    let rendered = render("counter", "counter_enabled.png", &increments);
    let pictures = pictures("counter", &rendered.bytes);
    assert_eq!(pictures.len(), 3);
    // The enabled buttons' fill: the boot frame's most common color after
    // the background in its corner.
    let first = &pictures[0];
    let background = &first[..4];
    let mut colors: Vec<&[u8; 4]> = first
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|pixel| *pixel != background)
        .collect();
    colors.sort_unstable();
    let fill = colors
        .chunk_by(|a, b| a == b)
        .max_by_key(|run| run.len())
        .unwrap()[0];
    let enabled = count(first, fill);
    for (at, picture) in pictures.iter().enumerate() {
        // Drawn at another scale, the edges differ a little; disabled
        // buttons lose the fill entirely.
        let shown = count(picture, fill);
        assert!(
            shown * 10 >= enabled * 9,
            "frame {at} shows its buttons disabled: {shown} of {enabled}"
        );
    }
}

#[test]
fn motion_a_theme_change_changes_the_colors() {
    let rendered = render(
        "styling",
        "styling_theme.png",
        &["--message", "Message::ThemeChanged(Theme::Dracula)"],
    );
    let pictures = pictures("styling", &rendered.bytes);
    let corner = |picture: &[u8]| picture[..4].to_vec();
    let (first, last) = (pictures.first().unwrap(), pictures.last().unwrap());
    assert_ne!(
        corner(first),
        corner(last),
        "the background follows the theme"
    );
    // Dracula's background.
    assert_eq!(corner(last), [0x28, 0x2a, 0x36, 0xff]);
}
