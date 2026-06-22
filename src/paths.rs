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

use std::collections::{BTreeMap, BTreeSet};

use crate::canonical::well_known_prefix;
use crate::model::XmlElement;

/// What's seen on elements at one path: ordinary attribute names (values vary,
/// so only names are unioned) and the default namespace URI, if the element
/// declares one. Prefixed namespaces are collected document-wide for the legend,
/// not here.
#[derive(Default)]
struct NodeInfo {
    attrs: BTreeSet<String>,
    default_ns: Option<String>,
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

/// Render the distinct element paths of `roots` as an indented tree under a `//`
/// legend of the prefixed namespaces. `max_depth` (0 = unlimited) caps the
/// nesting levels emitted, root being level 1; `no_attrs` drops ordinary
/// attribute names from each node, keeping only namespaces.
pub(crate) fn dump_paths(roots: &[&XmlElement], max_depth: usize, no_attrs: bool) -> String {
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
