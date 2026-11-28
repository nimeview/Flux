import { readFileSync } from "node:fs";
import { Config, ConfigError } from "./config.ts";

const KNOWN = ["name", "port", "debug", "ratio"];

function load(path: string) {
  let text: string;
  try {
    text = readFileSync(path, "utf8");
  } catch (e) {
    throw new ConfigError((e as Error).message);
  }
  const cfg = Config.parse(text);
  const name = cfg.text("name");
  const port = cfg.int("port");
  if (port < 1 || port > 65535) throw new ConfigError(`\`port\` must be between 1 and 65535, got ${port}`);
  const ratio = cfg.floatOr("ratio", 1.0);
  if (ratio < 0 || ratio > 1) throw new ConfigError(`\`ratio\` must be between 0 and 1, got ${ratio}`);
  const debug = cfg.flagOr("debug", false);
  const extra = cfg.keys().filter((k) => !KNOWN.includes(k));
  return { name, port, debug, ratio, extra };
}

function main(): number {
  const argv = process.argv.slice(2);
  if (argv.length !== 1) {
    console.error("usage: config FILE");
    return 1;
  }
  try {
    const s = load(argv[0]);
    console.log(`service ${s.name}`);
    console.log(`  listens on port ${s.port}`);
    console.log(`  debug ${s.debug ? "on" : "off"}`);
    console.log(`  sampling ratio ${s.ratio}`);
    if (s.extra.length > 0) console.log(`  ignored keys: ${s.extra.join(", ")}`);
    return 0;
  } catch (e) {
    if (!(e instanceof ConfigError)) throw e;
    console.error(`config error: ${e.message}`);
    return 1;
  }
}

process.exitCode = main();
