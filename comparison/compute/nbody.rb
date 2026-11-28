PI = 3.141592653589793
SOLAR = 4 * PI * PI
DAYS = 365.24
Body = Struct.new(:x, :y, :z, :vx, :vy, :vz, :mass)
def body(x, y, z, vx, vy, vz, mass) = Body.new(x, y, z, vx * DAYS, vy * DAYS, vz * DAYS, mass * SOLAR)
bodies = [
  Body.new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0, SOLAR),
  body(4.84143144246472090, -1.16032004402742839, -0.103622044471123109, 0.00166007664274403694, 0.00769901118419740425, -0.0000690460016972063023, 0.000954791938424326609),
  body(8.34336671824457987, 4.12479856412430479, -0.403523417114321381, -0.00276742510726862411, 0.00499852801234917238, 0.0000230417297573763929, 0.000285885980666130812),
  body(12.894369562139131, -15.111151401698631, -0.22330757889265573, 0.0029646018915851645, 0.0023784717395948095, -0.000029658956854023756, 0.000043662440433515637),
  body(15.379697114850917, -25.919314609987964, 0.17925877295037118, 0.0026806777249038932, 0.001628241700382423, -0.000095159225451971587, 0.000051513890204661145),
]
def energy(bs)
  e = 0.0
  bs.each_with_index do |b, i|
    e += 0.5 * b.mass * (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz)
    ((i + 1)...bs.size).each do |j|
      c = bs[j]
      dx = b.x - c.x; dy = b.y - c.y; dz = b.z - c.z
      e -= b.mass * c.mass / Math.sqrt(dx * dx + dy * dy + dz * dz)
    end
  end
  e
end
def advance(bs, dt)
  n = bs.size
  n.times do |i|
    bi = bs[i]
    ((i + 1)...n).each do |j|
      bj = bs[j]
      dx = bi.x - bj.x; dy = bi.y - bj.y; dz = bi.z - bj.z
      d2 = dx * dx + dy * dy + dz * dz
      mag = dt / (d2 * Math.sqrt(d2))
      bi.vx -= dx * bj.mass * mag; bi.vy -= dy * bj.mass * mag; bi.vz -= dz * bj.mass * mag
      bj.vx += dx * bi.mass * mag; bj.vy += dy * bi.mass * mag; bj.vz += dz * bi.mass * mag
    end
  end
  bs.each { |b| b.x += dt * b.vx; b.y += dt * b.vy; b.z += dt * b.vz }
end
px = py = pz = 0.0
bodies.each { |b| px += b.vx * b.mass; py += b.vy * b.mass; pz += b.vz * b.mass }
bodies[0].vx = -px / SOLAR; bodies[0].vy = -py / SOLAR; bodies[0].vz = -pz / SOLAR
before = energy(bodies)
100000.times { advance(bodies, 0.01) }
puts "#{(before * 1e9).to_i} #{(energy(bodies) * 1e9).to_i}"
