def a(i, j) = 1.0 / ((i + j) * (i + j + 1) / 2 + i + 1)
def mul_av(v, n) = (0...n).map { |i| s = 0.0; n.times { |j| s += a(i, j) * v[j] }; s }
def mul_atv(v, n) = (0...n).map { |i| s = 0.0; n.times { |j| s += a(j, i) * v[j] }; s }
n = 250
u = [1.0] * n
v = []
10.times { v = mul_atv(mul_av(u, n), n); u = mul_atv(mul_av(v, n), n) }
vbv = 0.0; vv = 0.0
n.times { |j| vbv += u[j] * v[j]; vv += v[j] * v[j] }
puts (Math.sqrt(vbv / vv) * 1e9).to_i
