// rawr
// A driver generic over a type, at the opaque type a function's
// `Wrapper<impl SomeTrait>` holds: the pipe a generic function makes for
// it carries signals of that wrapper, and the driver takes the pipe and
// the wrapper.

use events::piped::{self, Driver};

fn main() {
    let stepper = piped::wrapped(3);
    let (pipe, mut drain) = piped::pipe_for(&stepper);
    let mut driver = Driver::new(pipe, &stepper);
    driver.press(&stepper, "go");
    driver.press(&stepper, "stay");
    driver.run(&stepper, "go");
    println!("told {}", drain.told().join(", "));
    println!("count {}", driver.count());
}
