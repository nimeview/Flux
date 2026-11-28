"""The same seven mistakes in Flux, Rust, TypeScript, Python and Elm."""

ELM_HEAD = "port module Main exposing (main)\n\nimport Platform\n\n\nport out : Int -> Cmd msg\n\n\n"
ELM_MAIN = "\n\nmain : Program () () ()\nmain =\n    Platform.worker { init = \\_ -> ( (), out (run ()) ), update = \\_ m -> ( m, Cmd.none ), subscriptions = \\_ -> Sub.none }\n"

CASES = {
 "type_mismatch": {
  "what": "Passing a string where a number is expected",
  "fl": 'import io { println }\n\nfn add(a: Int, b: Int) -> Int {\n    a + b\n}\n\nfn main() {\n    println("{add(1, "2")}")\n}\n',
  "rs": 'fn add(a: i64, b: i64) -> i64 {\n    a + b\n}\n\nfn main() {\n    println!("{}", add(1, "2"));\n}\n',
  "ts": 'function add(a: number, b: number): number {\n    return a + b;\n}\n\nconsole.log(add(1, "2"));\n',
  "py": 'def add(a: int, b: int) -> int:\n    return a + b\n\nprint(add(1, "2"))\n',
  "elm": ELM_HEAD + 'add : Int -> Int -> Int\nadd a b =\n    a + b\n\n\nrun : () -> Int\nrun _ =\n    add 1 "2"\n' + ELM_MAIN,
 },
 "typo": {
  "what": "A misspelled variable name",
  "fl": 'import io { println }\n\nfn main() {\n    let total = 5\n    println("{totl}")\n}\n',
  "rs": 'fn main() {\n    let total = 5;\n    println!("{}", totl);\n}\n',
  "ts": 'const total = 5;\nconsole.log(totl);\n',
  "py": 'total = 5\nprint(totl)\n',
  "elm": ELM_HEAD + 'run : () -> Int\nrun _ =\n    let\n        total =\n            5\n    in\n    totl\n' + ELM_MAIN,
 },
 "nonexhaustive": {
  "what": "A match that forgets one case",
  "fl": 'import io { println }\n\nenum Shape {\n    Circle,\n    Square,\n    Triangle,\n}\n\nfn sides(s: Shape) -> Int {\n    match s {\n        Shape::Circle => 0,\n        Shape::Square => 4,\n    }\n}\n\nfn main() {\n    println("{sides(Shape::Triangle)}")\n}\n',
  "rs": 'enum Shape {\n    Circle,\n    Square,\n    Triangle,\n}\n\nfn sides(s: Shape) -> i64 {\n    match s {\n        Shape::Circle => 0,\n        Shape::Square => 4,\n    }\n}\n\nfn main() {\n    println!("{}", sides(Shape::Triangle));\n}\n',
  "ts": 'type Shape = "circle" | "square" | "triangle";\n\nfunction sides(s: Shape): number {\n    switch (s) {\n        case "circle": return 0;\n        case "square": return 4;\n    }\n}\n\nconsole.log(sides("triangle"));\n',
  "py": 'from enum import Enum\n\nclass Shape(Enum):\n    CIRCLE = 1\n    SQUARE = 2\n    TRIANGLE = 3\n\ndef sides(s: Shape) -> int:\n    match s:\n        case Shape.CIRCLE:\n            return 0\n        case Shape.SQUARE:\n            return 4\n\nprint(sides(Shape.TRIANGLE))\n',
  "elm": ELM_HEAD + 'type Shape\n    = Circle\n    | Square\n    | Triangle\n\n\nsides : Shape -> Int\nsides s =\n    case s of\n        Circle ->\n            0\n\n        Square ->\n            4\n\n\nrun : () -> Int\nrun _ =\n    sides Triangle\n' + ELM_MAIN,
 },
 "no_method": {
  "what": "Calling a method that does not exist (typo in a method name)",
  "fl": 'import io { println }\n\nfn main() {\n    let xs = [1, 2, 3]\n    println("{xs.lenght()}")\n}\n',
  "rs": 'fn main() {\n    let xs = vec![1, 2, 3];\n    println!("{}", xs.lenght());\n}\n',
  "ts": 'const xs = [1, 2, 3];\nconsole.log(xs.lenght);\n',
  "py": 'xs = [1, 2, 3]\nprint(xs.lenght())\n',
  "elm": ELM_HEAD + 'run : () -> Int\nrun _ =\n    List.lenght [ 1, 2, 3 ]\n' + ELM_MAIN,
 },
 "immutable": {
  "what": "Assigning to an immutable binding",
  "fl": 'import io { println }\n\nfn main() {\n    let count = 1\n    count = 2\n    println("{count}")\n}\n',
  "rs": 'fn main() {\n    let count = 1;\n    count = 2;\n    println!("{}", count);\n}\n',
  "ts": 'const count = 1;\ncount = 2;\nconsole.log(count);\n',
  "py": 'from typing import Final\ncount: Final = 1\ncount = 2\nprint(count)\n',
  "elm": ELM_HEAD + 'run : () -> Int\nrun _ =\n    let\n        count =\n            1\n\n        count =\n            2\n    in\n    count\n' + ELM_MAIN,
 },
 "arity": {
  "what": "Calling a function with too few arguments",
  "fl": 'import io { println }\n\nfn area(w: Int, h: Int) -> Int {\n    w * h\n}\n\nfn main() {\n    println("{area(3)}")\n}\n',
  "rs": 'fn area(w: i64, h: i64) -> i64 {\n    w * h\n}\n\nfn main() {\n    println!("{}", area(3));\n}\n',
  "ts": 'function area(w: number, h: number): number {\n    return w * h;\n}\n\nconsole.log(area(3));\n',
  "py": 'def area(w: int, h: int) -> int:\n    return w * h\n\nprint(area(3))\n',
  "elm": ELM_HEAD + 'area : Int -> Int -> Int\narea w h =\n    w * h\n\n\nrun : () -> Int\nrun _ =\n    area 3\n' + ELM_MAIN,
 },
 "optional": {
  "what": "Using a value that may be missing as if it were there",
  "fl": 'import io { println }\n\nfn main() {\n    let xs = [3, 5, 8]\n    let first_even = xs.find(|x| x % 2 == 0)\n    println("{first_even + 1}")\n}\n',
  "rs": 'fn main() {\n    let xs = vec![3, 5, 8];\n    let first_even = xs.iter().find(|x| *x % 2 == 0);\n    println!("{}", first_even + 1);\n}\n',
  "ts": 'const xs = [3, 5, 8];\nconst firstEven = xs.find((x) => x % 2 === 0);\nconsole.log(firstEven + 1);\n',
  "py": 'xs = [3, 5, 7]\nfirst_even = next((x for x in xs if x % 2 == 0), None)\nprint(first_even + 1)\n',
  "elm": ELM_HEAD + 'run : () -> Int\nrun _ =\n    List.head (List.filter (\\x -> modBy 2 x == 0) [ 3, 5, 8 ]) + 1\n' + ELM_MAIN,
 },
}
