# Tools

## flux run

```bash
flux run                 # the entry in flux.toml
flux run app.fl          # a specific file
flux run app.fl -- a b   # arguments, read with args()
flux run --jit app.fl    # the experimental JIT
flux run --quiet app.fl  # no banner
```

A program with `fn main()` runs it and owns its output. Its exit status is what `main` returns (0 to 255), or 0 for a `main` without a result. A runtime error exits with status 1.

A file without `main` initializes and then prints its reactive values, which is handy for experiments.

## flux check

```bash
flux check app.fl
flux check --target cli app.fl
flux check --target web app.fl
```

Checks imports and types without running the program. It accepts `extern fn` declarations; `flux run` provides no host functions and cannot start a program that declares one.

`--target cli|web` selects the target. The default is `cli`. Each target has its part of the library. `web` has no `fs`, `process`, or `env` module. Its `io` module has no `args`, `read_line`, `read_stdin`, `read_file`, `write_file`, `append_file`, `file_exists`, `read_bytes`, or `write_bytes`.

Importing a module the target lacks is E034: "`fs` is not available on target `web`". Naming an unavailable function in an import is also E034. With `import io`, using such a function is E034 where it is used.

An unknown target is an error: "unknown target `mars`; the targets are `cli` and `web`". `web` can only be checked. Nothing builds or runs for it yet.

## flux test

Any function named `test_*` with no parameters, in any `.fl` file under the project, is a test. It passes if it returns `Unit` or `true`, and fails on `false`, on any runtime error, or if its file does not compile. Each runs against a freshly initialized program.

```flux
import testing { assert, assert_eq }

fn add(a: Int, b: Int) -> Int {
    a + b
}

fn test_add() {
    assert_eq(add(1, 2), 3)
}

fn test_positive() {
    assert(add(1, 1) > 0)
}

fn test_as_a_bool() -> Bool {
    add(0, 0) == 0
}
```

```bash
flux test            # all tests
flux test add        # those whose file::name contains "add"
flux test --jit      # on the JIT
```

```text
test math_test.fl::test_add ... ok
test math_test.fl::test_as_a_bool ... ok
test math_test.fl::test_positive ... ok

test result: ok. 3 passed; 0 failed
```

The exit status is 1 if anything failed. `flux test` does not run timers.

## flux fmt

```bash
flux fmt app.fl          # print the formatted file
flux fmt app.fl --write  # rewrite it
flux fmt app.fl --check  # exit 1 if it would change
```

There are no options, so all Flux code looks the same. A `match` stays on one line when it fits in 100 columns. Number spellings such as `1_000` and `0xff` are kept.

## flux migrate

```bash
flux migrate app.fl          # print what would change
flux migrate app.fl --write  # rewrite it
flux migrate src --write     # every file in a directory
```

Rewrites older forms to the current syntax. It applies the replacement that each warning E026 names (`rfn` becomes `track fn`, `on Tick { n -> }` becomes `on Tick(n) { }`, `m.name` becomes `m::name`), and turns a `fn` that writes a source (E027) into an `act fn`.

It also adds `pub` to every field of every `pub struct`, preserving the visibility that all fields had before 0.4. Other structs and enum variants are left as they are. Run `flux migrate --write` to keep programs compiling as before, then remove `pub` where a field should be private.

It keeps comments and changes nothing else. Like the quick fixes in the editor, it is experimental.

## flux repl

An interactive shell. `:help` lists its commands. End of input (Ctrl-D, or the end of a piped script) ends the session.

`:emit Event v1 v2 …` fires an event with its values in declaration order, separated by spaces. A `"quoted string"` is one value:

```text
event Move(dx: Int, dy: Int)
:emit Move 1 2
event Label(text: String)
:emit Label "two words"
```

The number of values must match the declaration. Otherwise the command does nothing and reports, for example, "`Move` carries 2 values, but 1 was given: `:emit Move v v`". An undeclared event reports "unknown event `Nope`: declare it first, as `event Nope`".

A runtime error in a handler is printed, for example, "error: runtime error: integer division by zero in `on Go`". The session continues with the sources' values as they were when the error happened.

Each new source line compiles the whole session again. Sources keep their current values, copied by name when the type is the same, so adding a declaration does not reset them. A watch may run once more if a copied value differs from its initial value.

## Editors

**VS Code** has an extension in [`editors/vscode`](https://github.com/nimeview/Flux/tree/main/editors/vscode), with highlighting, snippets, run and test buttons, and everything the language server offers. Its README explains how to install it.

For other editors, `flux lsp-server` speaks the Language Server Protocol over standard input and output. It provides diagnostics, hover, completion with automatic imports, parameter hints, inlay type hints, go-to-definition, references, highlights, rename, formatting, document symbols, and a quick fix for missing imports. Point your editor's LSP client at it for `.fl` files:

```text
command: flux
args: ["lsp-server"]
filetypes: ["flux"]
```

## Build profiles

For working on Flux itself:

| Profile | Command | Use |
|---|---|---|
| `dev` | `cargo build` | everyday work and tests |
| `release` | `cargo build --release` | quick optimized builds |
| `dist` | `cargo build --profile dist -p flux_cli` | what you give to others; takes several minutes, gives a faster and smaller binary |

Compare timings only between builds of the same profile.
