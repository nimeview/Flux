# Flux for Visual Studio Code

Language support for [Flux](https://github.com/nimeview/Flux).

## What you get

- **Highlighting**, including code inside `{...}` in strings and every number form.
- **Completion**: local variables and parameters, everything in the file, imported names, `Ok`/`Some` and the other built-in names, types, enum variants, and struct fields after `.`.
- **Automatic imports**: accepting `len` also adds `import collections { len }`.
- **Parameter hints** while typing a call, for your functions and the standard library.
- **Hover** with types and standard library documentation.
- **Inlay hints** showing inferred types, as in `let it: Item = ...`.
- **Errors as you type**, and a quick fix that imports an undefined standard library name.
- **Go to definition, find references, highlighted uses, rename, and outline.**
- **Format on save** with `flux fmt`, on by default.
- **Run and test**: a Run lens above `fn main`, a Run test lens above each `fn test_*`, and a run button in the title bar.
- **Snippets**, such as `main`, `test`, `match`, `matchr`, `onv`, and `forr`. Type a prefix and pick it from the list.
- **Tasks** for `flux check`, `run`, and `test`, with errors shown in Problems.

## Install

Build the compiler and the extension, then install the extension:

```bash
cargo build --profile dist -p flux_cli
cd editors/vscode
npm install
npx vsce package
code --install-extension flux-lang-0.1.0.vsix
```

The extension finds `flux` in `target/dist`, `target/release`, or `target/debug` of the open folder, and then on your `PATH`. For projects outside this repository, put the binary on your `PATH` or set `flux.path`.

## Keys and commands

| Key (macOS) | Key (Linux, Windows) | Command |
|---|---|---|
| `Cmd+Alt+R` | `Ctrl+Alt+R` | Flux: Run File |
| `Cmd+Alt+T` | `Ctrl+Alt+T` | Flux: Run All Tests |

The command palette has the rest under `Flux:`: Run Project, Check File, Run Tests in File, Open REPL, New Project in This Folder, Restart Language Server, and Show Language Server Output. The status bar item shows the server's state, and clicking it restarts the server.

## Settings

| Setting | Default | Meaning |
|---|---|---|
| `flux.path` | empty | path to the `flux` binary |
| `flux.server.enable` | `true` | start the language server |
| `flux.run.jit` | `false` | pass `--jit` to run and test |
| `flux.codeLens.enable` | `true` | show the Run lenses |
| `flux.trace.server` | `off` | log messages to the language server |

To turn off the type hints, set `"[flux]": { "editor.inlayHints.enabled": "off" }`.

## Development

```bash
npm test                     # grammar, manifest, and server protocol
node scripts/vscode/run.js   # everything above, inside a real VS Code
```

The second command opens a VS Code window with a throwaway profile for a few seconds. Set `VSCODE_APP` to the editor's binary if it is not in the standard macOS location. Set `FLUX_BIN` to test another build of `flux`.
