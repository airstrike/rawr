use rawr::preview::Seed;
use rawr::Engine;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut engine = Engine::new();
    rawr_iced::install(&mut engine)?;
    let descriptor =
        engine.compile_application_descriptor(include_str!("scripts/preview_metadata.rs"))?;
    let repeated =
        engine.compile_application_descriptor(include_str!("scripts/preview_metadata.rs"))?;

    println!(
        "deterministic application + preview proof: {}",
        descriptor.fingerprint() == repeated.fingerprint()
    );
    for preview in descriptor.previews() {
        let seed = match preview.seed() {
            Seed::None => "none",
            Seed::Default => "Default",
            Seed::Callback(callback) => callback.item().key().name(),
        };
        println!(
            "{} | group={} | size={}x{} | seed={} | snapshot={}",
            preview.key(),
            preview.group(),
            preview.size().width(),
            preview.size().height(),
            seed,
            preview.snapshot(),
        );
    }
    Ok(())
}
