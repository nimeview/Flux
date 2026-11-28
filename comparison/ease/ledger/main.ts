import { readFileSync } from "node:fs";

type Txn = { line: number; date: string; account: string; cents: number };

type LineError =
  | { kind: "FieldCount"; n: number }
  | { kind: "BadDate"; date: string }
  | { kind: "BadAmount"; amount: string }
  | { kind: "NoAccount" };

type Result<T> = { ok: true; value: T } | { ok: false; error: LineError };

function describe(e: LineError): string {
  switch (e.kind) {
    case "FieldCount": return `expected 3 fields, found ${e.n}`;
    case "BadDate": return `bad date \`${e.date}\` (want YYYY-MM-DD)`;
    case "BadAmount": return `bad amount \`${e.amount}\``;
    case "NoAccount": return "empty account";
  }
}

function checkDate(d: string): boolean {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(d);
  if (!m) return false;
  const month = Number(m[2]), day = Number(m[3]);
  return month >= 1 && month <= 12 && day >= 1 && day <= 31;
}

function parseCents(raw: string): number | null {
  const m = /^(-?)(\d+)(?:\.(\d{1,2}))?$/.exec(raw);
  if (!m) return null;
  const frac = m[3] === undefined ? 0 : m[3].length === 1 ? Number(m[3]) * 10 : Number(m[3]);
  const cents = Number(m[2]) * 100 + frac;
  return m[1] === "-" ? -cents : cents;
}

function parseLine(n: number, text: string): Result<Txn> {
  const f = text.split(",").map((s) => s.trim());
  if (f.length !== 3) return { ok: false, error: { kind: "FieldCount", n: f.length } };
  if (f[1] === "") return { ok: false, error: { kind: "NoAccount" } };
  if (!checkDate(f[0])) return { ok: false, error: { kind: "BadDate", date: f[0] } };
  const cents = parseCents(f[2]);
  if (cents === null) return { ok: false, error: { kind: "BadAmount", amount: f[2] } };
  return { ok: true, value: { line: n, date: f[0], account: f[1], cents } };
}

function money(c: number): string {
  const a = Math.abs(c);
  return `${c < 0 ? "-" : ""}${Math.floor(a / 100)}.${String(a % 100).padStart(2, "0")}`;
}

function main(): number {
  const argv = process.argv.slice(2);
  if (argv.length !== 1) {
    console.error("usage: ledger FILE");
    return 1;
  }
  let text: string;
  try {
    text = readFileSync(argv[0], "utf8");
  } catch (e) {
    console.error(`ledger: ${(e as Error).message}`);
    return 1;
  }
  const totals = new Map<string, number>();
  let largest: Txn | null = null;
  let bad = 0;
  text.split("\n").forEach((line, i) => {
    if (i === 0 || line.trim() === "") return;
    const r = parseLine(i + 1, line);
    if (!r.ok) {
      console.error(`line ${i + 1}: ${describe(r.error)}`);
      bad++;
      return;
    }
    const t = r.value;
    totals.set(t.account, (totals.get(t.account) ?? 0) + t.cents);
    if (largest === null || t.cents > largest.cents) largest = t;
  });
  for (const [account, cents] of [...totals].sort((a, b) => (a[0] < b[0] ? -1 : 1))) {
    console.log(`${account}: ${money(cents)}`);
  }
  const l = largest as Txn | null;
  console.log(l ? `largest: ${money(l.cents)} to ${l.account} on ${l.date} (line ${l.line})` : "no transactions");
  console.log(`${bad} malformed line(s) skipped`);
  return 0;
}

process.exitCode = main();
