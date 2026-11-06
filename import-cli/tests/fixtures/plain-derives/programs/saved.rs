// rawr
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Default)]
enum TaskState {
    #[default]
    Idle,
    Editing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Task {
    #[serde(default = "Uuid::new_v4")]
    id: Uuid,
    description: String,
    completed: bool,
    #[serde(skip)]
    state: TaskState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
enum Filter {
    #[default]
    All,
    Active,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SavedState {
    input_value: String,
    #[serde(skip_deserializing)]
    filter: Filter,
    tasks: Vec<Task>,
}

fn main() {
    let id = Uuid::parse_str("67e55044-10b1-426f-9247-bb680e5fe0c8").unwrap();
    let saved = SavedState {
        input_value: "pie".to_string(),
        filter: Filter::Active,
        tasks: vec![Task {
            id,
            description: "write".to_string(),
            completed: true,
            state: TaskState::Editing,
        }],
    };
    let json = serde_json::to_string_pretty(&saved).unwrap();
    println!("{json}");
    let back: SavedState = serde_json::from_str(&json).unwrap();
    println!("{back:?}");
    let fresh: Task =
        serde_json::from_str("{\"description\":\"new\",\"completed\":false}").unwrap();
    println!("{} {:?} {}", fresh.id != id, fresh.state, fresh.description);
    println!("{}", serde_json::to_string(&Filter::All).unwrap());
}
