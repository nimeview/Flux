#[derive(Clone, Debug)]
pub enum Expr {
    Number(f64),
    Var(String),
    Binary(Box<Expr>, Op, Box<Expr>),
}

#[derive(Clone, Copy, Debug)]
pub enum Op { Add, Sub, Mul, Div }

#[derive(Clone, Debug)]
pub enum Stmt {
    Signal(String, Expr),
    Reactive(String, Expr),
    Set(String, Expr),
    Print(String),
}
