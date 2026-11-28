fn fannkuch(n: usize) -> (i64, i64) {
    let mut perm1: Vec<usize> = (0..n).collect();
    let mut count = vec![0usize; n];
    let mut perm = vec![0usize; n];
    let (mut max_flips, mut checksum, mut sign, mut r) = (0i64, 0i64, 1i64, n);
    loop {
        while r != 1 { count[r - 1] = r; r -= 1; }
        perm.copy_from_slice(&perm1);
        let mut flips = 0;
        let mut first = perm[0];
        while first != 0 { perm[..=first].reverse(); flips += 1; first = perm[0]; }
        max_flips = max_flips.max(flips);
        checksum += sign * flips;
        sign = -sign;
        loop {
            if r == n { return (checksum, max_flips); }
            let p0 = perm1[0];
            for m in 0..r { perm1[m] = perm1[m + 1]; }
            perm1[r] = p0;
            count[r] -= 1;
            if count[r] > 0 { break; }
            r += 1;
        }
    }
}
fn main() { let (c, f) = fannkuch(9); println!("{c} {f}"); }
