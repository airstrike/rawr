let list = [1, 2, 3];
let mut list_vec = Vec::new();

for idx in list.into_iter() {
    list_vec.push(if idx == 1 {
        text("First! Привет :)")
    } else {
        text(idx)
    });
}

column(list_vec)
