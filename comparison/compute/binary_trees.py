import sys
sys.setrecursionlimit(10000)

def make(depth):
    if depth == 0:
        return (None, None)
    return (make(depth - 1), make(depth - 1))

def check(t):
    if t is None:
        return 0
    return 1 + check(t[0]) + check(t[1])

def main():
    max_depth = 16
    total = check(make(max_depth + 1))
    long_lived = make(max_depth)
    depth = 4
    while depth <= max_depth:
        iterations = 1 << (max_depth - depth + 4)
        for _ in range(iterations):
            total += check(make(depth))
        depth += 2
    total += check(long_lived)
    print(total)

main()
