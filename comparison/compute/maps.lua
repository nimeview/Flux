local counts, size = {}, 0
for i = 0, 199999 do
  local k = "k" .. (i % 5000)
  local old = counts[k]
  if old == nil then size = size + 1; old = 0 end
  counts[k] = old + i % 7
end
local total = 0
for _, v in pairs(counts) do total = total + v end
print(size, total)
