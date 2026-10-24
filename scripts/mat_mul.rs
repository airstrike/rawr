//! This script simulates multi-dimensional matrix calculations.

use std::time::Instant;

const SIZE: usize = 50;

fn new_mat(x: usize, y: usize) -> Vec<Vec<f64>> {
    vec![vec![0.0; y]; x]
}

fn mat_gen() -> Vec<Vec<f64>> {
    let n = SIZE;
    let tmp = 1.0 / n as f64 / n as f64;
    let mut m = new_mat(n, n);

    for i in 0..n {
        for j in 0..n {
            m[i][j] = tmp * (i as f64 - j as f64) * (i as f64 + j as f64);
        }
    }

    m
}

fn mat_mul(a: &Vec<Vec<f64>>, b: &Vec<Vec<f64>>) -> Vec<Vec<f64>> {
    let mut b2 = new_mat(a[0].len(), b[0].len());

    for i in 0..a[0].len() {
        for j in 0..b[0].len() {
            b2[j][i] = b[i][j];
        }
    }

    let mut c = new_mat(a.len(), b[0].len());

    for i in 0..c.len() {
        for j in 0..c[i].len() {
            c[i][j] = 0.0;

            for z in 0..a[i].len() {
                c[i][j] += a[i][z] * b2[j][z];
            }
        }
    }

    c
}

let now = Instant::now();

let a = mat_gen();
let b = mat_gen();
let c = mat_mul(&a, &b);

/*
for i in 0..SIZE {
    println!("{:?}", c[i]);
}
*/

println!("c[1][2] = {}", c[1][2]);
println!("Finished. Run time = {:?}.", now.elapsed());
