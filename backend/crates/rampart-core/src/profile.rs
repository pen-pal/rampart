//! Continuous profiling — the folded-stack model + flamegraph assembly.
//!
//! A *profile* is a set of **samples**; each sample is a call **stack** plus a
//! **value** (CPU nanos, alloc bytes, sample count…). The universal internal
//! representation is a **folded-stack map**: stack → summed value, where a stack
//! is its frames joined leaf-last by `;`:
//!
//! ```text
//! main;runtime;serde_json::from_slice   3812
//! main;runtime;sqlx::execute            1190
//! ```
//!
//! Every wire format we ingest (folded text, pprof, OTLP profiles) is lowered
//! into this map; storage and the flamegraph render off it. This module is pure
//! and unit-tested — the parsing/merging/tree-building logic lives here, away
//! from the DB and HTTP layers. See `docs/design/PROFILING.md`.

use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

/// A folded-stack map: `;`-joined leaf-last stack → summed value.
pub type FoldedMap = BTreeMap<String, i64>;

/// A node in the flamegraph tree. `value` is **inclusive** — the total of every
/// sample whose stack passes through this node — so a node's width is its share
/// of its parent. Serialized to the dashboard as `{name, value, children}`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FlameNode {
    pub name: String,
    pub value: i64,
    pub children: Vec<FlameNode>,
}

/// One row of the "top functions" table: a function's `self` value (samples
/// where it is the leaf — the on-CPU cost of its own code) and `total` value
/// (samples where it appears anywhere in the stack — itself plus callees).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FnStat {
    pub name: String,
    pub self_value: i64,
    pub total_value: i64,
}

/// Parse Brendan-Gregg "folded" text: one `stack value` line per unique stack,
/// frames `;`-joined, value an integer trailing after the last whitespace.
/// Blank lines and lines without a trailing integer are skipped; duplicate
/// stacks accumulate. Leading/trailing whitespace on the stack is trimmed.
pub fn parse_folded(text: &str) -> FoldedMap {
    let mut map = FoldedMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Split on the LAST whitespace run: frames may contain spaces
        // (e.g. `Foo::bar(int, int)`), but the value is always the final token.
        let Some(idx) = line.rfind(char::is_whitespace) else {
            continue;
        };
        let (stack, value) = line.split_at(idx);
        let Ok(value) = value.trim().parse::<i64>() else {
            continue;
        };
        let stack = normalize_stack(stack);
        if stack.is_empty() {
            continue;
        }
        *map.entry(stack).or_insert(0) += value;
    }
    map
}

/// Collapse a stack to its canonical form: drop empty frames, trim each frame,
/// re-join with `;`. Keeps the merge key stable across producers that emit stray
/// `;;` or padded frames.
fn normalize_stack(stack: &str) -> String {
    stack
        .split(';')
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .collect::<Vec<_>>()
        .join(";")
}

/// Sum `src` into `dst` (per-stack value addition). Used to merge every profile
/// in a query window into one map before building the flamegraph.
pub fn merge_into(dst: &mut FoldedMap, src: &FoldedMap) {
    for (stack, value) in src {
        *dst.entry(stack.clone()).or_insert(0) += value;
    }
}

/// Merge an iterator of folded maps into one.
pub fn merge_all<'a, I>(maps: I) -> FoldedMap
where
    I: IntoIterator<Item = &'a FoldedMap>,
{
    let mut out = FoldedMap::new();
    for m in maps {
        merge_into(&mut out, m);
    }
    out
}

/// Build the flamegraph tree from a folded map. Each node carries the inclusive
/// value of all samples passing through it; the root's value is the grand total.
/// Children are sorted by descending value so the hot path renders left-first.
pub fn fold_to_tree(map: &FoldedMap, root_name: &str) -> FlameNode {
    let mut root = FlameNode {
        name: root_name.to_string(),
        value: 0,
        children: Vec::new(),
    };
    for (stack, &value) in map {
        root.value += value;
        let mut node = &mut root;
        for frame in stack.split(';') {
            if frame.is_empty() {
                continue;
            }
            let idx = match node.children.iter().position(|c| c.name == frame) {
                Some(i) => i,
                None => {
                    node.children.push(FlameNode {
                        name: frame.to_string(),
                        value: 0,
                        children: Vec::new(),
                    });
                    node.children.len() - 1
                }
            };
            node = &mut node.children[idx];
            node.value += value;
        }
    }
    sort_tree(&mut root);
    root
}

fn sort_tree(node: &mut FlameNode) {
    node.children
        .sort_by(|a, b| b.value.cmp(&a.value).then(a.name.cmp(&b.name)));
    for c in &mut node.children {
        sort_tree(c);
    }
}

/// Rank functions by self value (own on-CPU cost), descending, top `n`. `total`
/// counts a function once per stack it appears in (recursion isn't double-
/// counted), so `total >= self` always.
pub fn top_functions(map: &FoldedMap, n: usize) -> Vec<FnStat> {
    let mut self_v: BTreeMap<&str, i64> = BTreeMap::new();
    let mut total_v: BTreeMap<&str, i64> = BTreeMap::new();
    for (stack, &value) in map {
        let frames: Vec<&str> = stack.split(';').filter(|f| !f.is_empty()).collect();
        if let Some(leaf) = frames.last() {
            *self_v.entry(leaf).or_insert(0) += value;
        }
        // Unique frames per stack → total isn't inflated by recursion.
        for f in frames.iter().copied().collect::<BTreeSet<_>>() {
            *total_v.entry(f).or_insert(0) += value;
        }
    }
    let mut stats: Vec<FnStat> = total_v
        .into_iter()
        .map(|(name, total_value)| FnStat {
            name: name.to_string(),
            self_value: self_v.get(name).copied().unwrap_or(0),
            total_value,
        })
        .collect();
    stats.sort_by(|a, b| {
        b.self_value
            .cmp(&a.self_value)
            .then(b.total_value.cmp(&a.total_value))
            .then(a.name.cmp(&b.name))
    });
    stats.truncate(n);
    stats
}

/// Serialize a folded map back to canonical folded text (`stack value\n` per
/// line, stacks in sorted order). The inverse of [`parse_folded`] modulo
/// accumulation — what we gzip into storage so every wire format persists in one
/// uniform shape.
pub fn to_folded_text(map: &FoldedMap) -> String {
    let mut out = String::with_capacity(map.len() * 24);
    for (stack, value) in map {
        out.push_str(stack);
        out.push(' ');
        out.push_str(&value.to_string());
        out.push('\n');
    }
    out
}

/// A node in a **diff** flamegraph. `value` is the inclusive value in the
/// *after* window; `delta` is after − before for this exact stack-path
/// (positive = got hotter, negative = got colder / disappeared). Includes paths
/// present in *either* window so regressions and vanished frames both show.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiffNode {
    pub name: String,
    pub value: i64,
    pub delta: i64,
    pub children: Vec<DiffNode>,
}

/// Per-prefix inclusive totals: every stack contributes its value to each of
/// its prefix paths (`a`, `a;b`, `a;b;c`). The basis for both flame trees and
/// diffs without re-walking the tree.
fn path_inclusive(map: &FoldedMap) -> BTreeMap<String, i64> {
    let mut out: BTreeMap<String, i64> = BTreeMap::new();
    for (stack, &value) in map {
        let mut acc = String::new();
        for frame in stack.split(';') {
            if frame.is_empty() {
                continue;
            }
            if !acc.is_empty() {
                acc.push(';');
            }
            acc.push_str(frame);
            *out.entry(acc.clone()).or_insert(0) += value;
        }
    }
    out
}

/// Build a diff flamegraph tree comparing a `before` window to an `after` one.
/// Each node carries its after-inclusive value and the after−before delta; the
/// root totals both. Nodes from either window appear, so a frame that vanished
/// shows as a full-negative delta. Children sorted by `|delta|` descending so
/// the biggest movers render first.
pub fn fold_to_diff_tree(before: &FoldedMap, after: &FoldedMap, root_name: &str) -> DiffNode {
    let bi = path_inclusive(before);
    let ai = path_inclusive(after);
    let after_total: i64 = after.values().sum();
    let before_total: i64 = before.values().sum();
    let mut root = DiffNode {
        name: root_name.to_string(),
        value: after_total,
        delta: after_total - before_total,
        children: Vec::new(),
    };
    // Union of every path in either window; BTreeMap keys iterate sorted, so a
    // parent path is always visited before its children.
    let mut paths: BTreeSet<&str> = BTreeSet::new();
    paths.extend(ai.keys().map(String::as_str));
    paths.extend(bi.keys().map(String::as_str));
    for path in paths {
        let mut node = &mut root;
        let mut acc = String::new();
        for frame in path.split(';') {
            if !acc.is_empty() {
                acc.push(';');
            }
            acc.push_str(frame);
            let idx = match node.children.iter().position(|c| c.name == frame) {
                Some(i) => i,
                None => {
                    node.children.push(DiffNode {
                        name: frame.to_string(),
                        value: 0,
                        delta: 0,
                        children: Vec::new(),
                    });
                    node.children.len() - 1
                }
            };
            node = &mut node.children[idx];
            node.value = ai.get(&acc).copied().unwrap_or(0);
            node.delta = node.value - bi.get(&acc).copied().unwrap_or(0);
        }
    }
    sort_diff_tree(&mut root);
    root
}

fn sort_diff_tree(node: &mut DiffNode) {
    node.children
        .sort_by(|a, b| b.delta.abs().cmp(&a.delta.abs()).then(a.name.cmp(&b.name)));
    for c in &mut node.children {
        sort_diff_tree(c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic_and_accumulate() {
        let m = parse_folded("a;b 10\na;b 5\na;c 3\n");
        assert_eq!(m.get("a;b"), Some(&15));
        assert_eq!(m.get("a;c"), Some(&3));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn parse_tolerates_spaces_in_frames_and_blank_lines() {
        // Frame contains spaces; value is the last token. Blank + junk skipped.
        let m = parse_folded("\nFoo::bar(int, int);baz 7\nnotanumber\n");
        assert_eq!(m.get("Foo::bar(int, int);baz"), Some(&7));
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn parse_normalizes_empty_frames() {
        let m = parse_folded("a;;b; 4\n");
        assert_eq!(m.get("a;b"), Some(&4));
    }

    #[test]
    fn merge_sums_per_stack() {
        let mut a = parse_folded("x;y 2\n");
        let b = parse_folded("x;y 3\nx;z 1\n");
        merge_into(&mut a, &b);
        assert_eq!(a.get("x;y"), Some(&5));
        assert_eq!(a.get("x;z"), Some(&1));
    }

    #[test]
    fn tree_inclusive_values_and_sorted() {
        // main -> a (leaf 10) and main -> b -> c (8). Root = 18.
        let m = parse_folded("main;a 10\nmain;b;c 8\n");
        let t = fold_to_tree(&m, "root");
        assert_eq!(t.value, 18);
        assert_eq!(t.children.len(), 1); // single "main"
        let main = &t.children[0];
        assert_eq!(main.name, "main");
        assert_eq!(main.value, 18);
        // children sorted by value desc: a(10) before b(8)
        assert_eq!(main.children[0].name, "a");
        assert_eq!(main.children[0].value, 10);
        assert_eq!(main.children[1].name, "b");
        assert_eq!(main.children[1].value, 8);
        // b -> c carries the 8
        assert_eq!(main.children[1].children[0].name, "c");
        assert_eq!(main.children[1].children[0].value, 8);
    }

    #[test]
    fn top_functions_self_vs_total() {
        // a is on every stack (total), but never a leaf (self 0). c is a leaf.
        let m = parse_folded("a;b 10\na;b;c 5\n");
        let top = top_functions(&m, 10);
        let by: BTreeMap<_, _> = top.iter().map(|s| (s.name.as_str(), s)).collect();
        assert_eq!(by["a"].total_value, 15);
        assert_eq!(by["a"].self_value, 0);
        assert_eq!(by["b"].total_value, 15);
        assert_eq!(by["b"].self_value, 10); // leaf of the first stack
        assert_eq!(by["c"].self_value, 5);
        // ranked by self desc → b first
        assert_eq!(top[0].name, "b");
    }

    #[test]
    fn folded_text_round_trips() {
        let m = parse_folded("a;b 15\na;c 3\n");
        let text = to_folded_text(&m);
        assert_eq!(parse_folded(&text), m);
    }

    #[test]
    fn diff_tree_deltas() {
        let before = parse_folded("main;a 10\nmain;b 5\n"); // total 15
        let after = parse_folded("main;a 4\nmain;b 5\nmain;c 7\n"); // total 16
        let d = fold_to_diff_tree(&before, &after, "root");
        assert_eq!(d.value, 16);
        assert_eq!(d.delta, 1);
        let main = &d.children[0];
        assert_eq!(main.name, "main");
        assert_eq!(main.delta, 1);
        let by: BTreeMap<_, _> = main.children.iter().map(|c| (c.name.as_str(), c)).collect();
        assert_eq!((by["a"].value, by["a"].delta), (4, -6));
        assert_eq!(by["b"].delta, 0);
        assert_eq!((by["c"].value, by["c"].delta), (7, 7)); // new frame
    }

    #[test]
    fn diff_includes_vanished_frames() {
        let before = parse_folded("gone 3\n");
        let after = parse_folded("kept 2\n");
        let d = fold_to_diff_tree(&before, &after, "root");
        let by: BTreeMap<_, _> = d.children.iter().map(|c| (c.name.as_str(), c)).collect();
        assert_eq!((by["gone"].value, by["gone"].delta), (0, -3));
        assert_eq!(by["kept"].delta, 2);
    }

    #[test]
    fn top_functions_no_recursion_double_count() {
        // a;a;b — "a" appears twice in one stack; total counts the stack once.
        let m = parse_folded("a;a;b 4\n");
        let top = top_functions(&m, 10);
        let a = top.iter().find(|s| s.name == "a").unwrap();
        assert_eq!(a.total_value, 4);
    }
}
