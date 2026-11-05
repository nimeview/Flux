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
```

Reports everything `run` would reject, without running anything.

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

The exit status is 1 if anything failed.

## flux fmt

```bash
flux fmt app.fl          # print the formatted file
flux fmt app.fl --write  # rewrite it
flux fmt app.fl --check  # exit 1 if it would change
```

There are no options, so all Flux code looks the same. A `match` stays on one line when it fits in 100 columns. Number spellings such as `1_000` and `0xff` are kept.

## flux repl

An interactive shell. `:emit Event [payload]` fires an event, and `:help` lists the rest.

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
