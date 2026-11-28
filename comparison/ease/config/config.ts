export type Value =
  | { kind: "integer"; v: number }
  | { kind: "float"; v: number }
  | { kind: "boolean"; v: boolean }
  | { kind: "string"; v: string };

export class ConfigError extends Error {}

function parseValue(raw: string): Value {
  if (raw === "true" || raw === "false") return { kind: "boolean", v: raw === "true" };
  if (raw.length >= 2 && raw.startsWith('"') && raw.endsWith('"')) return { kind: "string", v: raw.slice(1, -1) };
  if (/^-?\d+$/.test(raw)) return { kind: "integer", v: Number(raw) };
  if (raw !== "" && !Number.isNaN(Number(raw))) return { kind: "float", v: Number(raw) };
  return { kind: "string", v: raw };
}

export class Config {
  #entries = new Map<string, Value>();

  static parse(text: string): Config {
    const cfg = new Config();
    text.split("\n").forEach((raw, i) => {
      const line = raw.trim();
      if (line === "" || line.startsWith("#")) return;
      const eq = line.indexOf("=");
      const key = eq < 0 ? "" : line.slice(0, eq).trim();
      if (key === "") throw new ConfigError(`line ${i + 1}: expected \`key = value\`, got \`${line}\``);
      if (cfg.#entries.has(key)) throw new ConfigError(`line ${i + 1}: \`${key}\` is set twice`);
      cfg.#entries.set(key, parseValue(line.slice(eq + 1).trim()));
    });
    return cfg;
  }

  #typed<K extends Value["kind"]>(key: string, kind: K, fallback?: Extract<Value, { kind: K }>["v"]) {
    const v = this.#entries.get(key);
    if (v === undefined) {
      if (fallback !== undefined) return fallback;
      throw new ConfigError(`missing required key \`${key}\``);
    }
    if (kind === "float" && v.kind === "integer") return v.v;
    if (v.kind !== kind) throw new ConfigError(`\`${key}\` should be of type ${kind}, not ${v.kind}`);
    return v.v as Extract<Value, { kind: K }>["v"];
  }

  int(key: string) { return this.#typed(key, "integer"); }
  text(key: string) { return this.#typed(key, "string"); }
  flagOr(key: string, fallback: boolean) { return this.#typed(key, "boolean", fallback); }
  floatOr(key: string, fallback: number) { return this.#typed(key, "float", fallback); }
  keys() { return [...this.#entries.keys()].sort(); }
}
