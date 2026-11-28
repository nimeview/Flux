local parts = {}
for i = 0, 199999 do parts[#parts + 1] = "item" .. (i % 1000) end
local text = table.concat(parts, ",")
local count, total = 0, 0
for w in string.gmatch(text, "[^,]+") do count = count + 1; if w:sub(-1) == "7" then total = total + #w end end
print(count, total)
