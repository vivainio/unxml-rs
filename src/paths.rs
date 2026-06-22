//! Path dump (`--paths`): a compact structural summary of a document.
//!
//! Emit the set of *distinct* element paths as an indented tree, each node shown
//! once with the union of attribute names ever seen on elements at that path:
//!
//! ```text
//! // ext = urn:example:ext
//! order(xmlns="urn:shop", id)
//!   line(sku)
//!     qty
//! ```
//!
//! This is a de-duplicated view of "what shapes exist in this document"
//! regardless of how many times each occurs — repeated siblings collapse to one
//! node. The tree falls straight out of the sorted distinct paths: with `/` the
//! lowest separator byte, every node's whole subtree sorts contiguously right
//! after it and before any sibling, so a running prefix diff against the
//! previous path yields correct nesting.
//!
//! Namespaces are split: *prefixed* declarations (`xmlns:ext`) go into a leading
//! `//` legend, keyed by their unambiguous prefix (recognised vocabularies on
//! their conventional prefix — `xsl`, `cac`, … — are omitted as
//! self-explanatory). The *default* namespace (`xmlns`) is different: its key is
//! always `(default)`, so several nested redefinitions would collide in a
//! legend. It is instead shown inline on the element that sets it (the root and
//! any element that redefines it), which also puts the root's namespace — the
//! key format discriminator — right on the root. Composes with `--hide-ns`,
//! `--canonical`, and `--select`, which transform the names / subtree set first.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use crate::canonical::well_known_prefix;
use crate::model::XmlElement;

/// What's seen on elements at one path: ordinary attribute names (values vary,
/// so only names are unioned) and the default namespace URI, if the element
/// declares one. Prefixed namespaces are collected document-wide for the legend,
/// not here.
#[derive(Default, Clone)]
struct NodeInfo {
    attrs: BTreeSet<String>,
    default_ns: Option<String>,
}

/// An explicit node in the distinct-path tree, built from the flat `acc` map.
/// Children are keyed by their (final) segment, so sibling order is the same
/// sorted order the flat walk produces. Used only by `--fold`.
#[derive(Default)]
struct TreeNode {
    info: NodeInfo,
    children: BTreeMap<String, TreeNode>,
}

/// Accumulate `path -> NodeInfo` for every element, unioning across all
/// occurrences of each path, and gather prefixed namespace declarations into
/// `legend`. `depth` is this element's nesting level (root = 1); when
/// `max_depth` is non-zero, elements deeper than it are skipped, yielding a
/// coarser signature (and pruning their namespaces from the legend too).
fn collect(
    elem: &XmlElement,
    parent: &str,
    depth: usize,
    max_depth: usize,
    acc: &mut BTreeMap<String, NodeInfo>,
    legend: &mut BTreeSet<(String, String)>,
) {
    let path = if parent.is_empty() {
        elem.name.clone()
    } else {
        format!("{parent}/{}", elem.name)
    };
    let info = acc.entry(path.clone()).or_default();
    for (key, value) in &elem.attributes {
        if key == "xmlns" {
            info.default_ns = Some(value.clone());
        } else if let Some(prefix) = key.strip_prefix("xmlns:") {
            legend.insert((prefix.to_string(), value.clone()));
        } else {
            info.attrs.insert(key.clone());
        }
    }
    if max_depth == 0 || depth < max_depth {
        for child in &elem.children {
            collect(child, &path, depth + 1, max_depth, acc, legend);
        }
    }
}

/// The parenthesised annotation for a node: its default namespace (with URI)
/// first, then ordinary attribute names — both sorted. Empty when the node has
/// neither. With `no_attrs` the ordinary attribute names are dropped, keeping
/// only the namespace (the format identity), for coarser clustering signatures.
fn annotation(info: &NodeInfo, no_attrs: bool) -> String {
    let mut parts = Vec::new();
    if let Some(uri) = &info.default_ns {
        parts.push(format!("xmlns=\"{}\"", uri.replace('"', "&quot;")));
    }
    if !no_attrs {
        parts.extend(info.attrs.iter().cloned());
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("({})", parts.join(", "))
    }
}

/// Build the explicit distinct-path tree (forest of roots) from the flat
/// `path -> NodeInfo` map. Every ancestor path is itself a key (each element is
/// recorded), so descending segment-by-segment and stamping the info on the
/// final segment reconstructs the tree with no missing nodes.
fn build_forest(acc: &BTreeMap<String, NodeInfo>) -> BTreeMap<String, TreeNode> {
    let mut roots: BTreeMap<String, TreeNode> = BTreeMap::new();
    for (path, info) in acc {
        let segs: Vec<&str> = path.split('/').collect();
        let mut level = &mut roots;
        for (i, seg) in segs.iter().enumerate() {
            let node = level.entry((*seg).to_string()).or_default();
            if i + 1 == segs.len() {
                node.info = info.clone();
            }
            level = &mut node.children;
        }
    }
    roots
}

/// A canonical signature of a node's whole subtree: its display name, its
/// annotation, and — recursively — its children's signatures in sorted order.
/// Two subtrees share a signature iff they are structurally identical (same
/// element name, same attribute/namespace annotation, same descendants). The
/// element name is part of the signature, so a `DATE` and a same-shaped `TIME`
/// do *not* fold together.
fn subtree_sig(name: &str, node: &TreeNode, no_attrs: bool) -> String {
    let mut s = format!("{name}{}", annotation(&node.info, no_attrs));
    if !node.children.is_empty() {
        s.push('{');
        for (cname, child) in &node.children {
            s.push_str(&subtree_sig(cname, child, no_attrs));
            s.push(',');
        }
        s.push('}');
    }
    s
}

/// Whether a node is a *leaf group*: it has children, and every child is itself
/// a leaf (no grandchildren). These are the only nodes `--fold` collapses — one
/// flat level — so a folded shape can never contain another foldable shape.
fn is_leaf_group(node: &TreeNode) -> bool {
    !node.children.is_empty() && node.children.values().all(|c| c.children.is_empty())
}

/// Tally how often each subtree signature occurs across the whole forest, and
/// remember one representative `(name, node)` per signature for emitting the
/// shape definition later.
fn walk_sigs<'a>(
    name: &'a str,
    node: &'a TreeNode,
    no_attrs: bool,
    counts: &mut BTreeMap<String, usize>,
    repr: &mut BTreeMap<String, (&'a str, &'a TreeNode)>,
) {
    let s = subtree_sig(name, node, no_attrs);
    *counts.entry(s.clone()).or_insert(0) += 1;
    repr.entry(s).or_insert((name, node));
    for (cname, child) in &node.children {
        walk_sigs(cname, child, no_attrs, counts, repr);
    }
}

/// The fixed context threaded through `render_node`: how to annotate nodes, the
/// shape names to fold against, the per-line prefix (empty for the tree, `//   `
/// for the shapes legend), and the set of shapes actually referenced (so only
/// used definitions get emitted). Shape *definitions* are rendered with an empty
/// `names` map, so they expand fully — shapes never reference other shapes.
struct RenderCtx<'a> {
    no_attrs: bool,
    names: &'a BTreeMap<String, String>,
    prefix: &'a str,
    used: &'a RefCell<BTreeSet<String>>,
}

/// Render one node (and its subtree) as indented lines into `out`. When the
/// node's subtree matches a named shape, emit a single `@Shape` reference
/// (recording it as used) and stop; otherwise expand the node and recurse.
fn render_node(name: &str, node: &TreeNode, depth: usize, ctx: &RenderCtx, out: &mut String) {
    let indent = "  ".repeat(depth);
    let sig = subtree_sig(name, node, ctx.no_attrs);
    if let Some(shape) = ctx.names.get(&sig) {
        ctx.used.borrow_mut().insert(shape.clone());
        out.push_str(&format!("{}{indent}{shape}\n", ctx.prefix));
        return;
    }
    out.push_str(&format!(
        "{}{indent}{name}{}\n",
        ctx.prefix,
        annotation(&node.info, ctx.no_attrs)
    ));
    for (cname, child) in &node.children {
        render_node(cname, child, depth + 1, ctx, out);
    }
}

/// Render the distinct element paths of `roots` as an indented tree under a `//`
/// legend of the prefixed namespaces. `max_depth` (0 = unlimited) caps the
/// nesting levels emitted, root being level 1; `no_attrs` drops ordinary
/// attribute names from each node, keeping only namespaces. With `fold`,
/// repeated subtree shapes are hoisted into a `// shapes` legend and each
/// occurrence is replaced by an `@Shape` reference.
pub(crate) fn dump_paths(
    roots: &[&XmlElement],
    max_depth: usize,
    no_attrs: bool,
    fold: bool,
) -> String {
    let mut acc = BTreeMap::new();
    let mut legend = BTreeSet::new();
    for root in roots {
        collect(root, "", 1, max_depth, &mut acc, &mut legend);
    }

    let mut out = String::new();

    // Legend for prefixed namespaces. Skip recognised vocabularies bound to
    // their conventional prefix (`xsl`, `cac`, …): self-explanatory, so listing
    // them is noise. A non-standard prefix on a well-known URI still gets a line.
    for (prefix, uri) in &legend {
        if well_known_prefix(uri) == Some(prefix.as_str()) {
            continue;
        }
        out.push_str(&format!("// {prefix} = {uri}\n"));
    }

    if fold {
        return dump_folded(out, &acc, no_attrs);
    }

    // Walk the sorted paths, emitting only the segments that differ from the
    // previous path (its subtree is contiguous, so this nests correctly). A
    // node's annotation attaches to its final segment; intermediate segments are
    // each some ancestor path's final segment, already emitted with their own.
    let mut prev: Vec<&str> = Vec::new();
    for (path, info) in &acc {
        let segs: Vec<&str> = path.split('/').collect();
        let mut shared = 0;
        while shared < prev.len() && shared < segs.len() && prev[shared] == segs[shared] {
            shared += 1;
        }
        for (depth, seg) in segs.iter().enumerate().skip(shared) {
            let indent = "  ".repeat(depth);
            if depth + 1 == segs.len() {
                out.push_str(&format!("{indent}{seg}{}\n", annotation(info, no_attrs)));
            } else {
                out.push_str(&format!("{indent}{seg}\n"));
            }
        }
        prev = segs;
    }
    out
}

/// `--fold` rendering: build the explicit tree, name every subtree shape that
/// recurs (≥2 occurrences and non-leaf), emit a `// shapes` legend defining each
/// once, then render the tree with every occurrence collapsed to its `@Shape`
/// reference. `out` already holds the namespace legend.
fn dump_folded(mut out: String, acc: &BTreeMap<String, NodeInfo>, no_attrs: bool) -> String {
    let forest = build_forest(acc);

    let mut counts = BTreeMap::new();
    let mut repr = BTreeMap::new();
    for (name, node) in &forest {
        walk_sigs(name, node, no_attrs, &mut counts, &mut repr);
    }

    // Name each repeated *leaf-group* shape `@Local` after its root element's
    // local name (collisions get a numeric suffix). Only nodes whose children
    // are all leaves are folded: a single flat level, never deeper structure, so
    // shapes can never nest inside one another (no recursive definitions).
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let mut used: BTreeSet<String> = BTreeSet::new();
    for (sig, count) in &counts {
        let (repr_name, node) = repr[sig];
        if *count < 2 || !is_leaf_group(node) {
            continue;
        }
        let local = repr_name.rsplit(':').next().unwrap_or(repr_name);
        let mut shape = format!("@{local}");
        let mut n = 2;
        while used.contains(&shape) {
            shape = format!("@{local}{n}");
            n += 1;
        }
        used.insert(shape.clone());
        names.insert(sig.clone(), shape);
    }

    // Render the tree first, into a buffer, folding each outermost repeated
    // subtree into its `@Shape` reference (and recording which shapes are
    // actually referenced). Folded subtrees are not descended into, so a shape
    // that only ever nests inside another folded shape is never referenced.
    let used = RefCell::new(BTreeSet::new());
    let tree_ctx = RenderCtx {
        no_attrs,
        names: &names,
        prefix: "",
        used: &used,
    };
    let mut tree = String::new();
    for (name, node) in &forest {
        render_node(name, node, 0, &tree_ctx, &mut tree);
    }
    let used = used.into_inner();

    // Shapes legend: only the referenced shapes, ordered by name for stable
    // reading. Each is a leaf group, so its definition fits on one line —
    // `@Shape = root{rootattrs} { child, child(attr), … }` — with braces around
    // the child list (parens stay reserved for attributes, as everywhere else).
    let mut defs: Vec<(&String, &String)> = names
        .iter()
        .filter(|(_, shape)| used.contains(*shape))
        .collect();
    defs.sort_by(|a, b| a.1.cmp(b.1));
    if !defs.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("// shapes\n");
        for (sig, shape) in defs {
            let (root_name, node) = repr[sig];
            let children = node
                .children
                .iter()
                .map(|(cname, child)| format!("{cname}{}", annotation(&child.info, no_attrs)))
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!(
                "//   {shape} = {root_name}{} {{ {children} }}\n",
                annotation(&node.info, no_attrs)
            ));
        }
        out.push('\n');
    }

    out.push_str(&tree);
    out
}
