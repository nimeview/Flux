const WIDTH = 12;
const TICKS = 20;
const TICK_MS = 20;

let pos = 0;
let vel = 1;
let ticks = 0;
let bounces = 0;
let timer: ReturnType<typeof setInterval> | undefined;

function draw(p: number): string {
  let s = "|";
  for (let i = 0; i <= WIDTH; i++) s += i === p ? "o" : ".";
  return s + "|";
}

const script = new Map<number, number>([[5, 2], [11, -3], [16, 1]]);

function countdown(n: number) {
  if (n > 0) {
    console.log(`${n}...`);
    setTimeout(() => countdown(n - 1), 50);
  } else {
    console.log("go!");
    timer = setInterval(tick, TICK_MS);
  }
}

function tick() {
  ticks++;
  const kick = script.get(ticks);
  if (kick !== undefined) {
    console.log(`  kick: velocity ${vel} -> ${kick}`);
    vel = kick;
  }
  let next = pos + vel;
  const before = bounces;
  if (next < 0) { next = -next; vel = -vel; bounces++; }
  else if (next > WIDTH) { next = 2 * WIDTH - next; vel = -vel; bounces++; }
  pos = next;
  console.log(`tick ${ticks}: ${draw(pos)}`);
  if (bounces !== before) console.log(`  bounce #${bounces}`);
  if (ticks >= TICKS) clearInterval(timer);
}

countdown(3);
