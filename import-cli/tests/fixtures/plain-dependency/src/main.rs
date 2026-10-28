// rawr
// A program with one Cargo dependency and no GUI: rawr-import runs it in a
// runner that links the rawr crate and semver, and nothing else.
use semver::Version;

fn main() {
    let version = Version::parse("1.2.3").unwrap();
    println!("parsed: {version}");
    println!("major: {}", version.major);
    // A `u64` above `i64::MAX` crosses both ways.
    let widest = Version::new(u64::MAX, 0, 9223372036854775808);
    println!("widest: {widest}");
}
