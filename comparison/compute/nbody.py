import math

PI = 3.141592653589793
SOLAR = 4 * PI * PI
DAYS = 365.24

def body(x, y, z, vx, vy, vz, mass):
    return [x, y, z, vx * DAYS, vy * DAYS, vz * DAYS, mass * SOLAR]

bodies = [
    [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, SOLAR],
    body(4.84143144246472090, -1.16032004402742839, -0.103622044471123109, 0.00166007664274403694, 0.00769901118419740425, -0.0000690460016972063023, 0.000954791938424326609),
    body(8.34336671824457987, 4.12479856412430479, -0.403523417114321381, -0.00276742510726862411, 0.00499852801234917238, 0.0000230417297573763929, 0.000285885980666130812),
    body(12.894369562139131, -15.111151401698631, -0.22330757889265573, 0.0029646018915851645, 0.0023784717395948095, -0.000029658956854023756, 0.000043662440433515637),
    body(15.379697114850917, -25.919314609987964, 0.17925877295037118, 0.0026806777249038932, 0.001628241700382423, -0.000095159225451971587, 0.000051513890204661145),
]

def energy(bodies):
    e = 0.0
    n = len(bodies)
    for i in range(n):
        b = bodies[i]
        e += 0.5 * b[6] * (b[3] * b[3] + b[4] * b[4] + b[5] * b[5])
        for j in range(i + 1, n):
            c = bodies[j]
            dx, dy, dz = b[0] - c[0], b[1] - c[1], b[2] - c[2]
            e -= b[6] * c[6] / math.sqrt(dx * dx + dy * dy + dz * dz)
    return e

def advance(bodies, dt):
    n = len(bodies)
    for i in range(n):
        bi = bodies[i]
        for j in range(i + 1, n):
            bj = bodies[j]
            dx, dy, dz = bi[0] - bj[0], bi[1] - bj[1], bi[2] - bj[2]
            d2 = dx * dx + dy * dy + dz * dz
            mag = dt / (d2 * math.sqrt(d2))
            mi, mj = bi[6], bj[6]
            bi[3] -= dx * mj * mag; bi[4] -= dy * mj * mag; bi[5] -= dz * mj * mag
            bj[3] += dx * mi * mag; bj[4] += dy * mi * mag; bj[5] += dz * mi * mag
    for b in bodies:
        b[0] += dt * b[3]; b[1] += dt * b[4]; b[2] += dt * b[5]

px = sum(b[3] * b[6] for b in bodies)
py = sum(b[4] * b[6] for b in bodies)
pz = sum(b[5] * b[6] for b in bodies)
bodies[0][3], bodies[0][4], bodies[0][5] = -px / SOLAR, -py / SOLAR, -pz / SOLAR
before = energy(bodies)
for _ in range(100000):
    advance(bodies, 0.01)
after = energy(bodies)
print(int(before * 1e9), int(after * 1e9))
