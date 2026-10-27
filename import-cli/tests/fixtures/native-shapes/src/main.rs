// rawr
//! A marked crate that reaches items as upstream iced examples reach their
//! crates' (T204): color_palette's `Rgb { .. }` literal with
//! `..Rgb::default()`, multitouch's `from_tuple(&(..), &(..), &points)`,
//! screenshot's `save_buffer(.., &bytes, ..)` and changelog's native
//! `Result<(Self, Vec<Contribution>), Error>`.

mod listing;

use listing::Listing;
use shapes::Rgb;

fn to_rgb(red: f32, green: f32, blue: f32) -> Rgb {
    Rgb {
        red,
        green,
        blue,
        ..Rgb::default()
    }
}

fn main() {
    let rgb = to_rgb(0.25, 0.5, 1.0);
    println!("rgb {} {} {}", rgb.red, rgb.green, rgb.blue);

    let points: Vec<(f64, f64)> = vec![(1.0, 2.0), (3.0, 4.0), (9.0, 9.0)];
    let inside = shapes::inside(&(0.0, 0.0), &(5.0, 5.0), &points);
    println!("inside {:?} of {}", inside, points.len());
    println!(
        "outside {:?}",
        shapes::inside(&(5.0, 5.0), &(0.0, 0.0), &points)
    );

    let bytes: Vec<u8> = vec![1, 2, 3, 250];
    println!("checksum {}", shapes::checksum(&bytes));
    println!("bytes {:?}", bytes);

    match Listing::split("alpha beta gamma") {
        Ok((listing, entries)) => {
            println!("listing {}", listing.len());
            for entry in entries {
                println!("entry {} {}", entry.size, entry.name);
            }
        }
        Err(error) => println!("error {:?}", error),
    }
    match Listing::split("") {
        Ok((listing, _)) => println!("listing {}", listing.len()),
        Err(error) => println!("error {:?}", error),
    }
}
