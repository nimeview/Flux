#[derive(Clone, Debug)]
pub enum Expr {
    Number(f64),
    Bool(bool),
    String(String),
    Var(String),
    Tuple(Vec<Expr>),
    Binary(Box<Expr>, Op, Box<Expr>),
    Call { name: String, args: Vec<Expr> },
    If {
        cond: Box<Expr>,
        then_branch: Box<Expr>,
        else_branch: Box<Expr>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<(Pattern, Expr)>,
    },
    Lambda {
        params: Vec<String>,
        body: Box<Expr>,
    },
}

#[derive(Clone, Debug)]
pub enum Pattern {
    Wildcard,
    Var(String),
    Bool(bool),
    Number(f64),
}

#[derive(Clone, Copy, Debug)]
pub enum Op { Add, Sub, Mul, Div }

#[derive(Clone, Debug)]
pub enum Stmt {
    Package { name: String, root: String, exports: Vec<String> },
    Import { path: String, alias: Option<String> },
    Signal { name: String, expr: Expr, is_pub: bool },
    Reactive { name: String, expr: Expr, is_pub: bool },
    Set(String, Expr),
    FnDef { name: String, params: Vec<String>, param_types: Vec<Option<LangTypeRef>>, body: Expr, is_pub: bool },
    Expr(Expr),
}

#[derive(Clone, Debug)]
pub enum LangTypeRef { Number, Bool, String }
