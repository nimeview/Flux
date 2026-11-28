local PI = 3.141592653589793
local SOLAR, DAYS = 4 * PI * PI, 365.24
local function body(x, y, z, vx, vy, vz, mass) return { x = x, y = y, z = z, vx = vx * DAYS, vy = vy * DAYS, vz = vz * DAYS, mass = mass * SOLAR } end
local bodies = {
  { x = 0, y = 0, z = 0, vx = 0, vy = 0, vz = 0, mass = SOLAR },
  body(4.84143144246472090, -1.16032004402742839, -0.103622044471123109, 0.00166007664274403694, 0.00769901118419740425, -0.0000690460016972063023, 0.000954791938424326609),
  body(8.34336671824457987, 4.12479856412430479, -0.403523417114321381, -0.00276742510726862411, 0.00499852801234917238, 0.0000230417297573763929, 0.000285885980666130812),
  body(12.894369562139131, -15.111151401698631, -0.22330757889265573, 0.0029646018915851645, 0.0023784717395948095, -0.000029658956854023756, 0.000043662440433515637),
  body(15.379697114850917, -25.919314609987964, 0.17925877295037118, 0.0026806777249038932, 0.001628241700382423, -0.000095159225451971587, 0.000051513890204661145),
}
local sqrt = math.sqrt
local function energy(bs)
  local e, n = 0.0, #bs
  for i = 1, n do
    local b = bs[i]
    e = e + 0.5 * b.mass * (b.vx * b.vx + b.vy * b.vy + b.vz * b.vz)
    for j = i + 1, n do
      local c = bs[j]
      local dx, dy, dz = b.x - c.x, b.y - c.y, b.z - c.z
      e = e - b.mass * c.mass / sqrt(dx * dx + dy * dy + dz * dz)
    end
  end
  return e
end
local function advance(bs, dt)
  local n = #bs
  for i = 1, n do
    local bi = bs[i]
    for j = i + 1, n do
      local bj = bs[j]
      local dx, dy, dz = bi.x - bj.x, bi.y - bj.y, bi.z - bj.z
      local d2 = dx * dx + dy * dy + dz * dz
      local mag = dt / (d2 * sqrt(d2))
      bi.vx = bi.vx - dx * bj.mass * mag; bi.vy = bi.vy - dy * bj.mass * mag; bi.vz = bi.vz - dz * bj.mass * mag
      bj.vx = bj.vx + dx * bi.mass * mag; bj.vy = bj.vy + dy * bi.mass * mag; bj.vz = bj.vz + dz * bi.mass * mag
    end
  end
  for i = 1, n do local b = bs[i]; b.x = b.x + dt * b.vx; b.y = b.y + dt * b.vy; b.z = b.z + dt * b.vz end
end
local px, py, pz = 0.0, 0.0, 0.0
for _, b in ipairs(bodies) do px = px + b.vx * b.mass; py = py + b.vy * b.mass; pz = pz + b.vz * b.mass end
bodies[1].vx, bodies[1].vy, bodies[1].vz = -px / SOLAR, -py / SOLAR, -pz / SOLAR
local before = energy(bodies)
for _ = 1, 100000 do advance(bodies, 0.01) end
local function trunc(x) if x < 0 then return math.ceil(x) end return math.floor(x) end
print(string.format("%d %d", trunc(before * 1e9), trunc(energy(bodies) * 1e9)))
