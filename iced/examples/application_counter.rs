fn main() -> Result<(), Box<dyn std::error::Error>> {
    rawr_iced::application::Compiler::new()?
        .check_source(include_str!("scripts/application_counter.rs"))?
        .prepare()?
        .run()?;
    Ok(())
}
