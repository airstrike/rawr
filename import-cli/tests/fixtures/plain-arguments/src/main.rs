// rawr
// A marked `main` with a Cargo dependency: it reads its arguments and
// stdin, and runs until stdin ends.
use semver::Version;

const LABEL: &str = "major";

fn main() {
    let arguments: Vec<String> = std::env::args().collect();
    println!("name {}", arguments[0]);
    for argument in arguments.iter().skip(1) {
        println!("arg {argument}");
    }
    let stdin = std::io::stdin();
    let mut line = String::new();
    loop {
        line.clear();
        if stdin.read_line(&mut line).unwrap() == 0 {
            break;
        }
        let version = Version::parse(line.trim()).unwrap();
        println!("{LABEL} {}", version.major);
    }
    println!("done");
}
