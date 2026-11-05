// Tokenizes Flux source with the real TextMate engine and checks scopes.
const fs = require("fs");
const path = require("path");
const assert = require("assert");
const oniguruma = require("vscode-oniguruma");
const { Registry, INITIAL, parseRawGrammar } = require("vscode-textmate");

const root = path.join(__dirname, "..");
const repo = path.join(root, "..", "..");

async function grammar() {
    const wasm = fs.readFileSync(require.resolve("vscode-oniguruma/release/onig.wasm")).buffer;
    await oniguruma.loadWASM(wasm);
    const registry = new Registry({
        onigLib: Promise.resolve({
            createOnigScanner: (patterns) => new oniguruma.OnigScanner(patterns),
            createOnigString: (text) => new oniguruma.OnigString(text),
        }),
        loadGrammar: async () =>
            parseRawGrammar(
                fs.readFileSync(path.join(root, "syntaxes/flux.tmLanguage.json"), "utf8"),
                "flux.tmLanguage.json",
            ),
    });
    return registry.loadGrammar("source.flux");
}

function tokens(g, text) {
    const out = [];
    let stack = INITIAL;
    for (const line of text.split("\n")) {
        const result = g.tokenizeLine(line, stack);
        for (const t of result.tokens) {
            out.push({ text: line.slice(t.startIndex, t.endIndex), scopes: t.scopes });
        }
        stack = result.ruleStack;
    }
    return out;
}

function scopeOf(all, text) {
    const found = all.find((t) => t.text === text);
    assert(found, `no token ${JSON.stringify(text)} in ${all.map((t) => t.text).join("|")}`);
    return found.scopes.join(" ");
}

function files(dir, out = []) {
    for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
        if (["target", "node_modules", ".git", "fail", ".flux"].includes(entry.name)) {
            continue;
        }
        const full = path.join(dir, entry.name);
        if (entry.isDirectory()) {
            files(full, out);
        } else if (entry.name.endsWith(".fl")) {
            out.push(full);
        }
    }
    return out;
}

(async () => {
    const g = await grammar();
    const cases = [
        ["// a note", "// a note".slice(2), "comment.line"],
        ["import io { println }", "import", "keyword.control.import"],
        ["import a.b { c }", "a.b", "entity.name.namespace"],
        ["pub fn add(a: Int) -> Int { a }", "add", "entity.name.function"],
        ["pub fn add(a: Int) -> Int { a }", "pub", "storage.modifier"],
        ["pub fn add(a: Int) -> Int { a }", "Int", "support.type.primitive"],
        ["rfn scale(x: Int) { x }", "rfn", "storage.type.function"],
        ["struct Point { x: Int }", "Point", "entity.name.type"],
        ["enum Light { Red, Green(Int) }", "enum", "storage.type"],
        ["impl Area for Square { }", "Area", "entity.name.type"],
        ["type Id = Int", "Id", "entity.name.type"],
        ["event Click(n: Int)", "Click", "entity.name.function.event"],
        ["on Click { n -> }", "on", "keyword.control.reactive"],
        ["on Click { n -> }", "Click", "entity.name.function.event"],
        ["emit Click(1)", "emit", "keyword.control.reactive"],
        ["watch total { }", "total", "variable.other.reactive"],
        ["let mut x = 1", "mut", "storage.modifier.mut"],
        ["let mut x = 1", "x", "variable.other.declaration"],
        ["derived y = x * 2", "derived", "storage.type.binding"],
        ["const LIMIT = 10", "const", "storage.type.binding"],
        ["match x { _ => 1 }", "match", "keyword.control"],
        ["return Ok(1)", "Ok", "support.constant.prelude"],
        ["let a = b?", "?", "keyword.operator.try"],
        ["let a = x as Float", "as", "keyword.operator.cast"],
        ["let a = 1_000", "1_000", "constant.numeric.integer"],
        ["let a = 0xff_ff", "0xff_ff", "constant.numeric.hex"],
        ["let a = 0xf32", "0xf32", "constant.numeric.hex"],
        ["let a = 255u8", "255u8", "constant.numeric.integer"],
        ["let a = 1e3", "1e3", "constant.numeric.float"],
        ["let a = 2.5e-3", "2.5e-3", "constant.numeric.float"],
        ["let a = 1e3f32", "1e3f32", "constant.numeric.float"],
        ["let a = true", "true", "constant.language.boolean"],
        ["foo(1)", "foo", "entity.name.function.call"],
        ["p.total", "total", "variable.other.member"],
        ['"a\\n"', "\\n", "constant.character.escape"],
        ['"a\\q"', "\\q", "invalid.illegal"],
        ['"\\{x\\}"', "\\{", "constant.character.escape"],
        ['"sum {a + 1} end"', "1", "constant.numeric"],
        ['"sum {a + 1} end"', "{", "punctuation.section.interpolation.begin"],
        ['"{"<{n}>"}"', "n", "variable.other"],
        ['"{Point { x: 1 }}" // c', " c", "comment.line"],
        ["|x| x + 1", "|", "keyword.operator"],
    ];
    for (const [source, text, scope] of cases) {
        const got = scopeOf(tokens(g, source), text);
        assert(got.includes(scope), `${source}: ${text} is ${got}, want ${scope}`);
    }

    // A later token must not be swallowed by a string or comment that ran on.
    const after = tokens(g, 'let a = "x{1}y"\nfn f() { }');
    assert(scopeOf(after, "f").includes("entity.name.function"));

    const sources = files(repo);
    assert(sources.length > 50, `only ${sources.length} sources`);
    for (const file of sources) {
        const all = tokens(g, fs.readFileSync(file, "utf8"));
        const bad = all.filter((t) => t.scopes.some((s) => s.startsWith("invalid")));
        assert.deepStrictEqual(bad.map((t) => t.text), [], file);
    }
    console.log(`grammar: ${cases.length} cases, ${sources.length} repository files`);
})().catch((error) => {
    console.error(error.message);
    process.exit(1);
});
