#[derive(Clone, Copy)]
struct Body { x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, mass: f64 }
const PI: f64 = 3.141592653589793;
const SOLAR: f64 = 4.0 * PI * PI;
const DAYS: f64 = 365.24;
fn body(x: f64, y: f64, z: f64, vx: f64, vy: f64, vz: f64, mass: f64) -> Body {
    Body { x, y, z, vx: vx * DAYS, vy: vy * DAYS, vz: vz * DAYS, mass: mass * SOLAR }
}
fn energy(bs: &[Body]) -> f64 {
    let mut e = 0.0;
    for i in 0..bs.len() {
        let b = bs[i];
        e += 0.5 * b.mass * (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz);
        for c in &bs[i + 1..] {
            let (dx, dy, dz) = (b.x - c.x, b.y - c.y, b.z - c.z);
            e -= b.mass * c.mass / (dx * dx + dy * dy + dz * dz).sqrt();
        }
    }
    e
}
fn advance(bs: &mut [Body], dt: f64) {
    let n = bs.len();
    for i in 0..n {
        for j in i + 1..n {
            let (dx, dy, dz) = (bs[i].x - bs[j].x, bs[i].y - bs[j].y, bs[i].z - bs[j].z);
            let d2 = dx * dx + dy * dy + dz * dz;
            let mag = dt / (d2 * d2.sqrt());
            let (mi, mj) = (bs[i].mass, bs[j].mass);
            bs[i].vx -= dx * mj * mag; bs[i].vy -= dy * mj * mag; bs[i].vz -= dz * mj * mag;
            bs[j].vx += dx * mi * mag; bs[j].vy += dy * mi * mag; bs[j].vz += dz * mi * mag;
        }
    }
    for b in bs.iter_mut() { b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz; }
}
fn main() {
    let mut bs = vec![
        Body { x: 0.0, y: 0.0, z: 0.0, vx: 0.0, vy: 0.0, vz: 0.0, mass: SOLAR },
        body(4.84143144246472090, -1.16032004402742839, -0.103622044471123109, 0.00166007664274403694, 0.00769901118419740425, -0.0000690460016972063023, 0.000954791938424326609),
        body(8.34336671824457987, 4.12479856412430479, -0.403523417114321381, -0.00276742510726862411, 0.00499852801234917238, 0.0000230417297573763929, 0.000285885980666130812),
        body(12.894369562139131, -15.111151401698631, -0.22330757889265573, 0.0029646018915851645, 0.0023784717395948095, -0.000029658956854023756, 0.000043662440433515637),
        body(15.379697114850917, -25.919314609987964, 0.17925877295037118, 0.0026806777249038932, 0.001628241700382423, -0.000095159225451971587, 0.000051513890204661145),
    ];
    let (mut px, mut py, mut pz) = (0.0, 0.0, 0.0);
    for b in &bs { px += b.vx * b.mass; py += b.vy * b.mass; pz += b.vz * b.mass; }
    bs[0].vx = -px / SOLAR; bs[0].vy = -py / SOLAR; bs[0].vz = -pz / SOLAR;
    let before = energy(&bs);
    for _ in 0..100000 { advance(&mut bs, 0.01); }
    println!("{} {}", (before * 1e9) as i64, (energy(&bs) * 1e9) as i64);
}
