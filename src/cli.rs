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

    /// Enable WSDL 1.1 / SOAP web-service-description formatting
    ///
    /// The embedded <types> schema is rendered with the XSD transformations.
    #[arg(long)]
    pub(crate) wsdl: bool,

    /// Enable MSBuild-specific formatting transformations
    ///
    /// Folds a `Condition="..."` attribute (present on almost any MSBuild
    /// element — Target, PropertyGroup, ItemGroup, individual items and
    /// tasks) into a leading `if COND:` guard, with the element's remaining
    /// attributes rendered underneath. Whitespace inside the condition
    /// (MSBuild conditions are often wrapped across lines with `and`/`or`) is
    /// collapsed to a single line.
    #[arg(long)]
    pub(crate) msbuild: bool,

    /// Expand xsl:apply-templates by inlining matching templates from imports
    #[arg(long)]
    pub(crate) expand: bool,

    /// Autodetect the processing mode from each file's extension
    ///
    /// (.xsl/.xslt -> xslt, .sch -> schematron, .xsd -> xsd). Without this
    /// (and without an explicit mode flag) files render as plain XML.
    #[arg(long)]
    pub(crate) auto: bool,

    /// Pipe the rendered output through `bat` for a syntax-highlighted, paged display
    ///
    /// Runs `bat -l unxml`. Implies --auto. Falls back to plain stdout if
    /// `bat` is not installed.
    #[arg(long)]
    pub(crate) bat: bool,

    /// Hide one or more namespace prefixes from element and attribute names
    ///
    /// Cuts noise, e.g. `--hide-ns cbc,cac`. Repeatable and comma-separated;
    /// the matching xmlns: declarations are dropped too. The special value
    /// `--hide-ns ALL` hides every prefix, reducing all names to their bare
    /// local form — handy for fingerprinting documents of unknown vocabularies.
    /// Under --auto/--bat, well-known document types (e.g. UBL) also get a
    /// sensible set hidden automatically.
    #[arg(long, value_delimiter = ',')]
    pub(crate) hide_ns: Vec<String>,

    /// Render only the subtrees whose element name matches this tag
    ///
    /// Renders only matching subtrees instead of the whole document. Matching
    /// is by tag name only (no paths or predicates): a bare name like
    /// `InvoiceLine` matches on the local name so it ignores namespace
    /// prefixes, while a prefixed name like `cac:InvoiceLine` matches the
    /// full name. Each matched subtree is rendered as a top-level fragment.
    #[arg(long)]
    pub(crate) select: Option<String>,

    /// Canonicalise output for diffing
    ///
    /// Rebinds namespace prefixes to stable names (well-known vocabularies
    /// keep their conventional prefix, e.g. `cac`/`xsl`; everything else
    /// becomes `ns1`, `ns2`, … in sorted-URI order) and sorts sibling
    /// elements so prefix- and order-only differences disappear. Mixed
    /// content (prose) keeps its order. Sibling sorting applies only to
    /// plain XML: in a dialect/--special mode (--xslt, --xsd, --wsdl,
    /// --schematron, --msbuild) element order is significant, so only
    /// prefixes are normalised and document order is preserved.
    #[arg(long)]
    pub(crate) canonical: bool,

    /// Dump the distinct element paths as an indented tree instead of the full document
    ///
    /// Each element path is shown once (repeated siblings collapse),
    /// annotated with the union of attribute names ever seen at that path,
    /// under a `//` legend of the namespace prefixes. A compact structural
    /// summary, useful for understanding or comparing document shapes.
    /// Honours --select, --hide-ns and --canonical.
    #[arg(long)]
    pub(crate) paths: bool,

    /// Limit `--paths` output to N nesting levels (root = level 1)
    ///
    /// Deeper subtrees are dropped. Useful for coarser structural signatures
    /// when clustering, or for skimming the top-level shape of a large
    /// document. Only affects `--paths`.
    #[arg(long)]
    pub(crate) depth: Option<usize>,

    /// In `--paths`, drop ordinary attribute names from each node
    ///
    /// Keeps only namespaces (the format identity). Yields a coarser
    /// signature for clustering — incidental per-document attributes
    /// (schemaLocation, version, timestamps) stop fragmenting
    /// otherwise-identical formats. Only affects `--paths`.
    #[arg(long)]
    pub(crate) no_attrs: bool,

    /// In `--paths`, fold repeated subtree shapes into named `@Shape` definitions
    ///
    /// Definitions are listed in a leading `// shapes` legend, replacing each
    /// occurrence with a reference. Collapses structural repetition (e.g.
    /// many date fields sharing the same subtree) so each distinct shape is
    /// shown once. Only affects `--paths`.
    #[arg(long)]
    pub(crate) fold: bool,

    /// Collapse single-child wrapper chains onto a single `parent/child/grandchild` line
    ///
    /// A wrapper chain is one child, no attributes, no text — cutting the
    /// vertical noise of boilerplate scaffolding like UBL's
    /// `ext:UBLExtensions`. With no value every such wrapper folds; with a
    /// comma-separated list of names (e.g. `--collapse=ext:UBLExtensions`) a
    /// chain only *starts* at a listed element, then descends through its
    /// pass-through sub-elements automatically. Names match like --select
    /// (bare = local name, prefixed = full). A list must be joined with `=`
    /// so it is not mistaken for a file argument. Under --auto a sniffed UBL
    /// or CII instance folds all its wrapper chains automatically unless
    /// this is given. Plain XML only; ignored in
    /// --xslt/--xsd/--wsdl/--schematron/--msbuild/--special and --paths modes.
    #[arg(long, require_equals = true, value_delimiter = ',', num_args = 0..)]
    pub(crate) collapse: Option<Vec<String>>,

    /// Read input from stdin (assumes XML format)
    #[arg(long)]
    pub(crate) stdin: bool,

    /// Install the bundled Claude Code skills into `~/.claude/skills/` and exit
    ///
    /// E.g. `unxml/SKILL.md`. Overwrites any existing copies.
    #[arg(long)]
    pub(crate) install_skills: bool,

    /// Register the `.unxml` syntax with `bat`/`batcat`, then exit
    ///
    /// Copies the bundled Sublime grammar into its config dir and rebuilds
    /// the cache. Requires `bat` on PATH. Idempotent; re-run safely.
    #[arg(long)]
    pub(crate) install_bat: bool,

    /// Configure the current git repository to diff XML/HTML through unxml, then exit
    ///
    /// Registers a `textconv` diff driver (`unxml --canonical --auto`) in
    /// repo-local config and binds the usual XML/HTML globs in
    /// `.git/info/attributes`, so `git diff`, `git log -p` and `git show`
    /// render the canonicalised flattened form and prefix- or order-only
    /// churn drops out. Everything lives inside `.git/` — the working tree
    /// is untouched and nothing is committed. Idempotent; re-run safely.
    ///
    /// For a one-off equivalent that touches no git config at all, use
    /// `unxml git <args>` (e.g. `unxml git diff`) instead — it passes the same
    /// textconv driver via `-c` for that single invocation only.
    #[arg(long)]
    pub(crate) init_git: bool,
}
