import math

def a(i, j):
    return 1.0 / ((i + j) * (i + j + 1) // 2 + i + 1)

def mul_av(v, n):
    return [sum(a(i, j) * v[j] for j in range(n)) for i in range(n)]

def mul_atv(v, n):
    return [sum(a(j, i) * v[j] for j in range(n)) for i in range(n)]

n = 250
u = [1.0] * n
v = []
for _ in range(10):
    v = mul_atv(mul_av(u, n), n)
    u = mul_atv(mul_av(v, n), n)
vbv = sum(u[j] * v[j] for j in range(n))
vv = sum(v[j] * v[j] for j in range(n))
print(int(math.sqrt(vbv / vv) * 1000000000.0))
