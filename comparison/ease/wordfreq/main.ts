import { readFileSync } from "node:fs";

function countWords(text: string): Map<string, number> {
  const counts = new Map<string, number>();
  const words = text.toLowerCase().replace(/[^a-z0-9]+/g, " ").split(" ").filter((w) => w !== "");
  for (const w of words) counts.set(w, (counts.get(w) ?? 0) + 1);
  return counts;
}

function main(): number {
  const argv = process.argv.slice(2);
  if (argv.length < 1) {
    console.error("usage: wordfreq FILE");
    return 1;
  }
  let text: string;
  try {
    text = readFileSync(argv[0], "utf8");
  } catch (e) {
    console.error(`wordfreq: ${(e as Error).message}`);
    return 1;
  }
  const ranked = [...countWords(text)].sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
  for (const [w, n] of ranked.slice(0, 10)) console.log(`${n} ${w}`);
  return 0;
}

process.exitCode = main();
