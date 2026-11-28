local function a(i, j) return 1.0 / (math.floor(((i + j) * (i + j + 1)) / 2) + i + 1) end
local function mulAv(v, n) local out = {} for i = 0, n - 1 do local s = 0.0 for j = 0, n - 1 do s = s + a(i, j) * v[j + 1] end out[i + 1] = s end return out end
local function mulAtv(v, n) local out = {} for i = 0, n - 1 do local s = 0.0 for j = 0, n - 1 do s = s + a(j, i) * v[j + 1] end out[i + 1] = s end return out end
local n = 250
local u, v = {}, {}
for i = 1, n do u[i] = 1.0 end
for _ = 1, 10 do v = mulAtv(mulAv(u, n), n); u = mulAtv(mulAv(v, n), n) end
local vbv, vv = 0.0, 0.0
for j = 1, n do vbv = vbv + u[j] * v[j]; vv = vv + v[j] * v[j] end
print(string.format("%d", math.floor(math.sqrt(vbv / vv) * 1e9)))
