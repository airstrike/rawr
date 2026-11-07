//! Native `main`: says whether it and the marked code it calls run on the
//! process's main thread, where winit's event loop must run, and calls the
//! marked module `answer`, once with a `u64` past `i64::MAX`, and for
//! results of its own types and of a native one.
mod answer;
mod thread;

fn main() {
    println!("main thread {}", thread::on_main_thread());
    println!("marked main thread {}", answer::on_main_thread());
    println!("answer {}", answer::answer());
    println!("wide {}", answer::wide(0xC0FF_EEDE_ADBE_EF00));
    println!("checked {:?}", answer::checked(3));
    println!("checked {:?}", answer::checked(0));
    println!("run {:?}", answer::run(false));
    println!("run {:?}", answer::run(true));
}
