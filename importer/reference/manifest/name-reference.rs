use std::io::{self, BufRead};
mod util {
    pub mod frontmatter {
        include!(concat!(env!("RAWR_CARGO_SOURCE"), "/src/cargo/util/frontmatter.rs"));
    }
}
mod embedded {
    include!(concat!(env!("RAWR_CARGO_SOURCE"), "/src/cargo/util/toml/embedded.rs"));
}
fn main() {
    for line in io::stdin().lock().lines() {
        let source: String = serde_json::from_str(&line.unwrap()).unwrap();
        let output = embedded::sanitize_name(&source);
        println!("{}", serde_json::json!({"source":source,"output":output}));
    }
}
