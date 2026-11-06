// rawr
// Structs a crate decodes on one of tokio's worker threads, defined in an
// async fn's body, as iced's pokedex decodes its `Entry` through reqwest.

const BODY: &str = r#"{"name":"bulbasaur","id":1,"flavor_text_entries":[
    {"flavor_text":"Une graine","language":{"name":"fr"}},
    {"flavor_text":"A strange\nseed","language":{"name":"en"}}]}"#;

async fn search() -> Result<String, String> {
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    struct Entry {
        name: String,
        flavor_text_entries: Vec<FlavorText>,
    }

    #[derive(Debug, Deserialize)]
    struct FlavorText {
        flavor_text: String,
        language: Language,
    }

    #[derive(Debug, Deserialize)]
    struct Language {
        name: String,
    }

    let broken: Result<(Entry, bool), String> = wire::json(r#"{"name":1}"#).await;
    println!("{broken:?}");
    let (entry, elsewhere): (Entry, bool) = wire::json(BODY).await?;
    let text = entry
        .flavor_text_entries
        .iter()
        .find(|text| text.language.name == "en")
        .ok_or(String::from("no English text"))?;
    let text: String = text
        .flavor_text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    Ok(format!("{} {text} {elsewhere}", entry.name.to_uppercase()))
}

#[tokio::main]
async fn main() {
    println!("{:?}", search().await);
}
