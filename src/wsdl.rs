//! WSDL 1.1 dialect rendering: web-service descriptions — definitions,
//! types, message/part, portType/operation, binding plus the SOAP extension
//! elements, and service/port. The embedded `<types>` schema is delegated to
//! the XSD renderer via the `or_else` fallback wired up in `render.rs`, so all
//! of the XSD formatting (facets, occurs, type inlining) applies there for free.
//!
//! Targets the dialect WCF emits: WSDL 1.1, SOAP 1.1 (`soap:`) and 1.2
//! (`soap12:`), document/literal, and import-split metadata where `<types>`
//! holds only `<xsd:import>`s rather than an inline schema.

use crate::model::{FormatOpts, XmlElement};
use crate::xslt::TemplateRegistry;

/// Local name: the part after the last `:` (drops any namespace prefix).
fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

/// Namespace prefix, or `""` when the name is unprefixed.
fn ns_prefix(name: &str) -> &str {
    match name.split_once(':') {
        Some((p, _)) => p,
        None => "",
    }
}

/// True when a name is bound to a SOAP prefix (`soap`, `soap12`, `wsoap`…).
/// The SOAP extension elements (binding/operation/body/header/fault/address)
/// reuse WSDL's own local names, so the prefix is what tells them apart in
/// practice — every real WSDL binds SOAP to a distinct namespace.
fn is_soap(name: &str) -> bool {
    ns_prefix(name).contains("soap")
}

/// The standard SOAP-over-HTTP transport URI, implied by every HTTP binding
/// and therefore elided from the rendered `soap`/`soap12` line.
const SOAP_HTTP_TRANSPORT: &str = "http://schemas.xmlsoap.org/soap/http";

/// WSDL/SOAP infrastructure namespaces whose `xmlns:` bindings are noise: we
/// spell `soap`/`schema` as keywords rather than prefixes, so listing these as
/// `ns` lines on the `wsdl` header would add nothing.
fn is_infra_ns(uri: &str) -> bool {
    matches!(
        uri,
        "http://schemas.xmlsoap.org/wsdl/"
            | "http://schemas.xmlsoap.org/wsdl/soap/"
            | "http://schemas.xmlsoap.org/wsdl/soap12/"
            | "http://schemas.xmlsoap.org/wsdl/mime/"
            | "http://schemas.xmlsoap.org/wsdl/http/"
            | "http://www.w3.org/2001/XMLSchema"
    )
}

impl XmlElement {
    pub(crate) fn format_wsdl_element(
        &self,
        indent: usize,
        indent_str: &str,
        registry: Option<&TemplateRegistry>,
    ) -> Option<String> {
        let lname = local_name(&self.name);
        let opts = &FormatOpts {
            wsdl: true,
            ..FormatOpts::default()
        };
        let mut result = String::new();

        match lname {
            "definitions" => {
                let name = self
                    .attributes
                    .get("name")
                    .map(|s| format!(" {s}"))
                    .unwrap_or_default();
                let tns = self
                    .attributes
                    .get("targetNamespace")
                    .map(|s| format!(" {s}"))
                    .unwrap_or_default();
                result.push_str(&format!("{indent_str}wsdl{name}{tns}\n"));

                // Emit the meaningful xmlns bindings (tns + app namespaces) as
                // `ns prefix = uri` lines, dropping the WSDL/SOAP/XSD infra
                // namespaces we render as keywords.
                let inner_indent = "  ".repeat(indent + 1);
                let mut decls: Vec<(String, &String)> = Vec::new();
                for (k, v) in &self.attributes {
                    if is_infra_ns(v) {
                        continue;
                    }
                    if let Some(prefix) = k.strip_prefix("xmlns:") {
                        decls.push((prefix.to_string(), v));
                    } else if k == "xmlns" {
                        decls.push((String::new(), v));
                    }
                }
                decls.sort_by(|a, b| a.0.cmp(&b.0));
                for (prefix, uri) in decls {
                    if prefix.is_empty() {
                        result.push_str(&format!("{inner_indent}xmlns = {uri}\n"));
                    } else {
                        result.push_str(&format!("{inner_indent}ns {prefix} = {uri}\n"));
                    }
                }

                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "types" => {
                result.push_str(&format!("{indent_str}types\n"));
                // Children are xs:schema (or xsd:import) — format_wsdl_element
                // returns None for them, so render.rs routes them to the XSD
                // renderer.
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "import" => {
                // xsd:import (inside <types>) carries schemaLocation — leave it
                // to the XSD renderer. WSDL's own import uses `location`.
                if self.attributes.contains_key("schemaLocation") {
                    return None;
                }
                let ns = self.attributes.get("namespace");
                let loc = self.attributes.get("location");
                match (ns, loc) {
                    (Some(n), Some(l)) => {
                        result.push_str(&format!("{indent_str}import {n} from {l}\n"))
                    }
                    (Some(n), None) => result.push_str(&format!("{indent_str}import {n}\n")),
                    (None, Some(l)) => result.push_str(&format!("{indent_str}import {l}\n")),
                    (None, None) => result.push_str(&format!("{indent_str}import\n")),
                }
                Some(result)
            }
            "message" => {
                let n = self.attributes.get("name")?;
                result.push_str(&format!("{indent_str}message {n}\n"));
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "part" => {
                let n = self.attributes.get("name")?;
                if let Some(e) = self.attributes.get("element") {
                    result.push_str(&format!("{indent_str}part {n} : {e}\n"));
                } else if let Some(t) = self.attributes.get("type") {
                    // Keep the type prefix as written, matching how the XSD
                    // renderer shows element types in the <types> schema above.
                    result.push_str(&format!("{indent_str}part {n} : {t}\n"));
                } else {
                    result.push_str(&format!("{indent_str}part {n}\n"));
                }
                Some(result)
            }
            "portType" => {
                let n = self.attributes.get("name")?;
                result.push_str(&format!("{indent_str}portType {n}\n"));
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "operation" => {
                // A standalone soap:operation (rendered out of a binding op
                // context) just surfaces its SOAP action.
                if is_soap(&self.name) {
                    if let Some(a) = self.attributes.get("soapAction").filter(|a| !a.is_empty()) {
                        result.push_str(&format!("{indent_str}action {a}\n"));
                    }
                    return Some(result);
                }
                let n = self.attributes.get("name")?;
                // In a binding, the soap:operation child carries soapAction.
                // Fold it onto the `op` header line.
                let mut header = format!("{indent_str}op {n}");
                if let Some(so) = self
                    .children
                    .iter()
                    .find(|c| local_name(&c.name) == "operation" && is_soap(&c.name))
                    && let Some(a) = so.attributes.get("soapAction").filter(|a| !a.is_empty())
                {
                    header.push_str(&format!("  action {a}"));
                }
                result.push_str(&header);
                result.push('\n');
                for child in &self.children {
                    if local_name(&child.name) == "operation" && is_soap(&child.name) {
                        continue; // folded into the header above
                    }
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "input" | "output" => {
                let kw = if lname == "input" { "in" } else { "out" };
                // portType context: a message reference.
                if let Some(msg) = self.attributes.get("message") {
                    result.push_str(&format!("{indent_str}{kw} : {msg}\n"));
                    return Some(result);
                }
                // binding context: a soap:body carrying use=literal/encoded.
                if let Some(b) = self
                    .children
                    .iter()
                    .find(|c| local_name(&c.name) == "body" && is_soap(&c.name))
                {
                    let usage = b
                        .attributes
                        .get("use")
                        .map(|s| s.as_str())
                        .unwrap_or("literal");
                    result.push_str(&format!("{indent_str}{kw} : {usage}\n"));
                    // Surface any soap:header parts under the in/out line.
                    for child in &self.children {
                        if local_name(&child.name) == "header" && is_soap(&child.name) {
                            result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                        }
                    }
                    return Some(result);
                }
                result.push_str(&format!("{indent_str}{kw}\n"));
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "fault" => {
                let usage = self.attributes.get("use").map(|s| s.as_str());
                let name = self.attributes.get("name");
                // soap:fault extension — surfaces name + use.
                if is_soap(&self.name) {
                    match (name, usage) {
                        (Some(n), Some(u)) => {
                            result.push_str(&format!("{indent_str}fault {n} : {u}\n"))
                        }
                        (Some(n), None) => result.push_str(&format!("{indent_str}fault {n}\n")),
                        (None, Some(u)) => result.push_str(&format!("{indent_str}fault : {u}\n")),
                        (None, None) => result.push_str(&format!("{indent_str}fault\n")),
                    }
                    return Some(result);
                }
                // portType fault: name + message. binding fault: name + a
                // nested soap:fault.
                if let Some(msg) = self.attributes.get("message") {
                    match name {
                        Some(n) => result.push_str(&format!("{indent_str}fault {n} : {msg}\n")),
                        None => result.push_str(&format!("{indent_str}fault : {msg}\n")),
                    }
                    return Some(result);
                }
                // binding fault (no message): fold a nested soap:fault's `use`
                // onto this line rather than nesting a near-duplicate.
                let soap_use = self
                    .children
                    .iter()
                    .find(|c| local_name(&c.name) == "fault" && is_soap(&c.name))
                    .and_then(|f| f.attributes.get("use"));
                match (name, soap_use) {
                    (Some(n), Some(u)) => {
                        result.push_str(&format!("{indent_str}fault {n} : {u}\n"))
                    }
                    (Some(n), None) => result.push_str(&format!("{indent_str}fault {n}\n")),
                    (None, Some(u)) => result.push_str(&format!("{indent_str}fault : {u}\n")),
                    (None, None) => result.push_str(&format!("{indent_str}fault\n")),
                }
                for child in &self.children {
                    if local_name(&child.name) == "fault" && is_soap(&child.name) {
                        continue; // folded above
                    }
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "binding" => {
                // soap:binding / soap12:binding extension: style + transport.
                if is_soap(&self.name) {
                    let label = if ns_prefix(&self.name).contains("12") {
                        "soap12"
                    } else {
                        "soap"
                    };
                    let style = self
                        .attributes
                        .get("style")
                        .map(|s| s.as_str())
                        .unwrap_or("document");
                    match self.attributes.get("transport") {
                        Some(t) if t != SOAP_HTTP_TRANSPORT => {
                            result.push_str(&format!("{indent_str}{label} {style} over {t}\n"))
                        }
                        _ => result.push_str(&format!("{indent_str}{label} {style}\n")),
                    }
                    return Some(result);
                }
                // WSDL binding: name + the portType it implements.
                let n = self.attributes.get("name")?;
                if let Some(t) = self.attributes.get("type") {
                    result.push_str(&format!("{indent_str}binding {n} : {t}\n"));
                } else {
                    result.push_str(&format!("{indent_str}binding {n}\n"));
                }
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "service" => {
                let n = self.attributes.get("name")?;
                result.push_str(&format!("{indent_str}service {n}\n"));
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "port" | "endpoint" => {
                let n = self.attributes.get("name")?;
                if let Some(b) = self.attributes.get("binding") {
                    result.push_str(&format!("{indent_str}port {n} : {b}\n"));
                } else {
                    result.push_str(&format!("{indent_str}port {n}\n"));
                }
                for child in &self.children {
                    result.push_str(&child.format_yaml_like(indent + 1, opts, registry));
                }
                Some(result)
            }
            "address" if is_soap(&self.name) => {
                if let Some(loc) = self.attributes.get("location") {
                    result.push_str(&format!("{indent_str}address {loc}\n"));
                } else {
                    result.push_str(&format!("{indent_str}address\n"));
                }
                Some(result)
            }
            "body" if is_soap(&self.name) => {
                let usage = self
                    .attributes
                    .get("use")
                    .map(|s| s.as_str())
                    .unwrap_or("literal");
                result.push_str(&format!("{indent_str}body {usage}\n"));
                Some(result)
            }
            "header" if is_soap(&self.name) => {
                let usage = self
                    .attributes
                    .get("use")
                    .map(|s| s.as_str())
                    .unwrap_or("literal");
                match (self.attributes.get("message"), self.attributes.get("part")) {
                    (Some(m), Some(p)) => {
                        result.push_str(&format!("{indent_str}header {m}/{p} : {usage}\n"))
                    }
                    (Some(m), None) => {
                        result.push_str(&format!("{indent_str}header {m} : {usage}\n"))
                    }
                    _ => result.push_str(&format!("{indent_str}header : {usage}\n")),
                }
                Some(result)
            }
            "documentation" => {
                let text = self.text_content.trim();
                if !text.is_empty() {
                    let clean = text.split_whitespace().collect::<Vec<_>>().join(" ");
                    result.push_str(&format!("{indent_str}// {clean}\n"));
                } else {
                    // Prose may be buried in nested markup — recurse so it isn't lost.
                    for child in &self.children {
                        result.push_str(&child.format_yaml_like(indent, opts, registry));
                    }
                }
                Some(result)
            }
            _ => None,
        }
    }
}
