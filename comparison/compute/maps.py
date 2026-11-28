counts = {}
for i in range(200000):
    key = f"k{i % 5000}"
    counts[key] = counts.get(key, 0) + i % 7
print(len(counts), sum(counts.values()))
