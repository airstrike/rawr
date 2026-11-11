// rawr
// A crate's type the program never names, reached as an async method's
// output through `?`, as pokedex's `fetch_image` reaches reqwest's `Bytes`,
// and its derived `Debug`.

async fn fetch() -> Result<String, wire::Error> {
    let bytes = wire::get("1.png").await?.bytes().await?;
    Ok(format!("{bytes:?}"))
}

#[tokio::main]
async fn main() {
    println!("{:?}", fetch().await);
}
