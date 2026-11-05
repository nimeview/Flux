// Runs the extension inside a real VS Code, with a throwaway profile.
// VSCODE_APP may name the Electron binary; otherwise the installed app is used.
const fs = require("fs");
const os = require("os");
const path = require("path");
const { runTests } = require("@vscode/test-electron");

const root = path.join(__dirname, "..", "..");
const repo = path.join(root, "..", "..");

(async () => {
    const workspace = fs.mkdtempSync(path.join(os.tmpdir(), "flux-vscode-"));
    const data = fs.mkdtempSync(path.join(os.tmpdir(), "flux-vscode-data-"));
    fs.mkdirSync(path.join(workspace, ".vscode"));
    fs.writeFileSync(
        path.join(workspace, ".vscode/settings.json"),
        JSON.stringify({
            "flux.path": process.env.FLUX_BIN || path.join(repo, "target/release/flux"),
            "security.workspace.trust.enabled": false,
        }),
    );
    fs.writeFileSync(
        path.join(workspace, "flux.toml"),
        '[package]\nname = "t"\nversion = "0.1.0"\nentry = "main.fl"\n',
    );
    fs.copyFileSync(path.join(__dirname, "sample.fl"), path.join(workspace, "main.fl"));
    await runTests({
        vscodeExecutablePath:
            process.env.VSCODE_APP || "/Applications/Visual Studio Code.app/Contents/MacOS/Code",
        extensionDevelopmentPath: root,
        extensionTestsPath: path.join(__dirname, "suite.js"),
        launchArgs: [workspace, "--disable-extensions", `--user-data-dir=${data}`, "--disable-workspace-trust"],
    });
    fs.rmSync(workspace, { recursive: true, force: true });
    fs.rmSync(data, { recursive: true, force: true });
})().catch((error) => {
    console.error(error.message || error);
    process.exit(1);
});
