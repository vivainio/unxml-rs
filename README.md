# Unxml

Simplify and "flatten" XML files into a YAML-like readable format.

This is a Rust clone of the original [unxml](https://github.com/vivainio/unxml) F# tool.

**[See it in action →](https://vivainio.github.io/unxml-demos/)** — a gallery of
real-world XML documents, schemas, stylesheets, and Schematron rules rendered
with `unxml`, with original-vs-rendered size comparisons.

## Installation

### Using uv (Easiest)

Install the published wheel from PyPI as a standalone tool:

```bash
uv tool install unxml-rs
```

This puts the `unxml` command on your PATH. To try it without installing anything:

```bash
uvx --from unxml-rs unxml <xml_file>
```

### Pre-built Binaries (Recommended)

Download the latest release for your platform from the [GitHub Releases](https://github.com/yourusername/unxml-rs/releases) page:

- **Linux (x86_64)**: `unxml-linux-x86_64.tar.gz`
- **Windows (x86_64)**: `unxml-windows-x86_64.zip`
- **macOS (Intel)**: `unxml-macos-x86_64.tar.gz`
- **macOS (Apple Silicon)**: `unxml-macos-arm64.tar.gz`

Extract the archive and place the `unxml` binary in your PATH.

### From Source

```bash
git clone https://github.com/yourusername/unxml-rs
cd unxml-rs
cargo install --path .
```

### Using Cargo

```bash
cargo install unxml
```

## Usage

```bash
unxml <xml_file>
```

By default files render as plain XML. Pass `--auto` to pick the processing mode
from each file's extension:

| Extension      | Mode applied   |
| -------------- | -------------- |
| `.xsl` `.xslt` | `--xslt`       |
| `.sch`         | `--schematron` |
| `.xsd`         | `--xsd`        |

An explicit mode flag (`--xslt`, `--schematron`, `--xsd`, `--special`) always
overrides autodetection.

Each mode rewrites its vocabulary into a terser pseudocode. The full set of
transformations, with side-by-side samples, is documented per format:

- [XSLT transformations](docs/xslt.md) — `xsl:*` stylesheets
- [XSD transformations](docs/xsd.md) — `xs:*` / `xsd:*` schemas
- [Schematron transformations](docs/schematron.md) — `.sch` rule schemas

### Syntax-highlighted output (`--bat`)

```bash
unxml --bat some.xsd      # implies --auto (detects --xsd), pipes through `bat -l unxml`
```

`--bat` renders the output through [`bat`](https://github.com/sharkdp/bat) using
the bundled `unxml` grammar (see `editor/`) for paged, colourised display. If
`bat` is not installed it falls back to plain stdout.

### Hiding noisy namespace prefixes (`--hide-ns`)

Vocabularies like UBL bury the signal under repeated prefixes (`cbc:`, `cac:`).
`--hide-ns` drops the named prefixes from element **and attribute** names — and
their `xmlns:` declarations — so the output reads as bare local names:

```bash
unxml --hide-ns cbc,cac invoice.xml   # repeatable and comma-separated
```

Signal-carrying prefixes you don't list (e.g. `ext:`, `bim:`) are kept, so an
extension subtree still stands out.

The special value `--hide-ns ALL` hides **every** prefix, reducing all element
and attribute names to their bare local form. Useful when you don't know the
prefixes up front — e.g. fingerprinting or clustering documents of unknown
vocabularies with `--paths`:

```bash
unxml --paths --hide-ns ALL unknown.xml   # prefix-free structural signature
```

Under `--auto`/`--bat`, unxml also **sniffs** the document type and hides a
sensible set automatically. Currently it recognises UBL *instance* documents
(an unprefixed root such as `<Invoice>` in a UBL namespace) and hides whichever
prefixes are bound to the Common Basic/Aggregate Components namespaces. A
stylesheet or schema that merely *references* UBL (e.g. an `xsl:stylesheet`
translating to UBL) is left untouched, since there the prefixes are real syntax.

### Canonicalising for diffs (`--canonical`)

Two documents can mean the same thing yet differ byte-for-byte over things that
carry no meaning: namespace *prefixes* are arbitrary local aliases for a URI,
and sibling order is often incidental. `--canonical` removes both so the
rendered output of equivalent documents diffs cleanly:

- **Prefixes are rebound** to stable names. Recognised vocabularies keep their
  conventional prefix (`xsl`, `xs`, `cac`, `ram`, …); everything else becomes
  `ns1`, `ns2`, … in sorted-URI order. A default namespace (`xmlns="…"`) is
  rewritten to the same explicit prefix, so `<a:Foo>` and `<Foo xmlns="…">` for
  one URI collapse to the identical name. All `xmlns:*` declarations are
  re-emitted, sorted, on the root.
- **Sibling elements are sorted** by a recursive signature, so order-only
  differences vanish. Mixed content (prose) keeps document order.

```bash
diff <(unxml --canonical a.xml) <(unxml --canonical b.xml)
```

Two documents differing only in prefix spelling, default-vs-explicit namespace,
and sibling order produce byte-identical output:

```xml
<a:Order xmlns:a="urn:shop:order" xmlns:c="urn:shop:cust">
  <a:Line sku="X1"><a:Qty>2</a:Qty></a:Line>
  <c:Customer id="42">Acme</c:Customer>
</a:Order>
```

```
ns2:Order(xmlns:ns1="urn:shop:cust", xmlns:ns2="urn:shop:order")
  ns1:Customer(id="42") = Acme
  ns2:Line(sku="X1")
    ns2:Qty = 2
```

Sibling sorting applies only to plain XML. Element order *is* significant in
stylesheets and schemas (`xsl:*` control flow, `xs:sequence`, Schematron rule
order), so in a dialect/`--special` mode (`--xslt`, `--xsd`, `--wsdl`,
`--schematron`) `--canonical` normalises prefixes only and preserves document
order.

### Listing document paths (`--paths`)

`--paths` dumps a compact structural summary instead of the full document: the
set of **distinct** element paths as an indented tree, each node shown once
(repeated siblings collapse) and annotated with the union of attribute names
ever seen at that path. A leading `//` legend explains the namespace prefixes
(recognised vocabularies on their conventional prefix are omitted as
self-explanatory):

```bash
unxml --paths invoice.xml
```

```
order(xmlns="urn:shop:order")
  customer(id)
  line(discount, sku)
    qty(unit)
```

Prefixed namespaces (`xmlns:ext`) go into a leading `//` legend; the default
namespace (`xmlns`) is shown inline on the element that sets it, since several
nested redefinitions would collide under one `(default)` legend key.

It answers "what shapes exist in this document" and is handy for understanding
or comparing document shapes. It composes with `--select` (subtree under a
match), `--hide-ns` (shorter segments), and `--canonical` (the legend resolves
the generated `ns1`/`ns2` names).

## Introduction

This command line application was developed for comparing XML files (e.g. database/application state dumps). It takes an XML file and converts it to a YAML-like syntax that is easier to read and compare.

### Example

Take an excerpt of the standard [UBL 2.1 invoice
example](https://docs.oasis-open.org/ubl/os-UBL-2.1/xml/UBL-Invoice-2.1-Example.xml):

```xml
<?xml version="1.0" encoding="UTF-8"?>
<Invoice xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2"
	xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
	xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
	<cbc:UBLVersionID>2.1</cbc:UBLVersionID>
	<cbc:ID>TOSL108</cbc:ID>
	<cbc:IssueDate>2009-12-15</cbc:IssueDate>
	<cbc:InvoiceTypeCode listID="UN/ECE 1001 Subset" listAgencyID="6">380</cbc:InvoiceTypeCode>
	<cbc:DocumentCurrencyCode listID="ISO 4217 Alpha" listAgencyID="6">EUR</cbc:DocumentCurrencyCode>
	<cac:AccountingSupplierParty>
		<cac:Party>
			<cac:PartyName>
				<cbc:Name>Salescompany ltd.</cbc:Name>
			</cac:PartyName>
			<cac:PostalAddress>
				<cbc:StreetName>Main street</cbc:StreetName>
				<cbc:CityName>Big city</cbc:CityName>
				<cbc:PostalZone>54321</cbc:PostalZone>
			</cac:PostalAddress>
		</cac:Party>
	</cac:AccountingSupplierParty>
</Invoice>
```

`unxml invoice.xml` flattens it into:

```
Invoice(
    xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2",
    xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2",
    xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2")
  cbc:UBLVersionID = 2.1
  cbc:ID = TOSL108
  cbc:IssueDate = 2009-12-15
  cbc:InvoiceTypeCode(listAgencyID="6", listID="UN/ECE 1001 Subset") = 380
  cbc:DocumentCurrencyCode(listAgencyID="6", listID="ISO 4217 Alpha") = EUR
  cac:AccountingSupplierParty
    cac:Party
      cac:PartyName
        cbc:Name = Salescompany ltd.
      cac:PostalAddress
        cbc:StreetName = Main street
        cbc:CityName = Big city
        cbc:PostalZone = 54321
```

With `--auto`, unxml sniffs the UBL instance and hides the noisy `cbc:`/`cac:`
prefixes (along with their `xmlns:` declarations), leaving just the signal:

```
Invoice(xmlns="urn:oasis:names:specification:ubl:schema:xsd:Invoice-2")
  UBLVersionID = 2.1
  ID = TOSL108
  IssueDate = 2009-12-15
  InvoiceTypeCode(listAgencyID="6", listID="UN/ECE 1001 Subset") = 380
  DocumentCurrencyCode(listAgencyID="6", listID="ISO 4217 Alpha") = EUR
  AccountingSupplierParty
    Party
      PartyName
        Name = Salescompany ltd.
      PostalAddress
        StreetName = Main street
        CityName = Big city
        PostalZone = 54321
```

### Mode example: XSLT

Beyond flattening, each mode rewrites its vocabulary into terser pseudocode.
A small XSLT stylesheet:

```xml
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
<xsl:template match="/">
  <table border="1">
    <xsl:for-each select="catalog/cd">
    <tr>
      <td><xsl:value-of select="title"/></td>
      <td><xsl:value-of select="artist"/></td>
    </tr>
    </xsl:for-each>
  </table>
</xsl:template>
</xsl:stylesheet>
```

renders with `unxml --xslt` as:

```
xsl:stylesheet(version="1.0", xmlns:xsl="http://www.w3.org/1999/XSL/Transform")
  match /:
    table(border="1")
      foreach catalog/cd:
        tr
          td
            <- title
          td
            <- artist
```

`match`, `foreach` and `<-` (for `xsl:value-of`) read like the control flow the
stylesheet actually expresses. See [XSLT transformations](docs/xslt.md) for the
full vocabulary, and [XSD](docs/xsd.md) / [Schematron](docs/schematron.md) for
the other modes.

### Key Features

- **Attributes in Parentheses**: Element attributes are displayed Pug-style as `element(attr="value")`
- **Text Content with Equals**: Element text content is shown as `ElementName = text content`
- **Hierarchical Indentation**: Nested elements are properly indented
- **Clean Format**: Easy to read and compare, great for diffing
- **Inline mixed content**: Prose interleaved with short inline elements stays on one readable line

### Mixed content (prose with inline spans)

Document-style XML interleaves text with small inline elements — a paragraph
containing a `<command>` or a `<link>`. Flattening every run onto its own line
makes such prose hard to read, so `unxml` keeps it inline as one line of
verbatim XML:

```xml
<para>The <command>widget</command> daemon keeps its
  <link href="recovery.html">recoverable</link> state in one database.</para>
```

renders as:

```
para = The <command>widget</command> daemon keeps its <link href="recovery.html">recoverable</link> state in one database.
```

An element flows inline when its whole subtree is *inline-safe* — text
interleaved with elements that are themselves inline-safe. A leaf with
significant (multi-line) text, such as `<programlisting>` or `<screen>`, is not
inline-safe, so its parent stays in the flattened block form and the listing
keeps its line breaks. Nested inline markup (e.g. `<emphasis>` wrapping a
`<command>`) collapses all the way up. This applies to the generic XML render;
the `--xslt`/`--xsd`/`--wsdl`/`--schematron` modes use their own formatting.

## Technical Details

- Built with Rust for performance and safety
- Uses `quick-xml` for fast XML parsing
- Uses `clap` for command-line argument parsing
- Proper error handling with `anyhow`

## License

MIT License - see LICENSE file for details.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

### Creating Releases

The version lives in the **git tag**, not in `Cargo.toml` (which stays at the
`0.0.0-dev` placeholder; the release workflow injects the real version with
`cargo set-version`). Do **not** bump `Cargo.toml` or create tags by hand.

To cut a release, let `gh` create the tag:

```bash
gh release create vX.Y.Z --title "Release vX.Y.Z" --notes "…"
```

The pushed tag triggers the GitHub Actions workflow, which builds binaries and
the PyPI wheel for all platforms and attaches them to the release.

The CI workflow runs on every push to ensure code quality with formatting checks, linting, and tests. 