// rawr
// A crate's struct built with a field under `#[cfg(..)]`, as iced's
// `window::Settings { #[cfg(target_os = "macos")] platform_specific: ..,
// ..Default::default() }` is: the field is set where its cfg holds and
// comes from the base where it doesn't. A match arm and an array element
// under `#[cfg(..)]` are kept or left out the same way, as rustc
// configures them for the target Cargo builds for.

use events::custom::Quad;

fn main() {
    let quad = Quad {
        size: 3,
        #[cfg(unix)]
        radius: 4,
        #[cfg(windows)]
        radius: 5,
        ..Default::default()
    };
    println!("quad {} {}", quad.size, quad.radius);
    let sizes = [
        quad.size,
        #[cfg(target_os = "nowhere")]
        9,
        quad.radius,
    ];
    println!("sizes {}", sizes.len());
    let family = match quad.radius {
        #[cfg(unix)]
        4 => "unix",
        #[cfg(windows)]
        5 => "windows",
        _ => "neither",
    };
    println!("{family} {}", cfg!(target_pointer_width = "64"));
}
