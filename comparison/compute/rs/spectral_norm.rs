fn a(i: usize, j: usize) -> f64 { 1.0 / ((i + j) * (i + j + 1) / 2 + i + 1) as f64 }
fn mul_av(v: &[f64], n: usize) -> Vec<f64> { (0..n).map(|i| (0..n).map(|j| a(i, j) * v[j]).sum()).collect() }
fn mul_atv(v: &[f64], n: usize) -> Vec<f64> { (0..n).map(|i| (0..n).map(|j| a(j, i) * v[j]).sum()).collect() }
fn main() {
    let n = 250;
    let mut u = vec![1.0; n];
    let mut v = Vec::new();
    for _ in 0..10 { v = mul_atv(&mul_av(&u, n), n); u = mul_atv(&mul_av(&v, n), n); }
    let vbv: f64 = (0..n).map(|j| u[j] * v[j]).sum();
    let vv: f64 = (0..n).map(|j| v[j] * v[j]).sum();
    println!("{}", ((vbv / vv).sqrt() * 1e9) as i64);
}
