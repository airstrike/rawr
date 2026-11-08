// rawr
use std::error::Error;
use std::io;

use natives::web::{error, DomBackend};

fn open(id: &str) -> io::Result<()> {
    let backend = DomBackend::new_by_id(id)?;
    drop(backend);
    Ok(())
}

fn boxed(id: &str) -> Result<(), Box<dyn Error>> {
    DomBackend::new_by_id(id)?;
    Ok(())
}

fn main() {
    println!("{}", open("page").is_ok());
    println!("{}", open("").unwrap_err());
    println!("{}", boxed("").unwrap_err());
    let error: Box<dyn Error> = Box::new(error::Error::UnableToRetrieveElementById("x".to_owned()));
    println!("{error}");
}
