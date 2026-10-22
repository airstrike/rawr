pub fn keys(hash: impl Fn(&str) -> u64) -> Vec<String> {
    let mut keys = Vec::new();
    let mut candidate = 0;
    while keys.len() < 112 {
        let key = format!("long-shared-prefix-collision-{candidate}");
        if hash(&key) & 127 == 0 { keys.push(key); }
        candidate += 1;
    }
    keys.extend((112..160).map(|i| format!("key-{i:03}")));
    keys
}

// i = conversion insert; m = merge entry; r = removal.
pub fn operations() -> Vec<(char, usize, usize)> {
    let mut out = Vec::new();
    for i in 0..112 {
        out.push(('i', i, i));
        out.push(('m', i, 1000 + i));
    }
    for i in 0..104 { out.push(('r', i, 0)); }
    for i in 112..160 { out.push(('m', i, i)); }
    for round in 0..3 {
        for i in 0..96 { out.push(('m', i, 2000 + round * 100 + i)); }
        for i in 0..96 { out.push(('r', i, 0)); }
    }
    for i in 0..32 {
        out.push(('i', i, i));
        out.push(('i', i, 3000 + i));
    }
    out
}
