// rawr
// iced's pokedex's run, with a local server in reqwest's shape for the
// network and `wire::perform` for `Task::perform`: `search` joins the
// entry, decoded from nested `Deserialize` structs, and the image, on one
// of tokio's worker threads, and a failed request becomes the program's
// `Error` through its `From` impl.

use futures::future;

#[derive(Debug)]
enum Pokedex {
    Loading,
    Loaded { pokemon: Pokemon },
    Errored,
}

#[derive(Debug, Clone)]
enum Message {
    PokemonFound(Result<Pokemon, Error>),
    Search,
}

impl Pokedex {
    fn title(&self) -> String {
        let subtitle = match self {
            Pokedex::Loading => "Loading",
            Pokedex::Loaded { pokemon, .. } => &pokemon.name,
            Pokedex::Errored => "Whoops!",
        };

        format!("{subtitle} - Pokédex")
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::PokemonFound(Ok(pokemon)) => {
                *self = Pokedex::Loaded { pokemon };
            }
            Message::PokemonFound(Err(_error)) => {
                *self = Pokedex::Errored;
            }
            Message::Search => {
                *self = Pokedex::Loading;
            }
        }
    }
}

#[derive(Debug, Clone)]
struct Pokemon {
    number: u16,
    name: String,
    description: String,
    image: wire::Handle,
}

impl Pokemon {
    const TOTAL: u16 = 807;

    async fn search() -> Result<Pokemon, Error> {
        use rand::Rng;
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

        let id = {
            let mut rng = rand::rngs::OsRng;

            rng.gen_range(0..Pokemon::TOTAL)
        };

        let fetch_entry = async {
            let url = format!("https://pokeapi.co/api/v2/pokemon-species/{id}");

            wire::get(&url).await?.json().await
        };

        let (entry, image): (Entry, _) =
            future::try_join(fetch_entry, Self::fetch_image(id)).await?;

        let description = entry
            .flavor_text_entries
            .iter()
            .find(|text| text.language.name == "en")
            .ok_or(Error::LanguageError)?;

        Ok(Pokemon {
            number: id,
            name: entry.name.to_uppercase(),
            description: description
                .flavor_text
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect(),
            image,
        })
    }

    async fn fetch_image(id: u16) -> Result<wire::Handle, wire::Error> {
        let url = format!(
            "https://raw.githubusercontent.com/PokeAPI/sprites/master/sprites/pokemon/{id}.png"
        );

        #[cfg(not(target_arch = "wasm32"))]
        {
            let bytes = wire::get(&url).await?.bytes().await?;

            Ok(wire::Handle::from_bytes(bytes))
        }

        #[cfg(target_arch = "wasm32")]
        Ok(wire::Handle::from_bytes(url))
    }
}

#[derive(Debug, Clone)]
enum Error {
    APIError,
    LanguageError,
}

impl From<wire::Error> for Error {
    fn from(error: wire::Error) -> Error {
        dbg!(error);

        Error::APIError
    }
}

#[tokio::main]
async fn main() {
    let mut pokedex = Pokedex::Loading;
    println!("{}", pokedex.title());
    let found = wire::perform(Pokemon::search(), Message::PokemonFound)
        .await
        .unwrap();
    pokedex.update(found);
    println!("{}", pokedex.title());
    if let Pokedex::Loaded { pokemon } = &pokedex {
        println!(
            "{} {}",
            pokemon.number < Pokemon::TOTAL,
            pokemon.description
        );
        println!("{:?}", pokemon.image);
    }
    pokedex.update(Message::Search);
    println!("{}", pokedex.title());
    wire::outage();
    let found = wire::perform(Pokemon::search(), Message::PokemonFound)
        .await
        .unwrap();
    println!("{:?}", found);
    pokedex.update(found);
    println!("{}", pokedex.title());
}
