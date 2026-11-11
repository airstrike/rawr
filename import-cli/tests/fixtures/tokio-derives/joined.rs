// rawr
// futures' `try_join` of an async block and an async fn's future, as iced's
// pokedex's `search` joins its entry and its image: `Fut1: TryFuture`'s
// `Ok` and `Error` are what each future gives inside its `Result`.

use futures::future;

async fn size(text: &'static str) -> Result<usize, String> {
    Ok(text.len())
}

async fn search(body: &'static str) -> Result<String, String> {
    let fetch = async { wire::json(body).await };
    let ((name, _), length): ((String, bool), usize) =
        future::try_join(fetch, size("seed")).await?;
    Ok(format!("{} {length}", name.to_uppercase()))
}

#[tokio::main]
async fn main() {
    println!("{:?}", search(r#""bulbasaur""#).await);
    println!("{:?}", search("1").await);
}
