// Checks the manifest against the code, with `vscode` replaced by a stub.
const fs = require("fs");
const path = require("path");
const assert = require("assert");
const Module = require("module");

const root = path.join(__dirname, "..");
const manifest = JSON.parse(fs.readFileSync(path.join(root, "package.json"), "utf8"));

const registered = new Set();
const disposable = { dispose() {} };
const stub = {
    window: {
        createStatusBarItem: () => ({ show() {}, dispose() {} }),
        showErrorMessage: async () => undefined,
        showWarningMessage: async () => undefined,
        terminals: [],
    },
    commands: {
        registerCommand(name) {
            registered.add(name);
            return disposable;
        },
        executeCommand() {},
    },
    workspace: {
        getConfiguration: () => ({ get: (key) => (key === "server.enable" ? false : undefined) }),
        onDidChangeConfiguration: () => disposable,
        workspaceFolders: [],
    },
    languages: { registerCodeLensProvider: () => disposable },
    tasks: { registerTaskProvider: () => disposable },
    EventEmitter: class {
        event = () => disposable;
        fire() {}
        dispose() {}
    },
    StatusBarAlignment: { Left: 1 },
};
const load = Module._load;
const client = { LanguageClient: class {}, State: {} };
Module._load = (request, ...rest) => {
    if (request === "vscode") {
        return stub;
    }
    return request === "vscode-languageclient/node" ? client : load(request, ...rest);
};

(async () => {
    await require(path.join(root, "extension.js")).activate({ subscriptions: [] });
    for (const { command } of manifest.contributes.commands) {
        assert(registered.has(command), `${command} is declared but not registered`);
    }
    const declared = new Set(manifest.contributes.commands.map((c) => c.command));
    for (const menu of Object.values(manifest.contributes.menus)) {
        for (const { command } of menu) {
            assert(declared.has(command), `${command} is in a menu but not declared`);
        }
    }
    for (const { command } of manifest.contributes.keybindings) {
        assert(declared.has(command), `${command} has a key but is not declared`);
    }
    const snippets = JSON.parse(fs.readFileSync(path.join(root, "snippets/flux.json"), "utf8"));
    assert(Object.keys(snippets).length > 20);
    JSON.parse(fs.readFileSync(path.join(root, "language-configuration.json"), "utf8"));
    console.log(`extension: ${registered.size} commands, ${Object.keys(snippets).length} snippets`);
})().catch((error) => {
    console.error(error.message);
    process.exit(1);
});
