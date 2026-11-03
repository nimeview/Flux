# Getting started

## Install

Flux is built from source with a recent Rust toolchain:

```bash
git clone https://github.com/nimeview/Flux.git
cd Flux
cargo build --release -p flux_cli
```

The compiler is then at `target/release/flux`. For a smaller, faster binary that takes several minutes to build, use `cargo build --profile dist -p flux_cli`, which puts it at `target/dist/flux`. Add either directory to your `PATH`, or copy the binary somewhere that already is.

## Your first program

Save this as `hello.fl`:

```flux
import io { println }

fn main() {
    println("Hello, world!")
}
```

```output
Hello, world!
```

Run it:

```bash
flux run hello.fl
```

`println` comes from the standard library module `io`, so it has to be imported. A program's entry point is `fn main()`, and it may return an `Int`, which becomes the exit status:

```flux,exit=3
fn main() -> Int {
    3
}
```

## A project

`flux init` creates a project:

```bash
flux init my_app
cd my_app
flux run
```

That gives you a `flux.toml` and a small `main.fl`. Inside a project, `flux run` and `flux check` need no file name: they use the `entry` from `flux.toml`.

```toml
[package]
name = "my_app"
version = "0.1.0"
entry = "main.fl"
```

`flux init --lib my_lib` creates a library instead, with `lib.fl` and `lib_test.fl`.

## The commands you will use most

| Command | What it does |
|---|---|
| `flux run` | compile and run |
| `flux check` | check types without running |
| `flux test` | run every `fn test_*()` in the project |
| `flux fmt file.fl --write` | format a file |
| `flux repl` | try things interactively |

[Tools](./tools.md) covers these and the rest.
