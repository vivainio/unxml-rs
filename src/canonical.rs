//! Canonicalisation for diff-friendly output (`--canonical`).
//!
//! Two documents that mean the same thing can still differ byte-for-byte in
//! ways that have no semantic weight: namespace *prefixes* are arbitrary local
//! aliases for a URI, and sibling element order is often incidental. This pass
//! removes both sources of noise so the rendered output of equivalent documents
//! diffs cleanly:
//!
//!  1. **Prefix rebinding.** Every prefix is resolved to its namespace URI and
//!     re-expressed with a canonical prefix: recognised vocabularies (XSLT, XSD,
//!     SOAP/WSDL, Schematron, UBL, CII/Factur-X) keep their conventional prefix
//!     (`xsl`, `xs`, `cac`, `ram`, …); everything else gets `ns1`, `ns2`, … in
//!     sorted-URI order. A default namespace (`xmlns="…"`) is rewritten to the
//!     same explicit prefix, so `<a:Foo>` and `<Foo xmlns="…">` for one URI
//!     collapse to the identical name. All `xmlns:*` declarations are re-emitted,
//!     sorted, on the root element.
//!  2. **Sibling sort.** Element-only content is sorted by a recursive signature
//!     so order-only differences disappear. Mixed content (prose interleaved
//!     with markup) keeps document order, where sequence is meaning.
//!
//! Cross-file stability is exact for recognised vocabularies; for unknown URIs
//! the `ns<n>` numbering is stable only for a given *set* of URIs (it follows
//! sorted-URI order), so two files with different URI sets may number a shared
//! URI differently.

use std::collections::{BTreeSet, HashMap};

use crate::model::{NodeRef, XmlElement};

/// The implicit `xml:` namespace. Never declared with `xmlns:` and its prefix
/// is fixed by the spec, so it passes through rewriting untouched.
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";

/// Map a namespace URI to a conventional prefix, or `None` if it is not a
/// recognised vocabulary. W3C / web-service namespaces match exactly; large
/// business vocabularies (UBL, CII) match by substring marker — the same
/// technique the document sniffer uses — so they are robust to version variants.
pub(crate) fn well_known_prefix(uri: &str) -> Option<&'static str> {
    // Exact W3C / SOAP / WSDL / Schematron namespaces. Order is irrelevant
    // because matching is by equality, so e.g. XMLSchema-instance never shadows
    // XMLSchema.
    const EXACT: &[(&str, &str)] = &[
        ("http://www.w3.org/1999/XSL/Transform", "xsl"),
        ("http://www.w3.org/2001/XMLSchema", "xs"),
        ("http://www.w3.org/2001/XMLSchema-instance", "xsi"),
        ("http://www.w3.org/1999/xhtml", "xhtml"),
        ("http://schemas.xmlsoap.org/wsdl/", "wsdl"),
        ("http://schemas.xmlsoap.org/wsdl/soap/", "soapbind"),
        ("http://schemas.xmlsoap.org/wsdl/soap12/", "soap12bind"),
        ("http://schemas.xmlsoap.org/soap/envelope/", "soap"),
        ("http://www.w3.org/2003/05/soap-envelope", "soap12"),
        ("http://purl.oclc.org/dsdl/schematron", "sch"),
    ];
    for (u, p) in EXACT {
        if uri == *u {
            return Some(p);
        }
    }
    // Substring markers for UBL and UN/CEFACT CII (Factur-X / ZUGFeRD).
    const MARKERS: &[(&str, &str)] = &[
        ("CommonBasicComponents", "cbc"),
        ("CommonAggregateComponents", "cac"),
        ("CommonExtensionComponents", "ext"),
        ("ReusableAggregateBusinessInformationEntity", "ram"),
        ("CrossIndustryInvoice", "rsm"),
        ("CrossIndustryDocument", "rsm"),
        ("UnqualifiedDataType", "udt"),
        ("QualifiedDataType", "qdt"),
    ];
    for (m, p) in MARKERS {
        if uri.contains(m) {
            return Some(p);
        }
    }
    None
}

/// Gather every namespace URI declared anywhere in the tree.
fn collect_uris(elem: &XmlElement, out: &mut BTreeSet<String>) {
    for (key, value) in &elem.attributes {
        if key == "xmlns" || key.starts_with("xmlns:") {
            out.insert(value.clone());
        }
    }
    for child in &elem.children {
        collect_uris(child, out);
    }
}

/// Build the stable URI -> canonical-prefix map for the whole document.
/// Recognised vocabularies take their conventional prefix; the remainder are
/// numbered `ns1`, `ns2`, … in sorted-URI order, skipping any number whose name
/// a well-known prefix already claimed.
fn build_prefix_map(roots: &[XmlElement]) -> HashMap<String, String> {
    let mut uris = BTreeSet::new();
    for root in roots {
        collect_uris(root, &mut uris);
    }

    let mut map = HashMap::new();
    let mut used = BTreeSet::new();
    for uri in &uris {
        if let Some(prefix) = well_known_prefix(uri) {
            map.insert(uri.clone(), prefix.to_string());
            used.insert(prefix.to_string());
        }
    }
    let mut n = 1;
    for uri in &uris {
        if map.contains_key(uri) {
            continue;
        }
        let mut candidate = format!("ns{n}");
        while used.contains(&candidate) {
            n += 1;
            candidate = format!("ns{n}");
        }
        used.insert(candidate.clone());
        map.insert(uri.clone(), candidate);
        n += 1;
    }
    map
}

/// Rewrite one qualified name through the in-scope bindings and the canonical
/// map. `is_attr` distinguishes attributes (an unprefixed attribute is never in
/// a namespace, per the XML spec) from elements (an unprefixed element takes the
/// default namespace). Unresolved or unmapped prefixes are left untouched so no
/// information is lost on malformed input.
fn rewrite_qname(
    name: &str,
    scope: &HashMap<String, String>,
    uri2pfx: &HashMap<String, String>,
    is_attr: bool,
) -> String {
    match name.split_once(':') {
        Some(("xml", _)) => name.to_string(), // reserved prefix, fixed by spec
        Some((prefix, local)) => match scope.get(prefix).and_then(|uri| uri2pfx.get(uri)) {
            Some(canonical) => format!("{canonical}:{local}"),
            None => name.to_string(),
        },
        None => {
            if is_attr {
                return name.to_string();
            }
            match scope.get("").filter(|u| !u.is_empty()) {
                Some(uri) => match uri2pfx.get(uri) {
                    Some(canonical) => format!("{canonical}:{name}"),
                    None => name.to_string(),
                },
                None => name.to_string(),
            }
        }
    }
}

/// Recursively rewrite element and attribute names, extending the namespace
/// scope with each element's own declarations before descending. The original
/// `xmlns*` declarations are dropped here; canonical ones are re-emitted on the
/// roots afterwards. `inner_source` is cleared so shallow mixed content renders
/// from `nodes` (with rewritten names) rather than verbatim original XML.
fn rewrite(
    elem: &mut XmlElement,
    parent_scope: &HashMap<String, String>,
    uri2pfx: &HashMap<String, String>,
) {
    let mut scope = parent_scope.clone();
    for (key, value) in &elem.attributes {
        if key == "xmlns" {
            scope.insert(String::new(), value.clone());
        } else if let Some(prefix) = key.strip_prefix("xmlns:") {
            scope.insert(prefix.to_string(), value.clone());
        }
    }

    elem.name = rewrite_qname(&elem.name, &scope, uri2pfx, false);

    let mut new_attrs = HashMap::with_capacity(elem.attributes.len());
    for (key, value) in elem.attributes.drain() {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        new_attrs.insert(rewrite_qname(&key, &scope, uri2pfx, true), value);
    }
    elem.attributes = new_attrs;
    elem.inner_source = None;

    for child in &mut elem.children {
        rewrite(child, &scope, uri2pfx);
    }
}

/// Re-emit the canonical namespace declarations as `xmlns:<prefix>` attributes
/// on each root, sorted on output by the renderer. The implicit `xml` namespace
/// is never declared.
fn emit_decls(roots: &mut [XmlElement], uri2pfx: &HashMap<String, String>) {
    for root in roots.iter_mut() {
        for (uri, prefix) in uri2pfx {
            if uri == XML_NS {
                continue;
            }
            root.attributes
                .insert(format!("xmlns:{prefix}"), uri.clone());
        }
    }
}

/// A stable, order-independent signature of a subtree: name, sorted attributes,
/// trimmed text, then child signatures. Two structurally identical subtrees
/// produce equal signatures regardless of their original sibling position.
fn signature(elem: &XmlElement) -> String {
    let mut attrs: Vec<String> = elem
        .attributes
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    attrs.sort();
    let kids: Vec<String> = elem.children.iter().map(signature).collect();
    // Comments are part of identity: a subtree differing only in a comment must
    // get a distinct signature so the change shows in a diff. Sorted, since the
    // sibling sort reorders them anyway.
    let mut comments: Vec<&str> = elem
        .nodes
        .iter()
        .filter_map(|n| match n {
            NodeRef::Comment { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    comments.sort_unstable();
    format!(
        "{}\u{1}{}\u{1}{}\u{1}{}\u{1}{}",
        elem.name,
        attrs.join("\u{2}"),
        elem.text_content.trim(),
        comments.join("\u{4}"),
        kids.join("\u{3}")
    )
}

/// Sort sibling elements bottom-up by signature. Mixed-content elements are left
/// in document order (their `nodes` interleaving carries meaning); for sorted
/// element-only content `nodes` is rebuilt to match the new child order.
fn sort_tree(elem: &mut XmlElement) {
    for child in &mut elem.children {
        sort_tree(child);
    }
    if !elem.is_mixed() && elem.children.len() > 1 {
        // Salvage comment nodes before the drain; sibling order is being
        // normalised away, so a comment's exact anchor is incidental too. Keep
        // them (sorted, for cross-file stability) after the sorted children so
        // their content still renders and diffs.
        let mut comments: Vec<String> = elem
            .nodes
            .iter()
            .filter_map(|n| match n {
                NodeRef::Comment { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect();
        comments.sort();

        let mut keyed: Vec<(String, XmlElement)> = elem
            .children
            .drain(..)
            .map(|c| (signature(&c), c))
            .collect();
        keyed.sort_by(|a, b| a.0.cmp(&b.0));
        elem.children = keyed.into_iter().map(|(_, c)| c).collect();
        // Reattach comments as standalone: the sort has detached them from the
        // sibling they trailed, so an inline flag would now be misleading.
        elem.nodes = (0..elem.children.len())
            .map(NodeRef::Child)
            .chain(comments.into_iter().map(|text| NodeRef::Comment {
                text,
                inline: false,
            }))
            .collect();
    }
}

/// Canonicalise a parsed document in place: rebind namespace prefixes to stable
/// canonical names so the rendered output of semantically equivalent documents
/// diffs cleanly. When `sort_siblings` is set, also sort sibling elements.
///
/// Sorting is only safe where sibling order is incidental — i.e. plain XML data.
/// In a dialect/`--special` mode (XSLT, XSD, WSDL, Schematron) element order is
/// significant (`xsl:*` control flow, `xs:sequence`, rule order), so the caller
/// passes `sort_siblings = false` there and only the prefix rebinding applies.
/// See the module docs for the guarantees and their limits.
pub(crate) fn canonicalize(roots: &mut [XmlElement], sort_siblings: bool) {
    let uri2pfx = build_prefix_map(roots);
    let empty = HashMap::new();
    for root in roots.iter_mut() {
        rewrite(root, &empty, &uri2pfx);
    }
    emit_decls(roots, &uri2pfx);
    if sort_siblings {
        for root in roots.iter_mut() {
            sort_tree(root);
        }
    }
}
