//! `Array.prototype.sort` as V8 runs it: `TimSort`, ported from V8's `third_party/v8/builtins/array-sort.tq`,
//! so that a comparator which is not a consistent order sorts as it does in Node.
//!
//! - Specification: V8's `array-sort.tq` (Node 22 and 24), itself `CPython`'s `listsort`; proven by
//!   the fixed vectors below, taken from Node, and by conformance gate 1 layer 3's `html` oracle
//!   comparison, whose mocks repeat module names
//!   ([ADR-0009](../../../docs/adr/0009-conformance-suites-as-specification.md))
//! - Plan: [Wave 3, Step 6](../../../docs/plans/pending/0003-wave-3-operations-surface-inner-loop.md#22-steps-for-sub-wave-3b-the-remaining-reporters-and-the-sidecar)
//! - Requirement: [FR-OUT-01](../../../docs/prd.md#fr-out-01)
//!
//! dependency-cruiser's `html` reporter sorts with `(a, b) => key(a) > key(b) ? 1 : -1`, which
//! never answers "equal": two modules with the same key are each "less" than the other, and where
//! they end up depends on every comparison `TimSort` makes. A stable sort would put them elsewhere,
//! so the algorithm is reproduced step for step, runs, binary insertion, galloping and the merge
//! pattern included. The comparator is given as `less(a, b)`, V8's `comparefn(a, b) < 0`, the only
//! question V8 asks of its result.

/// V8's `kMinGallopWins` and the initial `minGallop`.
const MIN_GALLOP: usize = 7;

struct Sorter<'a, T, F> {
    work: &'a mut [T],
    less: F,
    runs: Vec<(usize, usize)>,
    min_gallop: usize,
}

/// `ComputeMinRunLength(n)`.
fn min_run_length(mut n: usize) -> usize {
    let mut r = 0;
    while n >= 64 {
        r |= n & 1;
        n >>= 1;
    }
    n + r
}

impl<T: Clone, F: FnMut(&T, &T) -> bool> Sorter<'_, T, F> {
    fn lt(&mut self, a: &T, b: &T) -> bool {
        (self.less)(a, b)
    }

    /// `BinaryInsertionSort(low, start, high)`.
    fn binary_insertion(&mut self, low: usize, start: usize, high: usize) {
        let mut start = if low == start { start + 1 } else { start };
        while start < high {
            let pivot = self.work[start].clone();
            let (mut left, mut right) = (low, start);
            while left < right {
                let mid = left + ((right - left) >> 1);
                let element = self.work[mid].clone();
                if self.lt(&pivot, &element) {
                    right = mid;
                } else {
                    left = mid + 1;
                }
            }
            self.work[left..=start].rotate_right(1);
            start += 1;
        }
    }

    /// `CountAndMakeRun(low, high)`.
    fn count_and_make_run(&mut self, low_arg: usize, high: usize) -> usize {
        let low = low_arg + 1;
        if low == high {
            return 1;
        }
        let mut run = 2;
        let element_low = self.work[low].clone();
        let element_low_pre = self.work[low - 1].clone();
        let descending = self.lt(&element_low, &element_low_pre);
        let mut previous = element_low;
        for index in low + 1..high {
            let current = self.work[index].clone();
            let order_less = self.lt(&current, &previous);
            if descending != order_less {
                break;
            }
            previous = current;
            run += 1;
        }
        if descending {
            self.work[low_arg..low_arg + run].reverse();
        }
        run
    }

    fn run_length(&self, n: usize) -> usize {
        self.runs[n].1
    }

    /// `RunInvariantEstablished(runs, n)`.
    fn invariant(&self, n: usize) -> bool {
        n < 2 || self.run_length(n - 2) > self.run_length(n - 1) + self.run_length(n)
    }

    /// `MergeCollapse()`.
    fn merge_collapse(&mut self) {
        while self.runs.len() > 1 {
            let mut n = self.runs.len() - 2;
            if !self.invariant(n + 1) || !self.invariant(n) {
                if n > 0 && self.run_length(n - 1) < self.run_length(n + 1) {
                    n -= 1;
                }
                self.merge_at(n);
            } else if self.run_length(n) <= self.run_length(n + 1) {
                self.merge_at(n);
            } else {
                break;
            }
        }
    }

    /// `MergeForceCollapse()`.
    fn merge_force_collapse(&mut self) {
        while self.runs.len() > 1 {
            let mut n = self.runs.len() - 2;
            if n > 0 && self.run_length(n - 1) < self.run_length(n + 1) {
                n -= 1;
            }
            self.merge_at(n);
        }
    }

    /// `MergeAt(i)`.
    fn merge_at(&mut self, i: usize) {
        let (mut base_a, mut length_a) = self.runs[i];
        let (base_b, mut length_b) = self.runs[i + 1];
        self.runs[i] = (base_a, length_a + length_b);
        self.runs.remove(i + 1);
        let key_right = self.work[base_b].clone();
        let k = gallop_right(&mut self.less, self.work, &key_right, base_a, length_a, 0);
        base_a += k;
        length_a -= k;
        if length_a == 0 {
            return;
        }
        let key_left = self.work[base_a + length_a - 1].clone();
        length_b = gallop_left(
            &mut self.less,
            self.work,
            &key_left,
            base_b,
            length_b,
            length_b - 1,
        );
        if length_b == 0 {
            return;
        }
        if length_a <= length_b {
            self.merge_low(base_a, length_a, base_b, length_b);
        } else {
            self.merge_high(base_a, length_a, base_b, length_b);
        }
    }

    /// `MergeLow(baseA, lengthA, baseB, lengthB)`.
    #[expect(
        clippy::too_many_lines,
        reason = "V8's MergeLow kept as one function, so it reads against array-sort.tq line for line"
    )]
    fn merge_low(&mut self, base_a: usize, length_a: usize, base_b: usize, length_b: usize) {
        let (mut length_a, mut length_b) = (length_a, length_b);
        let temp: Vec<T> = self.work[base_a..base_a + length_a].to_vec();
        let (mut dest, mut cursor_temp, mut cursor_b) = (base_a, 0, base_b);
        self.work[dest] = self.work[cursor_b].clone();
        dest += 1;
        cursor_b += 1;
        let copy_b;
        'merge: {
            length_b -= 1;
            if length_b == 0 {
                copy_b = false;
                break 'merge;
            }
            if length_a == 1 {
                copy_b = true;
                break 'merge;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let (mut wins_a, mut wins_b) = (0, 0);
                loop {
                    let b = self.work[cursor_b].clone();
                    if self.lt(&b, &temp[cursor_temp]) {
                        self.work[dest] = b;
                        dest += 1;
                        cursor_b += 1;
                        wins_b += 1;
                        length_b -= 1;
                        wins_a = 0;
                        if length_b == 0 {
                            copy_b = false;
                            break 'merge;
                        }
                        if wins_b >= min_gallop {
                            break;
                        }
                    } else {
                        self.work[dest] = temp[cursor_temp].clone();
                        dest += 1;
                        cursor_temp += 1;
                        wins_a += 1;
                        length_a -= 1;
                        wins_b = 0;
                        if length_a == 1 {
                            copy_b = true;
                            break 'merge;
                        }
                        if wins_a >= min_gallop {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first = true;
                while wins_a >= MIN_GALLOP || wins_b >= MIN_GALLOP || first {
                    first = false;
                    min_gallop = min_gallop.saturating_sub(1).max(1);
                    self.min_gallop = min_gallop;
                    let key = self.work[cursor_b].clone();
                    wins_a = gallop_right(&mut self.less, &temp, &key, cursor_temp, length_a, 0);
                    if wins_a > 0 {
                        self.work[dest..dest + wins_a]
                            .clone_from_slice(&temp[cursor_temp..cursor_temp + wins_a]);
                        dest += wins_a;
                        cursor_temp += wins_a;
                        length_a -= wins_a;
                        if length_a == 1 {
                            copy_b = true;
                            break 'merge;
                        }
                        if length_a == 0 {
                            copy_b = false;
                            break 'merge;
                        }
                    }
                    self.work[dest] = self.work[cursor_b].clone();
                    dest += 1;
                    cursor_b += 1;
                    length_b -= 1;
                    if length_b == 0 {
                        copy_b = false;
                        break 'merge;
                    }
                    let key = temp[cursor_temp].clone();
                    wins_b = gallop_left(&mut self.less, self.work, &key, cursor_b, length_b, 0);
                    if wins_b > 0 {
                        self.work.copy_within_clone(cursor_b, dest, wins_b);
                        dest += wins_b;
                        cursor_b += wins_b;
                        length_b -= wins_b;
                        if length_b == 0 {
                            copy_b = false;
                            break 'merge;
                        }
                    }
                    self.work[dest] = temp[cursor_temp].clone();
                    dest += 1;
                    cursor_temp += 1;
                    length_a -= 1;
                    if length_a == 1 {
                        copy_b = true;
                        break 'merge;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        }
        if copy_b {
            // The last element of run A belongs at the end of the merge.
            self.work.copy_within_clone(cursor_b, dest, length_b);
            self.work[dest + length_b] = temp[cursor_temp].clone();
        } else if length_a > 0 {
            self.work[dest..dest + length_a]
                .clone_from_slice(&temp[cursor_temp..cursor_temp + length_a]);
        }
    }

    /// `MergeHigh(baseA, lengthA, baseB, lengthB)`. Cursors run downwards; a cursor one below
    /// its run's start is kept as `start + 1` offsets through `isize` arithmetic.
    #[expect(
        clippy::too_many_lines,
        reason = "V8's MergeHigh kept as one function, so it reads against array-sort.tq line for line"
    )]
    #[expect(
        clippy::cast_possible_wrap,
        clippy::cast_sign_loss,
        reason = "the cursors of MergeHigh step one below zero before the merge ends, as V8's do; every index taken is in range"
    )]
    fn merge_high(&mut self, base_a: usize, length_a: usize, base_b: usize, length_b: usize) {
        let (mut length_a, mut length_b) = (length_a as isize, length_b as isize);
        let temp: Vec<T> = self.work[base_b..base_b + length_b as usize].to_vec();
        let mut dest = (base_b as isize) + length_b - 1;
        let mut cursor_temp = length_b - 1;
        let mut cursor_a = (base_a as isize) + length_a - 1;
        let at = |i: isize| i as usize;
        self.work[at(dest)] = self.work[at(cursor_a)].clone();
        dest -= 1;
        cursor_a -= 1;
        let copy_a;
        'merge: {
            length_a -= 1;
            if length_a == 0 {
                copy_a = false;
                break 'merge;
            }
            if length_b == 1 {
                copy_a = true;
                break 'merge;
            }
            let mut min_gallop = self.min_gallop;
            loop {
                let (mut wins_a, mut wins_b) = (0_isize, 0_isize);
                loop {
                    let a = self.work[at(cursor_a)].clone();
                    if self.lt(&temp[at(cursor_temp)], &a) {
                        self.work[at(dest)] = a;
                        dest -= 1;
                        cursor_a -= 1;
                        wins_a += 1;
                        length_a -= 1;
                        wins_b = 0;
                        if length_a == 0 {
                            copy_a = false;
                            break 'merge;
                        }
                        if wins_a >= min_gallop as isize {
                            break;
                        }
                    } else {
                        self.work[at(dest)] = temp[at(cursor_temp)].clone();
                        dest -= 1;
                        cursor_temp -= 1;
                        wins_b += 1;
                        length_b -= 1;
                        wins_a = 0;
                        if length_b == 1 {
                            copy_a = true;
                            break 'merge;
                        }
                        if wins_b >= min_gallop as isize {
                            break;
                        }
                    }
                }
                min_gallop += 1;
                let mut first = true;
                while wins_a >= MIN_GALLOP as isize || wins_b >= MIN_GALLOP as isize || first {
                    first = false;
                    min_gallop = min_gallop.saturating_sub(1).max(1);
                    self.min_gallop = min_gallop;
                    let key = temp[at(cursor_temp)].clone();
                    let k = gallop_right(
                        &mut self.less,
                        self.work,
                        &key,
                        base_a,
                        at(length_a),
                        at(length_a - 1),
                    ) as isize;
                    wins_a = length_a - k;
                    if wins_a > 0 {
                        dest -= wins_a;
                        cursor_a -= wins_a;
                        self.work
                            .copy_within_clone(at(cursor_a + 1), at(dest + 1), at(wins_a));
                        length_a -= wins_a;
                        if length_a == 0 {
                            copy_a = false;
                            break 'merge;
                        }
                    }
                    self.work[at(dest)] = temp[at(cursor_temp)].clone();
                    dest -= 1;
                    cursor_temp -= 1;
                    length_b -= 1;
                    if length_b == 1 {
                        copy_a = true;
                        break 'merge;
                    }
                    let key = self.work[at(cursor_a)].clone();
                    let k = gallop_left(
                        &mut self.less,
                        &temp,
                        &key,
                        0,
                        at(length_b),
                        at(length_b - 1),
                    ) as isize;
                    wins_b = length_b - k;
                    if wins_b > 0 {
                        dest -= wins_b;
                        cursor_temp -= wins_b;
                        let (from, to, n) = (at(cursor_temp + 1), at(dest + 1), at(wins_b));
                        self.work[to..to + n].clone_from_slice(&temp[from..from + n]);
                        length_b -= wins_b;
                        if length_b == 1 {
                            copy_a = true;
                            break 'merge;
                        }
                        if length_b == 0 {
                            copy_a = false;
                            break 'merge;
                        }
                    }
                    self.work[at(dest)] = self.work[at(cursor_a)].clone();
                    dest -= 1;
                    cursor_a -= 1;
                    length_a -= 1;
                    if length_a == 0 {
                        copy_a = false;
                        break 'merge;
                    }
                }
                min_gallop += 1;
                self.min_gallop = min_gallop;
            }
        }
        if copy_a {
            // The first element of run B belongs at the front of the merge.
            dest -= length_a;
            cursor_a -= length_a;
            self.work
                .copy_within_clone(at(cursor_a + 1), at(dest + 1), at(length_a));
            self.work[at(dest)] = temp[at(cursor_temp)].clone();
        } else if length_b > 0 {
            let to = at(dest - (length_b - 1));
            self.work[to..to + at(length_b)].clone_from_slice(&temp[..at(length_b)]);
        }
    }
}

/// `copyWithin` for elements that are `Clone` rather than `Copy`: `n` elements from `from` to `to`,
/// correct when the ranges overlap.
trait CopyWithinClone {
    fn copy_within_clone(&mut self, from: usize, to: usize, n: usize);
}

impl<T: Clone> CopyWithinClone for [T] {
    fn copy_within_clone(&mut self, from: usize, to: usize, n: usize) {
        let moved: Vec<T> = self[from..from + n].to_vec();
        self[to..to + n].clone_from_slice(&moved);
    }
}

/// `GallopLeft(array, key, base, length, hint)`.
fn gallop_left<T, F: FnMut(&T, &T) -> bool>(
    less: &mut F,
    array: &[T],
    key: &T,
    base: usize,
    length: usize,
    hint: usize,
) -> usize {
    let (mut last_ofs, mut offset) = (0_usize, 1_usize);
    if less(&array[base + hint], key) {
        let max_ofs = length - hint;
        while offset < max_ofs {
            if !less(&array[base + hint + offset], key) {
                break;
            }
            last_ofs = offset;
            offset = (offset << 1) + 1;
        }
        offset = offset.min(max_ofs);
        last_ofs += hint;
        offset += hint;
    } else {
        let max_ofs = hint + 1;
        while offset < max_ofs {
            if less(&array[base + hint - offset], key) {
                break;
            }
            last_ofs = offset;
            offset = (offset << 1) + 1;
        }
        offset = offset.min(max_ofs);
        // `lastOfs = hint - offset` may be -1 in V8; it is incremented before use.
        let low = hint + 1 - offset;
        offset = hint - last_ofs;
        last_ofs = low;
        return binary_left(less, array, key, base, last_ofs, offset);
    }
    binary_left(less, array, key, base, last_ofs + 1, offset)
}

fn binary_left<T, F: FnMut(&T, &T) -> bool>(
    less: &mut F,
    array: &[T],
    key: &T,
    base: usize,
    mut last_ofs: usize,
    mut offset: usize,
) -> usize {
    while last_ofs < offset {
        let m = last_ofs + ((offset - last_ofs) >> 1);
        if less(&array[base + m], key) {
            last_ofs = m + 1;
        } else {
            offset = m;
        }
    }
    offset
}

/// `GallopRight(array, key, base, length, hint)`.
fn gallop_right<T, F: FnMut(&T, &T) -> bool>(
    less: &mut F,
    array: &[T],
    key: &T,
    base: usize,
    length: usize,
    hint: usize,
) -> usize {
    let (mut last_ofs, mut offset) = (0_usize, 1_usize);
    if less(key, &array[base + hint]) {
        let max_ofs = hint + 1;
        while offset < max_ofs {
            if !less(key, &array[base + hint - offset]) {
                break;
            }
            last_ofs = offset;
            offset = (offset << 1) + 1;
        }
        offset = offset.min(max_ofs);
        let low = hint + 1 - offset;
        offset = hint - last_ofs;
        last_ofs = low;
        return binary_right(less, array, key, base, last_ofs, offset);
    }
    let max_ofs = length - hint;
    while offset < max_ofs {
        if less(key, &array[base + hint + offset]) {
            break;
        }
        last_ofs = offset;
        offset = (offset << 1) + 1;
    }
    offset = offset.min(max_ofs);
    last_ofs += hint;
    offset += hint;
    binary_right(less, array, key, base, last_ofs + 1, offset)
}

fn binary_right<T, F: FnMut(&T, &T) -> bool>(
    less: &mut F,
    array: &[T],
    key: &T,
    base: usize,
    mut last_ofs: usize,
    mut offset: usize,
) -> usize {
    while last_ofs < offset {
        let m = last_ofs + ((offset - last_ofs) >> 1);
        if less(key, &array[base + m]) {
            offset = m;
        } else {
            last_ofs = m + 1;
        }
    }
    offset
}

/// Sorts `items` as V8's `Array.prototype.sort` does with a comparator for which `less(a, b)` is
/// `comparefn(a, b) < 0`.
pub fn sort<T: Clone>(items: &mut [T], less: impl FnMut(&T, &T) -> bool) {
    let length = items.len();
    if length < 2 {
        return;
    }
    let mut sorter = Sorter {
        work: items,
        less,
        runs: Vec::new(),
        min_gallop: MIN_GALLOP,
    };
    let min_run = min_run_length(length);
    let (mut low, mut remaining) = (0, length);
    while remaining != 0 {
        let mut run = sorter.count_and_make_run(low, low + remaining);
        if run < min_run {
            let forced = min_run.min(remaining);
            sorter.binary_insertion(low, low + run, low + forced);
            run = forced;
        }
        sorter.runs.push((low, run));
        sorter.merge_collapse();
        low += run;
        remaining -= run;
    }
    sorter.merge_force_collapse();
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    /// `(key, id)` pairs sorted with dependency-cruiser's `a > b ? 1 : -1` on the key.
    fn upstream_order(keys: &[u32]) -> Vec<usize> {
        let mut items: Vec<(u32, usize)> = keys.iter().copied().zip(0..).collect();
        sort(&mut items, |a, b| a.0 <= b.0);
        items.into_iter().map(|(_, id)| id).collect()
    }

    #[test]
    fn small_arrays_reverse_equal_keys_as_node_does() {
        // node -e 'const a=[2,1,2,1,2].map((k,i)=>({k,i}));
        //          console.log(a.sort((x,y)=>x.k>y.k?1:-1).map(x=>x.i))'
        assert_eq!(upstream_order(&[2, 1, 2, 1, 2]), [3, 1, 4, 2, 0]);
        assert_eq!(upstream_order(&[1, 1, 1]), [2, 1, 0]);
        assert_eq!(upstream_order(&[]), Vec::<usize>::new());
        assert_eq!(upstream_order(&[5]), [0]);
    }

    /// Large arrays, where runs merge and gallop: the FNV-1a hash of the order Node 24 gives,
    /// from
    /// `let s=seed; const r=()=>(s=(s*48271)%2147483647); keys = n × r()%kmax;
    ///  objects.sort((x,y)=>x.k>y.k?1:-1)`, the ids joined with `,`.
    #[test]
    fn large_arrays_merge_and_gallop_as_node_does() {
        let fnv = |text: &str| {
            text.bytes().fold(0x811c_9dc5_u32, |h, b| {
                (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
            })
        };
        for (seed, n, kmax, expected) in [
            (1_u64, 2000, 3, 3_558_080_879_u32),
            (2, 2000, 8, 2_954_060_135),
            (3, 700, 2, 806_390_019),
            (4, 5000, 50, 3_579_712_685),
            (5, 300, 1, 2_036_502_587),
        ] {
            let mut s = seed;
            let keys: Vec<u32> = (0..n)
                .map(|_| {
                    s = (s * 48271) % 2_147_483_647;
                    u32::try_from(s % kmax).unwrap_or(0)
                })
                .collect();
            let order: Vec<String> = upstream_order(&keys)
                .iter()
                .map(ToString::to_string)
                .collect();
            assert_eq!(fnv(&order.join(",")), expected, "seed {seed}");
        }
    }

    #[test]
    fn min_runs_are_v8_s() {
        assert_eq!(min_run_length(63), 63);
        assert_eq!(min_run_length(64), 32);
        assert_eq!(min_run_length(65), 33);
        assert_eq!(min_run_length(2000), 63);
    }

    proptest! {
        #[test]
        fn a_consistent_comparator_sorts_stably(keys in proptest::collection::vec(0u32..6, 0..400)) {
            let mut items: Vec<(u32, usize)> = keys.iter().copied().zip(0..).collect();
            sort(&mut items, |a, b| a.0 < b.0);
            let mut expected: Vec<(u32, usize)> = keys.iter().copied().zip(0..).collect();
            expected.sort_by_key(|(k, _)| *k);
            prop_assert_eq!(items, expected);
        }

        #[test]
        fn an_inconsistent_comparator_still_orders_the_keys(keys in proptest::collection::vec(0u32..6, 0..400)) {
            let order = upstream_order(&keys);
            let mut seen = order.clone();
            seen.sort_unstable();
            prop_assert_eq!(seen, (0..keys.len()).collect::<Vec<_>>());
            let sorted: Vec<u32> = order.iter().map(|&i| keys[i]).collect();
            prop_assert!(sorted.windows(2).all(|w| w[0] <= w[1]));
        }
    }
}
