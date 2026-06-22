//! Command-line interface definition.

use clap::Parser;

#[derive(Parser)]
#[command(name = "unxml")]
#[command(about = "Simplify and 'flatten' XML and HTML files")]
#[command(version)]
pub(crate) struct Cli {
    /// XML or HTML files to process (supports glob patterns)
    pub(crate) files: Vec<String>,

    /// Force input format (xml or html). If not specified, format is auto-detected
    #[arg(short, long)]
    pub(crate) format: Option<String>,

    /// Enable proprietary special element handling rules
    #[arg(long)]
    pub(crate) special: bool,

    /// Enable XSLT-specific formatting transformations
    #[arg(long)]
    pub(crate) xslt: bool,

    /// Enable Schematron-specific formatting transformations
    #[arg(long)]
    pub(crate) schematron: bool,

    /// Enable XML Schema (XSD) specific formatting transformations
    #[arg(long)]
    pub(crate) xsd: bool,

    /// Enable WSDL 1.1 / SOAP web-service-description formatting. The embedded
    /// <types> schema is rendered with the XSD transformations.
    #[arg(long)]
    pub(crate) wsdl: bool,

    /// Expand xsl:apply-templates by inlining matching templates from imports
    #[arg(long)]
    pub(crate) expand: bool,

    /// Autodetect the processing mode from each file's extension
    /// (.xsl/.xslt -> xslt, .sch -> schematron, .xsd -> xsd). Without this
    /// (and without an explicit mode flag) files render as plain XML.
    #[arg(long)]
    pub(crate) auto: bool,

    /// Pipe the rendered output through `bat -l unxml` for syntax-highlighted,
    /// paged display. Implies --auto. Falls back to plain stdout if `bat` is
    /// not installed.
    #[arg(long)]
    pub(crate) bat: bool,

    /// Hide one or more namespace prefixes from element and attribute names to
    /// cut noise, e.g. `--hide-ns cbc,cac`. Repeatable and comma-separated; the
    /// matching xmlns: declarations are dropped too. The special value
    /// `--hide-ns ALL` hides every prefix, reducing all names to their bare
    /// local form — handy for fingerprinting documents of unknown vocabularies.
    /// Under --auto/--bat, well-known document types (e.g. UBL) also get a
    /// sensible set hidden automatically.
    #[arg(long, value_delimiter = ',')]
    pub(crate) hide_ns: Vec<String>,

    /// Render only the subtrees whose element name matches this tag, instead of
    /// the whole document. Matching is by tag name only (no paths or
    /// predicates): a bare name like `InvoiceLine` matches on the local name so
    /// it ignores namespace prefixes, while a prefixed name like
    /// `cac:InvoiceLine` matches the full name. Each matched subtree is rendered
    /// as a top-level fragment.
    #[arg(long)]
    pub(crate) select: Option<String>,

    /// Canonicalise output for diffing: rebind namespace prefixes to stable
    /// names (well-known vocabularies keep their conventional prefix, e.g.
    /// `cac`/`xsl`; everything else becomes `ns1`, `ns2`, … in sorted-URI
    /// order) and sort sibling elements so prefix- and order-only differences
    /// disappear. Mixed content (prose) keeps its order. Sibling sorting applies
    /// only to plain XML: in a dialect/--special mode (--xslt, --xsd, --wsdl,
    /// --schematron) element order is significant, so only prefixes are
    /// normalised and document order is preserved.
    #[arg(long)]
    pub(crate) canonical: bool,

    /// Dump the distinct element paths as an indented tree instead of the full
    /// document: each element path is shown once (repeated siblings collapse),
    /// annotated with the union of attribute names ever seen at that path, under
    /// a `//` legend of the namespace prefixes. A compact structural summary,
    /// useful for understanding or comparing document shapes. Honours --select,
    /// --hide-ns and --canonical.
    #[arg(long)]
    pub(crate) paths: bool,

    /// Limit `--paths` output to N nesting levels (root = level 1); deeper
    /// subtrees are dropped. Useful for coarser structural signatures when
    /// clustering, or for skimming the top-level shape of a large document.
    /// Only affects `--paths`.
    #[arg(long)]
    pub(crate) depth: Option<usize>,

    /// In `--paths`, drop ordinary attribute names from each node, keeping only
    /// namespaces (the format identity). Yields a coarser signature for
    /// clustering — incidental per-document attributes (schemaLocation, version,
    /// timestamps) stop fragmenting otherwise-identical formats. Only affects
    /// `--paths`.
    #[arg(long)]
    pub(crate) no_attrs: bool,

    /// Read input from stdin (assumes XML format)
    #[arg(long)]
    pub(crate) stdin: bool,
}
