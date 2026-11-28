local function make(d) if d == 0 then return { false, false } end return { make(d - 1), make(d - 1) } end
local function check(t) if not t then return 0 end return 1 + check(t[1]) + check(t[2]) end
local maxDepth = 16
local total = check(make(maxDepth + 1))
local longLived = make(maxDepth)
for depth = 4, maxDepth, 2 do
  local iterations = 1 << (maxDepth - depth + 4)
  for _ = 1, iterations do total = total + check(make(depth)) end
end
total = total + check(longLived)
print(total)
