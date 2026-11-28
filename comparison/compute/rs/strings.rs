fn main() {
    let parts: Vec<String> = (0..200000).map(|i| format!("item{}", i % 1000)).collect();
    let text = parts.join(",");
    let words: Vec<&str> = text.split(',').collect();
    let total: usize = words.iter().filter(|w| w.ends_with('7')).map(|w| w.len()).sum();
    println!("{} {}", words.len(), total);
}
