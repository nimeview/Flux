// Speaks the Language Server Protocol to `flux lsp-server`, as VS Code does.
const fs = require("fs");
const os = require("os");
const path = require("path");
const assert = require("assert");
const cp = require("child_process");

const repo = path.join(__dirname, "..", "..", "..");
const flux = process.env.FLUX_BIN || path.join(repo, "target/release/flux");
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "flux-lsp-"));
const file = path.join(dir, "main.fl");
const uri = `file://${fs.realpathSync(dir)}/main.fl`;
const source = `import io { println }

struct Item {
    price: Int
}

fn total(i: Item) -> Int {
    i.price + 1
}

fn main() {
    let it = Item { price: 2 }
    println("{total(it)}")
}
`;
fs.writeFileSync(file, source);

const server = cp.spawn(flux, ["lsp-server"], { cwd: dir });
let buffer = Buffer.alloc(0);
let nextId = 1;
const pending = new Map();
const diagnostics = [];
server.stderr.on("data", () => {});
server.stdout.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    for (;;) {
        const end = buffer.indexOf("\r\n\r\n");
        if (end < 0) {
            return;
        }
        const length = Number(/Content-Length: (\d+)/i.exec(buffer.slice(0, end).toString())[1]);
        if (buffer.length < end + 4 + length) {
            return;
        }
        const message = JSON.parse(buffer.slice(end + 4, end + 4 + length).toString());
        buffer = buffer.slice(end + 4 + length);
        if (message.method === "textDocument/publishDiagnostics") {
            diagnostics.push(message.params);
        } else if (message.id !== undefined && pending.has(message.id)) {
            pending.get(message.id)(message);
            pending.delete(message.id);
        }
    }
});
function send(payload) {
    const body = JSON.stringify({ jsonrpc: "2.0", ...payload });
    server.stdin.write(`Content-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`);
}
function request(method, params) {
    const id = nextId++;
    return new Promise((resolve, reject) => {
        const timer = setTimeout(() => reject(new Error(`${method} timed out`)), 5000);
        pending.set(id, (message) => {
            clearTimeout(timer);
            message.error ? reject(new Error(message.error.message)) : resolve(message.result);
        });
        send({ id, method, params });
    });
}
const doc = { textDocument: { uri } };
async function waitFor(predicate, what) {
    for (let i = 0; i < 100; i += 1) {
        const found = diagnostics.filter((d) => d.uri === uri).pop();
        if (found && predicate(found.diagnostics)) {
            return found.diagnostics;
        }
        await new Promise((r) => setTimeout(r, 50));
    }
    throw new Error(`no diagnostics: ${what}: ${JSON.stringify(diagnostics.at(-1))}`);
}

(async () => {
    const init = await request("initialize", { processId: null, rootUri: `file://${dir}`, capabilities: {} });
    const caps = init.capabilities;
    for (const name of ["hoverProvider", "definitionProvider", "completionProvider", "documentFormattingProvider", "documentSymbolProvider", "renameProvider", "referencesProvider", "documentHighlightProvider", "signatureHelpProvider", "inlayHintProvider", "codeActionProvider"]) {
        assert(caps[name], `${name} missing`);
    }
    send({ method: "initialized", params: {} });
    send({ method: "textDocument/didOpen", params: { textDocument: { uri, languageId: "flux", version: 1, text: source } } });
    await waitFor((d) => d.length === 0, "clean file");

    const hover = await request("textDocument/hover", { ...doc, position: { line: 12, character: 16 } });
    assert(JSON.stringify(hover).includes("total"), `hover: ${JSON.stringify(hover)}`);

    const definition = await request("textDocument/definition", { ...doc, position: { line: 12, character: 16 } });
    const target = Array.isArray(definition) ? definition[0] : definition;
    assert.strictEqual(target.range.start.line, 6, `definition: ${JSON.stringify(definition)}`);

    const completion = await request("textDocument/completion", { ...doc, position: { line: 12, character: 16 } });
    const labels = (completion.items || completion).map((c) => c.label);
    assert(labels.includes("total"), `completion: ${labels.join(",")}`);

    const symbols = await request("textDocument/documentSymbol", doc);
    const names = symbols.map((s) => s.name);
    assert(names.includes("main") && names.includes("total") && names.includes("Item"), names.join(","));

    const rename = await request("textDocument/rename", { ...doc, position: { line: 6, character: 4 }, newName: "sum" });
    const edits = rename.changes ? Object.values(rename.changes).flat() : rename.documentChanges[0].edits;
    assert.strictEqual(edits.length, 2, `rename: ${JSON.stringify(rename)}`);

    const messy = source.replace("    i.price + 1", "i.price   +   1");
    send({ method: "textDocument/didChange", params: { textDocument: { uri, version: 2 }, contentChanges: [{ text: messy }] } });
    const formatted = await request("textDocument/formatting", { ...doc, options: { tabSize: 4, insertSpaces: true } });
    assert(formatted && formatted.length > 0 && JSON.stringify(formatted).includes("i.price + 1"), `format: ${JSON.stringify(formatted)}`);

    const broken = source.replace("let it = Item { price: 2 }", 'let it = Item { price: "no" }');
    send({ method: "textDocument/didChange", params: { textDocument: { uri, version: 3 }, contentChanges: [{ text: broken }] } });
    const errors = await waitFor((d) => d.length > 0, "type error");
    assert(errors.some((d) => d.severity === 1 && d.range.start.line > 0), JSON.stringify(errors));

    send({ method: "textDocument/didChange", params: { textDocument: { uri, version: 4 }, contentChanges: [{ text: source }] } });
    await waitFor((d) => d.length === 0, "fixed file");

    await request("shutdown", null);
    send({ method: "exit" });
    console.log("server: hover, definition, completion, symbols, rename, formatting, diagnostics");
})()
    .catch((error) => {
        console.error(error.message);
        process.exitCode = 1;
    })
    .finally(() => {
        server.kill();
        fs.rmSync(dir, { recursive: true, force: true });
    });
