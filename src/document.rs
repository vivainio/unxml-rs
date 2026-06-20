//! Document-level transforms: extension-based mode detection, namespace
//! hiding, `--select` subtree extraction, and UBL/CII type sniffing.

use std::collections::HashSet;
use std::path::Path;

use crate::model::{FormatOpts, XmlElement};

/// Pick a processing mode from a file's extension when the user hasn't forced
/// one. Mirrors the extension->flag mapping the test suite applies:
///   .xsl / .xslt -> --xslt,  .sch -> --schematron,  .xsd -> --xsd,
///   .wsdl -> --wsdl.
/// `--special` is intentionally excluded: it is proprietary and selected by
/// file name, not extension. Returns the default (no mode) for anything else.
pub(crate) fn detect_mode_from_ext(file_path: &str) -> FormatOpts {
    let ext = Path::new(file_path)
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "xsl" | "xslt" => FormatOpts {
            xslt: true,
            ..FormatOpts::default()
        },
        "sch" => FormatOpts {
            schematron: true,
            ..FormatOpts::default()
        },
        "xsd" => FormatOpts {
            xsd: true,
            ..FormatOpts::default()
        },
        "wsdl" => FormatOpts {
            wsdl: true,
            ..FormatOpts::default()
        },
        _ => FormatOpts::default(),
    }
}

/// Recursively strip the given namespace prefixes from element names and drop
/// the matching `xmlns:<prefix>` declarations. Purely cosmetic: it makes a
/// prefix-heavy vocabulary (e.g. UBL's `cbc:`/`cac:`) read as bare local names
/// while leaving signal-carrying prefixes (e.g. `ext:`/`bim:`) untouched.
pub(crate) fn hide_namespaces(elem: &mut XmlElement, prefixes: &HashSet<String>) {
    if let Some((pfx, local)) = elem.name.split_once(':')
        && prefixes.contains(pfx)
    {
        let pfx = pfx.to_string();
        elem.name = local.to_string();
        // The element's own namespace still identifies the document, so don't
        // simply drop it: re-present its `xmlns:<pfx>` as the default `xmlns`
        // (keeping the URI). This makes an all-prefixed vocabulary like CII read
        // like UBL's default-namespaced root (`CrossIndustryInvoice(xmlns=…)`)
        // rather than losing the namespace entirely.
        if let Some(uri) = elem.attributes.remove(&format!("xmlns:{pfx}")) {
            elem.attributes.entry("xmlns".to_string()).or_insert(uri);
        }
    }
    // Drop the now-redundant xmlns: declarations for the other hidden prefixes.
    elem.attributes
        .retain(|k, _| match k.strip_prefix("xmlns:") {
            Some(pfx) => !prefixes.contains(pfx),
            None => true,
        });
    for child in &mut elem.children {
        hide_namespaces(child, prefixes);
    }
}

/// Whether an element's tag matches a Tier-A `--select` pattern. A pattern
/// containing a `:` matches the full prefixed name; a bare pattern matches the
/// local name (the part after any prefix), so `InvoiceLine` finds
/// `cac:InvoiceLine` and is robust to `--hide-ns` having stripped the prefix.
pub(crate) fn name_matches_select(name: &str, pattern: &str) -> bool {
    if pattern.contains(':') {
        name == pattern
    } else {
        let local = name.rsplit(':').next().unwrap_or(name);
        local == pattern
    }
}

/// Collect the topmost subtrees whose element name matches `pattern` (Tier-A
/// `--select`). Matching is by tag name only — no paths, axes, or predicates.
/// A matched subtree is returned whole and not descended into, so a nested
/// element of the same name doesn't also produce a separate fragment.
pub(crate) fn select_subtrees<'a>(
    elements: &'a [XmlElement],
    pattern: &str,
    out: &mut Vec<&'a XmlElement>,
) {
    for elem in elements {
        if name_matches_select(&elem.name, pattern) {
            out.push(elem);
        } else {
            select_subtrees(&elem.children, pattern, out);
        }
    }
}

/// True if `root` is a genuine UBL *instance* document, i.e. an unprefixed
/// document element (e.g. `<Invoice>`, `<CreditNote>`) whose default namespace
/// is a UBL document schema. This deliberately excludes files that merely
/// *reference* UBL namespaces — an XSLT translating to/from UBL has a prefixed
/// root (`xsl:stylesheet`) and carries literal `cbc:`/`cac:` result elements
/// and XPath that must keep their prefixes.
pub(crate) fn is_ubl_document(root: &XmlElement) -> bool {
    const UBL_NS: &str = "urn:oasis:names:specification:ubl:schema:xsd:";
    !root.name.contains(':')
        && root
            .attributes
            .get("xmlns")
            .is_some_and(|uri| uri.contains(UBL_NS))
}

/// The UN/CEFACT Cross Industry document roots (CII / Factur-X / ZUGFeRD).
/// `CrossIndustryInvoice` is the current (D16B) root; `CrossIndustryDocument`
/// is the older ZUGFeRD 1.0 root.
pub(crate) const CII_ROOTS: [&str; 2] = ["CrossIndustryInvoice", "CrossIndustryDocument"];

/// True if `root` is a genuine CII *instance* document — a `CrossIndustryInvoice`
/// / `CrossIndustryDocument` document element (the root is itself prefixed, e.g.
/// `rsm:CrossIndustryInvoice`) bound to the matching UN/CEFACT namespace. As with
/// UBL, this excludes files that merely *reference* CII: a stylesheet converting
/// to/from CII has a prefixed root (`xsl:stylesheet`) and carries literal
/// `ram:`/`rsm:` result elements that must keep their prefixes.
pub(crate) fn is_cii_document(root: &XmlElement) -> bool {
    let local = root.name.rsplit(':').next().unwrap_or(&root.name);
    CII_ROOTS.contains(&local)
        && root
            .attributes
            .values()
            .any(|uri| uri.contains("uncefact") && CII_ROOTS.iter().any(|r| uri.contains(r)))
}

/// Sniff well-known document types from the root elements' namespace bindings
/// and return the set of prefixes worth hiding. Recognises two families:
/// - UBL: for a genuine UBL instance document, prefixes bound to the
///   CommonBasicComponents / CommonAggregateComponents namespaces.
/// - CII (UN/CEFACT Cross Industry Invoice, incl. Factur-X / ZUGFeRD): for a
///   genuine CII instance, prefixes bound to the Cross Industry document, the
///   ReusableAggregateBusinessInformationEntity (`ram:`), and the un/qualified
///   data type namespaces (`udt:`/`qdt:`).
///
/// Prefixes are matched by URI, so it works regardless of the actual prefix the
/// document chose. Non-matching documents, and stylesheets/schemas that merely
/// reference these vocabularies, contribute nothing.
pub(crate) fn sniff_hidden_prefixes(elements: &[XmlElement]) -> HashSet<String> {
    const UBL_MARKERS: [&str; 2] = ["CommonBasicComponents", "CommonAggregateComponents"];
    const CII_MARKERS: [&str; 5] = [
        "CrossIndustryInvoice",
        "CrossIndustryDocument",
        "ReusableAggregateBusinessInformationEntity",
        "UnqualifiedDataType",
        "QualifiedDataType",
    ];
    let mut hidden = HashSet::new();
    for root in elements {
        let markers: &[&str] = if is_ubl_document(root) {
            &UBL_MARKERS
        } else if is_cii_document(root) {
            &CII_MARKERS
        } else {
            continue;
        };
        for (key, value) in &root.attributes {
            if let Some(pfx) = key.strip_prefix("xmlns:")
                && markers.iter().any(|m| value.contains(m))
            {
                hidden.insert(pfx.to_string());
            }
        }
    }
    hidden
}
