signal flag = true
reactive v = if flag { 10 } else { 20 }
print(v)
set flag = false
print(v)
signal n = 2
reactive m = match n { 1 => 100, 2 => 200, _ => 300 }
print(m)
