// The Flux extension. The language server (`flux lsp-server`) provides the
// language features; this file starts it and adds commands, code lenses, and tasks.
const fs = require("fs");
const path = require("path");
const cp = require("child_process");
const vscode = require("vscode");
const { LanguageClient, State } = require("vscode-languageclient/node");

const LANGUAGE = "flux";
const TERMINAL = "Flux";

let client;
let status;

function config() {
    return vscode.workspace.getConfiguration("flux");
}

function isFile(candidate) {
    try {
        return fs.statSync(candidate).isFile();
    } catch {
        return false;
    }
}

function onPath(name) {
    const lookup = process.platform === "win32" ? "where" : "which";
    const found = cp.spawnSync(lookup, [name], { encoding: "utf8" });
    return found.status === 0 ? found.stdout.split(/\r?\n/)[0].trim() : undefined;
}

// The binary from `flux.path`, a build in the workspace, or the PATH.
function findFlux() {
    const name = process.platform === "win32" ? "flux.exe" : "flux";
    const configured = config().get("path");
    if (configured) {
        const expanded = configured.replace(/^~(?=\/|\\|$)/, process.env.HOME || "");
        return path.isAbsolute(expanded) || !expanded.includes(path.sep)
            ? expanded
            : path.join(workspaceRoot() || "", expanded);
    }
    for (const folder of vscode.workspace.workspaceFolders || []) {
        for (const profile of ["dist", "release", "debug"]) {
            const candidate = path.join(folder.uri.fsPath, "target", profile, name);
            if (isFile(candidate)) {
                return candidate;
            }
        }
    }
    return onPath(name);
}

function workspaceRoot() {
    const editor = vscode.window.activeTextEditor;
    const folder = editor && vscode.workspace.getWorkspaceFolder(editor.document.uri);
    return (folder || (vscode.workspace.workspaceFolders || [])[0])?.uri.fsPath;
}

// The directory of the nearest flux.toml above `file`, or the file's own directory.
function projectDir(file) {
    let dir = path.dirname(file);
    for (;;) {
        if (isFile(path.join(dir, "flux.toml"))) {
            return dir;
        }
        const parent = path.dirname(dir);
        if (parent === dir) {
            return path.dirname(file);
        }
        dir = parent;
    }
}

async function reportMissing() {
    const open = "Open Settings";
    const choice = await vscode.window.showErrorMessage(
        "Flux: the `flux` binary was not found. Build it with `cargo build --release -p flux_cli`, add it to your PATH, or set `flux.path`.",
        open,
    );
    if (choice === open) {
        vscode.commands.executeCommand("workbench.action.openSettings", "flux.path");
    }
}

function quote(arg) {
    return /^[A-Za-z0-9_./:=@+-]+$/.test(arg) ? arg : `'${arg.replace(/'/g, `'\\''`)}'`;
}

function terminal(cwd) {
    const existing = vscode.window.terminals.find((t) => t.name === TERMINAL);
    if (existing && existing.exitStatus === undefined && existing.fluxCwd === cwd) {
        return existing;
    }
    if (existing) {
        existing.dispose();
    }
    const created = vscode.window.createTerminal({ name: TERMINAL, cwd });
    created.fluxCwd = cwd;
    return created;
}

function runInTerminal(args, cwd) {
    const flux = findFlux();
    if (!flux) {
        reportMissing();
        return;
    }
    const shell = terminal(cwd);
    shell.show(true);
    shell.sendText([flux, ...args].map(quote).join(" "));
}

// The Flux document to act on, saved so that the command sees what is on screen.
async function activeFile() {
    const editor = vscode.window.activeTextEditor;
    if (!editor || editor.document.languageId !== LANGUAGE) {
        vscode.window.showWarningMessage("Flux: open a .fl file first.");
        return undefined;
    }
    if (editor.document.isUntitled) {
        vscode.window.showWarningMessage("Flux: save the file first.");
        return undefined;
    }
    await editor.document.save();
    return editor.document.fileName;
}

function jitArgs() {
    return config().get("run.jit") ? ["--jit"] : [];
}

async function runFile() {
    const file = await activeFile();
    if (file) {
        const dir = path.dirname(file);
        runInTerminal(["run", ...jitArgs(), path.relative(dir, file)], dir);
    }
}

async function checkFile() {
    const file = await activeFile();
    if (file) {
        const dir = path.dirname(file);
        runInTerminal(["check", path.relative(dir, file)], dir);
    }
}

function runProject() {
    const root = workspaceRoot();
    if (root) {
        const file = vscode.window.activeTextEditor?.document.fileName;
        runInTerminal(["run", ...jitArgs()], file ? projectDir(file) : root);
    }
}

async function runTests(filter) {
    await vscode.workspace.saveAll(false);
    const file = vscode.window.activeTextEditor?.document.fileName;
    const dir = file ? projectDir(file) : workspaceRoot();
    if (dir) {
        runInTerminal(["test", ...(filter ? [filter] : []), ...jitArgs()], dir);
    }
}

async function testFile() {
    const file = await activeFile();
    if (file) {
        runTests(path.relative(projectDir(file), file));
    }
}

function repl() {
    const root = workspaceRoot();
    runInTerminal(["repl"], root || process.cwd());
}

async function initProject() {
    const root = workspaceRoot();
    if (!root) {
        vscode.window.showWarningMessage("Flux: open a folder first.");
        return;
    }
    runInTerminal(["init", "."], root);
}

// Run above `fn main`, and Run test above each `fn test_*`.
class FluxLenses {
    constructor() {
        this.changed = new vscode.EventEmitter();
        this.onDidChangeCodeLenses = this.changed.event;
    }

    provideCodeLenses(document) {
        if (!config().get("codeLens.enable")) {
            return [];
        }
        const lenses = [];
        const relative = path.relative(projectDir(document.fileName), document.fileName);
        for (let number = 0; number < document.lineCount; number += 1) {
            const line = document.lineAt(number);
            const range = line.range;
            if (/^\s*(pub\s+)?fn\s+main\s*\(/.test(line.text)) {
                lenses.push(
                    new vscode.CodeLens(range, { title: "$(play) Run", command: "flux.run" }),
                );
            }
            const test = /^\s*fn\s+(test_[A-Za-z0-9_]*)\s*\(/.exec(line.text);
            if (test) {
                lenses.push(
                    new vscode.CodeLens(range, {
                        title: "$(beaker) Run test",
                        command: "flux.testNamed",
                        arguments: [`${relative}::${test[1]}`],
                    }),
                );
            }
        }
        return lenses;
    }
}

// Tasks for tasks.json and the Run Task list, with errors shown in Problems.
class FluxTasks {
    provideTasks() {
        return [
            this.create({ type: "flux", command: "check" }, "check"),
            this.create({ type: "flux", command: "run" }, "run"),
            this.create({ type: "flux", command: "test" }, "test"),
        ];
    }

    resolveTask(task) {
        if (task.definition.type === "flux" && task.definition.command) {
            return this.create(task.definition, task.name || task.definition.command);
        }
        return undefined;
    }

    create(definition, name) {
        const args = [definition.command, ...(definition.file ? [definition.file] : [])];
        const execution = new vscode.ShellExecution(findFlux() || "flux", args);
        const task = new vscode.Task(
            definition,
            vscode.TaskScope.Workspace,
            name,
            "flux",
            execution,
            "$flux",
        );
        if (definition.command === "test") {
            task.group = vscode.TaskGroup.Test;
        } else if (definition.command === "check") {
            task.group = vscode.TaskGroup.Build;
        }
        return task;
    }
}

function setStatus(text, tooltip) {
    status.text = text;
    status.tooltip = tooltip;
    status.show();
}

async function startClient(context) {
    if (!config().get("server.enable")) {
        setStatus("$(circle-slash) Flux", "The language server is turned off (flux.server.enable).");
        return;
    }
    const flux = findFlux();
    if (!flux) {
        setStatus("$(error) Flux", "The flux binary was not found.");
        reportMissing();
        return;
    }
    // No `transport`: `TransportKind.stdio` would add a `--stdio` the server rejects.
    const executable = { command: flux, args: ["lsp-server"] };
    client = new LanguageClient(
        LANGUAGE,
        "Flux Language Server",
        { run: executable, debug: executable },
        {
            documentSelector: [
                { scheme: "file", language: LANGUAGE },
                { scheme: "untitled", language: LANGUAGE },
            ],
            synchronize: {
                fileEvents: vscode.workspace.createFileSystemWatcher("**/{*.fl,flux.toml}"),
            },
        },
    );
    context.subscriptions.push(
        client.onDidChangeState((change) => {
            if (change.newState === State.Running) {
                setStatus("$(check) Flux", `Language server running: ${flux}`);
            } else if (change.newState === State.Starting) {
                setStatus("$(sync~spin) Flux", "Language server starting");
            } else {
                setStatus("$(warning) Flux", "Language server stopped. Click to restart.");
            }
        }),
    );
    try {
        await client.start();
    } catch (error) {
        setStatus("$(error) Flux", String(error));
        vscode.window.showErrorMessage(`Flux: the language server failed to start: ${error}`);
    }
}

async function stopClient() {
    if (client) {
        const stopping = client;
        client = undefined;
        await stopping.stop().catch(() => {});
    }
}

async function activate(context) {
    status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 0);
    status.command = "flux.restartServer";
    const lenses = new FluxLenses();
    const restart = async () => {
        await stopClient();
        await startClient(context);
    };
    const register = (name, handler) =>
        context.subscriptions.push(vscode.commands.registerCommand(name, handler));
    register("flux.run", runFile);
    register("flux.runProject", runProject);
    register("flux.check", checkFile);
    register("flux.test", () => runTests());
    register("flux.testNamed", (filter) => runTests(filter));
    register("flux.testFile", testFile);
    register("flux.repl", repl);
    register("flux.init", initProject);
    register("flux.restartServer", restart);
    register("flux.showOutput", () => client?.outputChannel.show());
    context.subscriptions.push(
        status,
        lenses.changed,
        vscode.languages.registerCodeLensProvider({ language: LANGUAGE }, lenses),
        vscode.tasks.registerTaskProvider("flux", new FluxTasks()),
        vscode.workspace.onDidChangeConfiguration((event) => {
            if (event.affectsConfiguration("flux.path") || event.affectsConfiguration("flux.server")) {
                restart();
            }
            if (event.affectsConfiguration("flux.codeLens")) {
                lenses.changed.fire();
            }
        }),
    );
    await startClient(context);
}

function deactivate() {
    return stopClient();
}

module.exports = { activate, deactivate };
