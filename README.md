# Flux

Flux is a small, statically typed language with reactive values built in. Write ordinary functions, structs, and enums, and when some values depend on others, declare that once: Flux keeps them up to date, the way cells in a spreadsheet do.

```flux
import io { println }

event Deposit(amount: Int)

let mut balance = 0

let doubled = balance * 2

on Deposit { amount ->
    balance = balance + amount
}

watch balance {
    println("balance={balance} doubled={doubled}")
}

fn main() {
    emit Deposit(50)
}
```

```output
balance=0 doubled=0
balance=50 doubled=100
```

Types are checked before anything runs, expected failures are values (`Result`, `Option`, `?`), and bugs stop the program with a trace that gives the file, line, and column. There is a virtual machine, which defines what programs mean, and an experimental JIT that must match it.

## Quick start

You need a recent Rust toolchain.

```bash
cargo build --release -p flux_cli
target/release/flux init my_app
cd my_app
../target/release/flux run
```

| Command | What it does |
|---|---|
| `flux run [file]` | compile and run |
| `flux check [file]` | check without running |
| `flux test [filter]` | run every `fn test_*()` in the project |
| `flux fmt file --write` | format a file |
| `flux lock [--check]` | manage `flux.lock` for path dependencies |
| `flux repl` | interactive shell |
| `flux lsp-server` | language server for editors; see [the VS Code extension](editors/vscode/README.md) |

## Documentation

- **The book**, in [`docs/flux`](docs/flux/src/SUMMARY.md), teaches the language. Read it there, or build it with [mdBook](https://rust-lang.github.io/mdBook/): `mdbook serve docs/flux`. The test suite runs every example in it.
- [`SPEC.md`](SPEC.md) is the exact reference for the language.
- [`CHANGELOG.md`](CHANGELOG.md) lists what changed.
- [`ARCHITECTURE.md`](ARCHITECTURE.md) explains how the compiler is put together.
- [`PERFORMANCE.md`](PERFORMANCE.md) records measurements and how to reproduce them.
- [`RELEASE.md`](RELEASE.md) gives the release gate and the versioning policy.

## Status

Version 0.1. The language surface is stable on Linux and macOS, while the JIT, `flux wasm`, and `flux migrate` are experimental. There is no package registry yet, only local path dependencies. See [Status and limits](docs/flux/src/status.md).

## Working on Flux

```bash
cargo test --workspace --all-targets --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
```

`cargo build --profile dist -p flux_cli` builds the binary to distribute, with link-time optimization. It takes several minutes, so use `--release` day to day.

## License

Apache-2.0. See [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE).
