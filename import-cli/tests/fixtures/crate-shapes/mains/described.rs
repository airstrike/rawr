// rawr
use natives::described::Owned;

fn main() {
    let owned = Owned { label: 4, count: 2 };
    let Owned { label, count } = owned.clone();
    println!("{label} {count}");
    let named = natives::described::Labeled {
        label: Some("gpu"),
        count: 3,
    };
    let natives::Labeled { count, .. } = named;
    println!("{count}");
    println!(
        "{}",
        owned == natives::described::Described { label: 4, count: 2 }
    );
    let adapter = natives::described::Adapter;
    let (one, two) = natives::described::block_on(async {
        adapter
            .request(&natives::Labeled {
                label: None,
                count: 5,
            })
            .await
            .expect("request")
    });
    println!("{one} {two}");
}
