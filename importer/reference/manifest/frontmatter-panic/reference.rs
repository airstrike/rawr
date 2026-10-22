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
        let result = std::panic::catch_unwind(|| embedded::expand_manifest(&source));
        let value = match result {
            Ok(Ok(output)) => serde_json::json!({"source":source,"output":output}),
            Ok(Err(error)) => serde_json::json!({"source":source,"error":error.message(),"primary":error.primary_span(),"visible":error.visible_spans()}),
            Err(_) => serde_json::json!({"source":source,"panic":true}),
        };
        println!("{value}");
    }
}
