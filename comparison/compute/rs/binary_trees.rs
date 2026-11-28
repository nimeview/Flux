enum Tree { Leaf, Node(Box<Tree>, Box<Tree>) }
fn make(d: u32) -> Tree {
    if d == 0 { Tree::Node(Box::new(Tree::Leaf), Box::new(Tree::Leaf)) }
    else { Tree::Node(Box::new(make(d - 1)), Box::new(make(d - 1))) }
}
fn check(t: &Tree) -> i64 { match t { Tree::Leaf => 0, Tree::Node(l, r) => 1 + check(l) + check(r) } }
fn main() {
    let max_depth = 16;
    let mut total = check(&make(max_depth + 1));
    let long_lived = make(max_depth);
    let mut depth = 4;
    while depth <= max_depth {
        let iterations = 1 << (max_depth - depth + 4);
        for _ in 0..iterations { total += check(&make(depth)); }
        depth += 2;
    }
    total += check(&long_lived);
    println!("{total}");
}
