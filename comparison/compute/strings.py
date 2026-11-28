parts = [f"item{i % 1000}" for i in range(200000)]
text = ",".join(parts)
words = text.split(",")
total = sum(len(w) for w in words if w.endswith("7"))
print(len(words), total)
