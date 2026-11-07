//! Native `main`: prints what the workspace's packages give it and what its
//! two marked modules compute from them.
mod badge;
mod panel;

mod icons {
    include!(concat!(env!("OUT_DIR"), "/icons.rs"));
}

fn main() {
    println!("icons {}", icons::ICONS.join(","));
    println!("logo {}", common::logo());
    println!("sibling {}", sibling::greeting());
    println!("panel {}", panel::title());
    println!("badge {}", badge::count(4));
}
