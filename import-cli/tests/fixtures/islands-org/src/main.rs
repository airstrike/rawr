//! Native `main`, as inboard's: it builds the config the marked inbox
//! reads.
mod workspace;

use model::{Id, Organization, User};

pub struct Config {
    pub org: Organization,
}

fn main() {
    let mut org = Organization::default();
    org.insert_user(User {
        id: Id(1),
        name: "Ada".to_owned(),
    });
    let config = Config { org };
    println!("{}", workspace::inbox::inviter(&config, Id(1)));
    println!("{}", workspace::inbox::inviter(&config, Id(2)));
}
