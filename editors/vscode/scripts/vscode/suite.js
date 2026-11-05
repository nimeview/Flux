// What a person gets in the editor, asked through VS Code's own API.
const assert = require("assert");
const path = require("path");
const vscode = require("vscode");

async function until(what, read, ok) {
    let last;
    for (let i = 0; i < 100; i += 1) {
        last = await read();
        if (ok(last)) {
            return last;
        }
        await new Promise((r) => setTimeout(r, 100));
    }
    throw new Error(`${what}: ${JSON.stringify(last)}`);
}

async function run() {
    const root = vscode.workspace.workspaceFolders[0].uri;
    const uri = vscode.Uri.joinPath(root, "main.fl");
    const document = await vscode.workspace.openTextDocument(uri);
    const editor = await vscode.window.showTextDocument(document);
    assert.strictEqual(document.languageId, "flux", "language");

    const extension = vscode.extensions.getExtension("nimeview.flux-lang");
    assert(extension, "extension not found");
    await extension.activate();
    assert(extension.isActive, "not active");

    const commands = await vscode.commands.getCommands(true);
    for (const name of ["flux.run", "flux.test", "flux.check", "flux.restartServer"]) {
        assert(commands.includes(name), `${name} missing`);
    }

    const line = document.getText().split("\n").findIndex((l) => l.includes("{total(it)}"));
    const at = new vscode.Position(line, document.lineAt(line).text.indexOf("total") + 2);
    const report = [];

    const hover = await until(
        "hover",
        () => vscode.commands.executeCommand("vscode.executeHoverProvider", uri, at),
        (h) => h && h.length > 0,
    );
    const hoverText = hover[0].contents.map((c) => c.value ?? String(c)).join(" ");
    assert(hoverText.includes("total"), `hover text: ${hoverText}`);
    report.push(`hover "${hoverText.replace(/\s+/g, " ").slice(0, 40)}"`);

    const definition = await vscode.commands.executeCommand("vscode.executeDefinitionProvider", uri, at);
    const target = definition[0].range || definition[0].targetRange;
    assert(document.lineAt(target.start.line).text.includes("fn total"), "definition line");
    report.push("definition");

    const completion = await vscode.commands.executeCommand("vscode.executeCompletionItemProvider", uri, at);
    const labels = completion.items.map((c) => (typeof c.label === "string" ? c.label : c.label.label));
    assert(labels.includes("total"), `completion: ${labels.slice(0, 20)}`);
    report.push(`completion on prefix (${labels.length})`);

    const start = new vscode.Position(line, 4);
    const all = await vscode.commands.executeCommand("vscode.executeCompletionItemProvider", uri, start);
    const allLabels = all.items.map((c) => (typeof c.label === "string" ? c.label : c.label.label));
    for (const name of ["main", "total", "Item", "it", "println", "Ok", "Some", "Int", "match", "watch", "fn"]) {
        assert(allLabels.includes(name), `no ${name} in: ${allLabels.join(",").slice(0, 300)}`);
    }
    report.push(`completion (${allLabels.length}: names, keywords, snippets)`);

    // A name from a module not imported yet brings its import when accepted.
    const edit = new vscode.WorkspaceEdit();
    edit.insert(uri, start, "le");
    await vscode.workspace.applyEdit(edit);
    const typed = new vscode.Position(line, 6);
    const prefixed = await vscode.commands.executeCommand("vscode.executeCompletionItemProvider", uri, typed);
    const len = prefixed.items.find((c) => (c.label.label || c.label) === "len");
    assert(len, "len offered");
    assert(len.additionalTextEdits?.[0]?.newText.includes("import collections { len }"), "auto import edit");
    const remove = new vscode.WorkspaceEdit();
    remove.delete(uri, new vscode.Range(start, typed));
    await vscode.workspace.applyEdit(remove);
    report.push("auto import");

    const call = document.lineAt(line).text.indexOf("(") + 1;
    const signatureAt = (character) =>
        until(
            `signature at ${character}: ${document.lineAt(line).text}`,
            () =>
                vscode.commands.executeCommand(
                    "vscode.executeSignatureHelpProvider",
                    uri,
                    new vscode.Position(line, character),
                ),
            (help) => help && help.signatures.length > 0,
        );
    const signature = await signatureAt(call);
    assert(signature.signatures[0].label.startsWith("println("), `signature: ${signature.signatures[0].label}`);
    const userCall = document.lineAt(line).text.indexOf("total(") + 6;
    const userSignature = await signatureAt(userCall);
    assert.strictEqual(userSignature.signatures[0].label, "total(i: Item) -> Int64");
    report.push(`signature "${userSignature.signatures[0].label}"`);

    const references = await vscode.commands.executeCommand("vscode.executeReferenceProvider", uri, at);
    assert.strictEqual(references.length, 2, "references");
    report.push("references");

    const highlights = await vscode.commands.executeCommand("vscode.executeDocumentHighlights", uri, at);
    assert.strictEqual(highlights.length, 2, "highlights");
    report.push("highlights");

    const whole = new vscode.Range(0, 0, document.lineCount, 0);
    const hints = await vscode.commands.executeCommand("vscode.executeInlayHintProvider", uri, whole);
    const shown = hints.map((h) => (typeof h.label === "string" ? h.label : h.label[0].value));
    assert(shown.includes(": Item"), `hints: ${shown}`);
    report.push(`inlay hints ${shown.join(" ")}`);

    const libraryHover = await vscode.commands.executeCommand(
        "vscode.executeHoverProvider",
        uri,
        new vscode.Position(line, document.lineAt(line).text.indexOf("println") + 2),
    );
    const libraryText = libraryHover[0].contents.map((c) => c.value ?? String(c)).join(" ");
    assert(libraryText.includes("From `io`"), `library hover: ${libraryText}`);
    report.push("library docs");

    const symbols = await vscode.commands.executeCommand("vscode.executeDocumentSymbolProvider", uri);
    const names = symbols.map((s) => s.name);
    assert(names.includes("total") && names.includes("Item"), `symbols: ${names}`);
    report.push("outline");

    const lenses = await vscode.commands.executeCommand("vscode.executeCodeLensProvider", uri, 10);
    const titles = lenses.map((l) => l.command && l.command.title).join(",");
    assert(titles.includes("Run test") && titles.includes("Run"), `lenses: ${titles}`);
    report.push("code lens");

    const rename = await vscode.commands.executeCommand("vscode.executeDocumentRenameProvider", uri, at, "sum");
    assert.strictEqual(rename.get(uri).length, 2, "rename edits");
    report.push("rename");

    const edits = await vscode.commands.executeCommand("vscode.executeFormatDocumentProvider", uri, {
        tabSize: 4,
        insertSpaces: true,
    });
    assert(edits && edits.length > 0, "format edits");
    const workspaceEdit = new vscode.WorkspaceEdit();
    workspaceEdit.set(uri, edits);
    await vscode.workspace.applyEdit(workspaceEdit);
    assert(document.getText().includes("i.price + 1"), "formatted text");
    report.push("format");

    const settings = vscode.workspace.getConfiguration("editor", { languageId: "flux" });
    assert.strictEqual(settings.get("formatOnSave"), true, "format on save default");
    report.push("format on save");

    await editor.edit((b) => b.insert(new vscode.Position(line, 0), '    let bad: Int = "no"\n'));
    const errors = await until(
        "diagnostics",
        () => vscode.languages.getDiagnostics(uri),
        (d) => d.some((x) => x.severity === vscode.DiagnosticSeverity.Error),
    );
    assert(errors.length > 0);
    report.push(`diagnostics: ${errors.find((x) => x.severity === 0).message.slice(0, 40)}`);

    await editor.edit((b) => b.replace(new vscode.Range(line, 0, line + 1, 0), "    let n = abs(-1)\n"));
    const missing = await until(
        "undefined name",
        () => vscode.languages.getDiagnostics(uri),
        (d) => d.some((x) => x.message.includes("`abs`")),
    );
    const problem = missing.find((x) => x.message.includes("`abs`"));
    const actions = await vscode.commands.executeCommand("vscode.executeCodeActionProvider", uri, problem.range);
    const fix = actions.find((a) => a.title === "Import `abs` from `math`");
    assert(fix, `actions: ${actions.map((a) => a.title)}`);
    await vscode.workspace.applyEdit(fix.edit);
    assert(document.getText().includes("import math { abs }"), "fix applied");
    report.push("quick fix import");

    const tasks = await vscode.tasks.fetchTasks({ type: "flux" });
    assert(tasks.length === 3, `tasks: ${tasks.length}`);
    report.push("tasks");

    await vscode.commands.executeCommand("flux.restartServer");
    await until(
        "hover after restart",
        () => {
            const fnLine = document.getText().split("\n").findIndex((l) => l.startsWith("fn total"));
            return vscode.commands.executeCommand("vscode.executeHoverProvider", uri, new vscode.Position(fnLine, 4));
        },
        (h) => h && h.length > 0,
    );
    report.push("restart");

    console.log(`vscode ${vscode.version}: ${report.join(", ")}`);
}

module.exports = { run };
