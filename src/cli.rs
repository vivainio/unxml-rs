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

    /// Hide one or more namespace prefixes from element names to cut noise,
    /// e.g. `--hide-ns cbc,cac`. Repeatable and comma-separated. The matching
    /// xmlns: declarations are dropped too. Under --auto/--bat, well-known
    /// document types (e.g. UBL) also get a sensible set hidden automatically.
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

    /// Read input from stdin (assumes XML format)
    #[arg(long)]
    pub(crate) stdin: bool,
}
