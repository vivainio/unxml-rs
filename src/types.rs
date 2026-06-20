//! Small, dependency-free helpers for XSD/XPath type and attribute handling.

/// Local names of the XSD / XPath built-in atomic types. unxml strips a
/// recognised `xs:` / `xsd:` prefix from these in type position (e.g.
/// `xs:string` → `string`), the way it hides known vocabulary prefixes
/// elsewhere. Anything not on this list keeps its prefix, so a custom type or a
/// stylesheet that bound `xs:` to a different namespace is left untouched.
pub(crate) fn is_builtin_xsd_type(local: &str) -> bool {
    matches!(
        local,
        // 19 primitive types
        "string" | "boolean" | "decimal" | "float" | "double" | "duration"
        | "dateTime" | "time" | "date" | "gYearMonth" | "gYear" | "gMonthDay"
        | "gDay" | "gMonth" | "hexBinary" | "base64Binary" | "anyURI" | "QName"
        | "NOTATION"
        // derived types
        | "normalizedString" | "token" | "language" | "NMTOKEN" | "NMTOKENS"
        | "Name" | "NCName" | "ID" | "IDREF" | "IDREFS" | "ENTITY" | "ENTITIES"
        | "integer" | "nonPositiveInteger" | "negativeInteger" | "long" | "int"
        | "short" | "byte" | "nonNegativeInteger" | "unsignedLong"
        | "unsignedInt" | "unsignedShort" | "unsignedByte" | "positiveInteger"
        // XPath / XQuery data model additions
        | "yearMonthDuration" | "dayTimeDuration" | "dateTimeStamp"
        | "anyAtomicType" | "untypedAtomic" | "untyped" | "numeric" | "error"
    )
}

/// Strip a recognised `xs:` / `xsd:` prefix from a top-level built-in atomic
/// type, preserving any trailing occurrence indicator: `xs:string?` →
/// `string?`. Parametric / structural item types (`element(…)`, `map(…)`,
/// `attribute()*`) are left as is — only a whole `xs:name` / `xsd:name` token
/// is rewritten.
pub(crate) fn simplify_type(t: &str) -> String {
    let (core, occ) = match t.strip_suffix(['?', '*', '+']) {
        Some(c) => (c, &t[c.len()..]),
        None => (t, ""),
    };
    for prefix in ["xs:", "xsd:"] {
        if let Some(local) = core.strip_prefix(prefix)
            && is_builtin_xsd_type(local)
        {
            return format!("{local}{occ}");
        }
    }
    t.to_string()
}

pub(crate) fn is_true(value: Option<&String>) -> bool {
    matches!(value.map(|s| s.as_str()), Some("true") | Some("1"))
}

/// The local name of an XSD element, with any namespace prefix stripped. Inside
/// a schema every structural element is in the XSD namespace whatever prefix it
/// is bound to (`xs:`, `xsd:`, `s:` in .NET schemas, or none), so matching on
/// the bare local name keeps the renderer prefix-agnostic. A foreign element
/// (e.g. from an `xs:any` extension) simply won't match any XSD keyword.
pub(crate) fn xsd_local(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}
