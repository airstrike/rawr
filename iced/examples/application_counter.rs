fn main() -> Result<(), Box<dyn std::error::Error>> {
    rawr_iced::application::Compiler::new()?
        .check_source(include_str!("application_counter.rawr"))?
        .prepare()?
        .run()?;
    Ok(())
}
