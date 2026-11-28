def make(d) = d == 0 ? [nil, nil] : [make(d - 1), make(d - 1)]
def check(t) = t.nil? ? 0 : 1 + check(t[0]) + check(t[1])
max_depth = 16
total = check(make(max_depth + 1))
long_lived = make(max_depth)
4.step(max_depth, 2) do |depth|
  (1 << (max_depth - depth + 4)).times { total += check(make(depth)) }
end
total += check(long_lived)
puts total
