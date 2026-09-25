//! Command-line interface definition.

use clap::Parser;

#[derive(Parser)]
#[command(name = "unxml")]
#[command(about = "Simplify and 'flatten' XML and HTML files")]
#[command(version)]
pub(crate) struct Cli {
    /// XML or HTML files to process (supports glob patterns; see also --zip)
    pub(crate) files: Vec<String>,

    /// Force input format (xml, html, or json). If omitted, it is auto-detected
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

    /// Enable Leo (leo-editor) outline formatting for `.leo` files
    ///
    /// Joins the `<vnodes>` outline tree with the body text held separately in
    /// `<tnodes>`, rendering each headline followed by its body indented
    /// underneath. Clones (a node appearing at more than one outline position)
    /// show their body once, at the first occurrence; later occurrences render
    /// as `headline (clone)`. Bookkeeping noise (gnx ids, expansion/mark
    /// state, the `<leo_header>`/`<globals>`/`<preferences>` boilerplate) is
    /// dropped.
    #[arg(long)]
    pub(crate) leo: bool,

    /// Expand xsl:apply-templates by inlining matching templates from imports
    #[arg(long)]
    pub(crate) expand: bool,

    /// Autodetect the processing mode from each file's extension or content
    ///
    /// (.xsl/.xslt -> xslt, .sch -> schematron, .xsd -> xsd). For JSON,
    /// simplifies standalone JSON Schema documents and Schema Objects embedded
    /// at known OpenAPI locations. Without this, inputs use generic rendering.
    #[arg(long)]
    pub(crate) auto: bool,

    /// Cancel the --auto that --bat/--html/--cat otherwise imply
    ///
    /// Those three flags default to acting as if --auto were also given
    /// (extension-based mode detection, plus sniffing the document type to
    /// infer namespace hiding and wrapper-chain folding). This cancels that,
    /// so e.g. `--html --no-auto` renders the exact literal output `unxml`
    /// alone would (no mode, no hidden prefixes) while still highlighting it
    /// via --html/--cat's native grammar. Has no effect together with an
    /// explicit --auto.
    #[arg(long)]
    pub(crate) no_auto: bool,

    /// Pipe the rendered output through `bat` for a syntax-highlighted, paged display
    ///
    /// Runs `bat -l unxml`. Implies --auto. Falls back to plain stdout if
    /// `bat` is not installed.
    #[arg(long)]
    pub(crate) bat: bool,

    /// Render output as syntax-highlighted HTML instead of plain text
    ///
    /// Uses the same bundled Sublime grammar as --bat, via `syntect` — no
    /// `bat` or Python required. Implies --auto. Writes a standalone page to
    /// stdout that links an external `unxml.css`; generate that once with
    /// --html-css and keep both files in the same directory.
    #[arg(long, conflicts_with_all = ["bat", "cat"])]
    pub(crate) html: bool,

    /// Print the rendered output with ANSI syntax highlighting, no pager
    ///
    /// Like --html but for the terminal: same bundled grammar/theme via
    /// `syntect`, escaped straight to stdout. Unlike --bat this never shells
    /// out to an external `bat`/`batcat` and never pages. Implies --auto.
    #[arg(long, conflicts_with_all = ["bat", "html"])]
    pub(crate) cat: bool,

    /// Print the stylesheet `--html` pages link as `unxml.css`, then exit
    ///
    /// e.g. `unxml --html-css > unxml.css`. Only needs regenerating if the
    /// bundled highlighting theme changes.
    #[arg(long)]
    pub(crate) html_css: bool,

    /// With --html, embed the stylesheet inline instead of linking `unxml.css`
    ///
    /// Produces one fully self-contained HTML file with no sibling
    /// stylesheet to keep alongside it — handy for a one-off page shared or
    /// moved on its own. Without this, --html links `unxml.css`, which you
    /// generate once with --html-css and reuse across every page.
    #[arg(long, requires = "html")]
    pub(crate) html_embed_css: bool,

    /// With --html or --cat, highlight the original source instead of unxml's output
    ///
    /// Skips the unxml transform: the file is read as-is and syntax-
    /// highlighted as XML or HTML (same format detection as normal
    /// processing) using syntect's own bundled grammar for that language,
    /// not the unxml one. A dependency-free stand-in for `bat -l xml`
    /// piped through `ansi2html` when you just want the original source
    /// highlighted, e.g. next to unxml's output. Requires --html or --cat.
    #[arg(long)]
    pub(crate) raw: bool,

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

    /// Render only the elements selected by this XPath-like pattern
    ///
    /// A small XPath subset: `/a/b` (from the root), `a/b` or `//a`
    /// (anywhere), `a//b`, `*`, `..` (parent), `.`, and chainable
    /// predicates: `[@attr]`, `[@attr="v"]`, `[child="v"]` (a child's
    /// text), `[.="v"]` / `[text()="v"]` (own text), `[a/b/@c="v"]` (any
    /// relative path, with its own predicates), `[contains(X, "v")]`, e.g.
    /// `item[@id="2"]`, `order[@status='open']/line`, `qty[@unit="kg"]/..`,
    /// `call[param[@name="command"]="decrypt"]`. Unlike XPath, a relative
    /// pattern matches anywhere (`item` = `//item`), `=` ignores whitespace
    /// around the document's text, and a bare name like `InvoiceLine`
    /// matches the local name, ignoring namespace prefixes, while
    /// `cac:InvoiceLine` matches the full name (attribute names likewise).
    /// No other axes, positions, operators or functions.
    /// Each selected element is rendered as a top-level fragment; one inside
    /// another selected element is shown only as part of it. Documents with
    /// no match produce no output, not even a `// FILE:` header, so this
    /// doubles as a search over many files or `--zip` archives. XML files
    /// are text-scanned for the pattern's names before parsing, and skipped
    /// when one is missing.
    #[arg(long)]
    pub(crate) select: Option<String>,

    /// Print one JSON object per hit, one per line (JSON Lines)
    ///
    /// For scripts and agents consuming a search. Each record has `file`
    /// (plus `archive`/`entry` for a zip entry), `path` (the `name[k]/...`
    /// anchor `unxml diff`/`patch` use), `name`, `attrs`, `line_range`
    /// (1-indexed, inclusive), `byte_range` (half-open, into the file's —
    /// or zip entry's — original bytes, to read the exact segment back),
    /// `text` (the rendered unxml form, honouring modes like --special) and
    /// `xml` (the raw source segment). Positions are omitted for HTML.
    /// Without --select, each document root is one hit.
    #[arg(long, conflicts_with_all = ["paths", "html", "cat", "bat", "raw", "files_with_matches"])]
    pub(crate) jsonl: bool,

    /// Print only the names of inputs with at least one --select hit
    #[arg(short = 'l', long, conflicts_with_all = ["paths", "html", "cat", "bat", "raw"])]
    pub(crate) files_with_matches: bool,

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

    /// Also process every XML/HTML entry inside these zip archives
    ///
    /// Repeatable, and glob patterns are supported (`--zip 'dumps/*.zip'`).
    /// Any zip container works (`.zip`, `.jar`, `.docx`, ...). Entries whose
    /// content doesn't start with `<` are skipped. Each entry is shown as
    /// `archive.zip!/path/in/archive.xml`, and its mode is picked from that
    /// inner name under --auto. Archives are processed after plain files.
    ///
    /// That `archive.zip!/entry` form also works as a plain file argument
    /// (or here) to read just those entries — paste a `// FILE:` name from a
    /// search to dump the whole document. The entry part may be a glob,
    /// e.g. `'bundle.zip!/orders/*.xml'`.
    #[arg(long, value_name = "ARCHIVE")]
    pub(crate) zip: Vec<String>,

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
}
