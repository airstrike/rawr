// rawr
// A program with one Cargo dependency and no GUI: rawr-import runs it in a
// runner that links the rawr crate and semver, and nothing else.
use semver::Version;

fn main() {
    let version = Version::parse("1.2.3").unwrap();
    println!("parsed: {version}");
    println!("major: {}", version.major);
}
