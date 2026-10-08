//! String tables with suffix sharing: `"bar"` is stored once and `"ar"` points into it.

use alloc::vec::Vec;

/// Builds an ELF string table holding every name in `names`, and returns it with the
/// offset of each name (in input order). The table starts with a NUL, so the empty
/// name is offset 0.
///
/// Names that are suffixes of other names share their bytes, as GNU ld and LLVM do.
/// Sorting the names by their reversed bytes places every name next to the names it is
/// a suffix of, so walking the sorted order from the end and comparing each name with
/// the one before it finds every share.
///
/// The sort is the cost that matters at scale. It is an MSD radix sort over the
/// reversed bytes (see [`sort_reversed`]), so its work is linear in the bytes it must
/// look at and no input can push it to quadratic time, unlike a comparison sort whose
/// every comparison may walk a long shared suffix (mangled names share many). Equal
/// names are interchangeable, so the output depends only on the input.
///
/// Returns `None` if the table would not fit the 32-bit offsets ELF uses.
pub(crate) fn build(names: &[&str]) -> Option<(Vec<u8>, Vec<u32>)> {
    let mut offsets = alloc::vec![0u32; names.len()];
    let total_bytes = names
        .iter()
        .map(|n| n.len())
        .fold(0usize, usize::saturating_add);
    let mut reversed: Vec<u8> = Vec::with_capacity(total_bytes);
    let mut order: Vec<Entry> = Vec::with_capacity(names.len());
    for (index, name) in names.iter().enumerate() {
        if name.is_empty() {
            continue;
        }
        let start = reversed.len();
        reversed.extend(name.bytes().rev());
        order.push(Entry {
            start,
            end: reversed.len(),
            index,
        });
    }
    sort_reversed(&mut order, &reversed);
    // Descending order: every name directly follows a name it is a suffix of, if any.
    order.reverse();

    let total = total_bytes.saturating_add(order.len()).saturating_add(1);
    let mut table = Vec::with_capacity(total);
    table.push(0);

    // (offset, bytes) of the previous name in sorted order.
    let mut previous: Option<(u32, &[u8])> = None;
    for entry in &order {
        let i = entry.index;
        let Some(name) = names.get(i).map(|n| n.as_bytes()) else {
            continue;
        };
        let offset = match previous {
            Some((prev_offset, prev)) if prev.ends_with(name) => {
                let skip = u32::try_from(prev.len() - name.len()).ok()?;
                prev_offset.checked_add(skip)?
            }
            _ => {
                let offset = u32::try_from(table.len()).ok()?;
                table.extend_from_slice(name);
                table.push(0);
                offset
            }
        };
        if let Some(slot) = offsets.get_mut(i) {
            *slot = offset;
        }
        previous = Some((offset, name));
    }
    if u32::try_from(table.len()).is_err() {
        return None;
    }
    Some((table, offsets))
}

/// A non-empty name: where its reversed bytes are, and its position in the input.
struct Entry {
    start: usize,
    end: usize,
    index: usize,
}

/// Below this many names a group is finished with a comparison sort.
const SMALL_GROUP: usize = 32;

/// Sorts names ascending by their reversed bytes (`reversed[start..end]`).
///
/// Iterative MSD radix sort: each group of names that agree on their first `depth`
/// reversed bytes is distributed by the next byte into 257 buckets (bucket 0 for names
/// that end there, which are then equal and done), and each bucket of two or more is
/// pushed as a new group one byte deeper. Groups smaller than [`SMALL_GROUP`] are
/// finished by comparing the remaining bytes. Every byte is inspected a bounded number
/// of times, and an explicit stack replaces recursion, so neither long names nor many
/// names can exhaust the call stack.
fn sort_reversed(entries: &mut [Entry], reversed: &[u8]) {
    let tail = |e: &Entry, depth: usize| -> &[u8] {
        reversed
            .get(e.start.saturating_add(depth)..e.end)
            .unwrap_or(&[])
    };
    let mut scratch: Vec<(usize, usize, usize)> = Vec::new();
    let mut stack: Vec<(usize, usize, usize)> = alloc::vec![(0, entries.len(), 0)];
    while let Some((lo, hi, depth)) = stack.pop() {
        let Some(group) = entries.get_mut(lo..hi) else {
            continue;
        };
        if group.len() < 2 {
            continue;
        }
        if group.len() < SMALL_GROUP {
            group.sort_unstable_by(|a, b| tail(a, depth).cmp(tail(b, depth)));
            continue;
        }
        // Mangled names share long runs (a common suffix, reversed into a common
        // prefix here). Skip the run every name in the group shares in one pass,
        // instead of one bucket pass per byte of it.
        let mut depth = depth;
        if let Some((first, others)) = group.split_first() {
            let head = tail(first, depth);
            let mut shared = head.len();
            for e in others {
                let t = tail(e, depth);
                shared = shared.min(head.iter().zip(t).take_while(|(a, b)| a == b).count());
                if shared == 0 {
                    break;
                }
            }
            depth += shared;
        }
        // Bucket of each entry: 0 if the name ends here, else 1 + its next byte.
        let bucket = |e: &Entry| tail(e, depth).first().map_or(0, |&b| usize::from(b) + 1);
        let mut counts = [0usize; 257];
        for e in group.iter() {
            if let Some(c) = counts.get_mut(bucket(e)) {
                *c += 1;
            }
        }
        let mut starts = [0usize; 257];
        let mut sum = 0;
        for (start, count) in starts.iter_mut().zip(counts.iter()) {
            *start = sum;
            sum += count;
        }
        // Distribute through a scratch copy of (start, end, index), then write back.
        scratch.clear();
        scratch.resize(group.len(), (0, 0, 0));
        let mut next = starts;
        for e in group.iter() {
            if let Some(slot) = next.get_mut(bucket(e)) {
                if let Some(dst) = scratch.get_mut(*slot) {
                    *dst = (e.start, e.end, e.index);
                }
                *slot += 1;
            }
        }
        for (e, &(start, end, index)) in group.iter_mut().zip(scratch.iter()) {
            *e = Entry { start, end, index };
        }
        // Bucket 0 holds names that ended: they are equal, so already in order.
        for (&start, &count) in starts.iter().zip(counts.iter()).skip(1) {
            if count > 1 {
                stack.push((lo + start, lo + start + count, depth + 1));
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail loudly on a missing value")]
mod tests {
    use super::build;

    fn name_at(table: &[u8], offset: u32) -> &str {
        let rest = &table[offset as usize..];
        let end = rest.iter().position(|&b| b == 0).unwrap();
        core::str::from_utf8(&rest[..end]).unwrap()
    }

    #[test]
    fn empty_input_is_a_single_nul() {
        let (table, offsets) = build(&[]).unwrap();
        assert_eq!(table, [0]);
        assert!(offsets.is_empty());
    }

    #[test]
    fn empty_name_is_offset_zero() {
        let (table, offsets) = build(&["", "a", ""]).unwrap();
        assert_eq!(offsets, [0, 1, 0]);
        assert_eq!(table, b"\0a\0");
    }

    #[test]
    fn suffixes_and_duplicates_share_bytes() {
        let names = ["bar", "ar", "r", "bar", "foobar", "baz"];
        let (table, offsets) = build(&names).unwrap();
        for (name, &offset) in names.iter().zip(&offsets) {
            assert_eq!(name_at(&table, offset), *name);
        }
        // Only "foobar" and "baz" need their own bytes.
        assert_eq!(table.len(), 1 + 7 + 4);
    }

    /// Against a quadratic reference: every name must decode from its offset, and the
    /// table must be exactly as small as suffix sharing allows (one copy of each
    /// distinct name that is not a proper suffix of another, plus the leading NUL).
    #[test]
    fn matches_the_optimal_size_on_pseudo_random_names() {
        let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for round in 0..300 {
            let count = (next() % 120) as usize + if round % 7 == 0 { 40 } else { 0 };
            let names: alloc::vec::Vec<alloc::string::String> = (0..count)
                .map(|_| {
                    let len = (next() % 7) as usize + if next() % 9 == 0 { 30 } else { 0 };
                    (0..len)
                        .map(|_| if next() % 3 == 0 { 'a' } else { 'b' })
                        .collect()
                })
                .collect();
            let refs: alloc::vec::Vec<&str> = names.iter().map(|n| n.as_str()).collect();
            let (table, offsets) = build(&refs).unwrap();
            for (name, &offset) in refs.iter().zip(&offsets) {
                assert_eq!(name_at(&table, offset), *name);
            }
            let mut distinct: alloc::vec::Vec<&str> =
                refs.iter().copied().filter(|n| !n.is_empty()).collect();
            distinct.sort_unstable();
            distinct.dedup();
            let optimal: usize = distinct
                .iter()
                .filter(|n| {
                    !distinct
                        .iter()
                        .any(|m| m.len() > n.len() && m.ends_with(**n))
                })
                .map(|n| n.len() + 1)
                .sum::<usize>()
                + 1;
            assert_eq!(table.len(), optimal, "round {round}");
        }
    }

    #[test]
    fn output_is_deterministic() {
        let names = ["x", "yx", "zyx", "q", "", "zz"];
        assert_eq!(build(&names), build(&names));
    }
}
