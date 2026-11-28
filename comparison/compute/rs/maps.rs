use std::collections::HashMap;
fn main() {
    let mut counts: HashMap<String, i64> = HashMap::new();
    for i in 0..200000i64 { *counts.entry(format!("k{}", i % 5000)).or_insert(0) += i % 7; }
    let total: i64 = counts.values().sum();
    println!("{} {}", counts.len(), total);
}
