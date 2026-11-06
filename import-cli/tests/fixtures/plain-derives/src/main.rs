// rawr
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Task {
    description: String,
    done: bool,
}

fn main() {
    let task = Task {
        description: "write".to_string(),
        done: false,
    };
    let json = serde_json::to_string(&task).unwrap();
    println!("{json}");
    let back: Task = serde_json::from_str(&json).unwrap();
    println!("{back:?}");
}
