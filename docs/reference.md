# Reference: how unxml simplifies your XML

This page is a tour of every way `unxml` shortens a document, so you can spot
which parts of *your* XML it will compress and which flags to reach for. Each
row shows a snippet of input and the line `unxml` prints for it.

For the format-specific vocabularies (XSLT, XSD, Schematron) there are separate
deep-dive pages — this page summarises them and links out.

- [Base syntax](#base-syntax) — always on, for any XML or HTML
- [Cutting noise with flags](#cutting-noise-with-flags) — opt-in simplifications
- [Format-specific modes](#format-specific-modes) — XSLT / XSD / Schematron / WSDL
- [Which flag do I want?](#which-flag-do-i-want)

## Base syntax

These transformations happen to every document with no flags. The output is a
Pug-like skeleton: indentation replaces closing tags, and the bracket noise is
gone.

| Your XML | unxml prints | What got shortened |
| --- | --- | --- |
| `<order><line>A</line></order>` | `order`<br>`  line = A` | Closing tags and angle brackets dropped; nesting becomes indentation. |
| `<user id="1">Jane</user>` | `user(id="1") = Jane` | Attributes move into `(…)`; text content becomes `= value`. |
| `<input disabled=""/>` | `input(disabled)` | Empty-valued attributes render as bare flags (booleans). |
| `<br/>` | `br` | Empty elements are just their name. |

If a line of attributes gets long (over 100 columns) it wraps to one attribute
per line automatically, so a namespace-heavy root stays readable.

### Text that spans multiple lines

A single-line value stays inline (`name = value`); a multi-line value becomes a
piped block, so it is clear where the value starts and ends:

```
code =
  | line1
  |   line2
  | line3
```

### Prose with inline tags

Mixed content (text interleaved with small inline elements) is kept as one line
of the original markup instead of being exploded into a stack:

```
para = The <command>build</command> step runs <option>-O2</option>.
```

### HTML classes

In HTML mode, CSS classes attach to the element with dots, Pug-style — and a
plain `<div class="…">` is shown by its classes alone:

```
.card.big
  span = hi
```

## Cutting noise with flags

These are opt-in. Reach for them when the base output is still too noisy for
what you're doing.

### Namespace prefixes you don't care about — `--hide-ns`

Vocabularies like UBL bury the signal under repeated `cbc:` / `cac:` prefixes.
Drop them (and their `xmlns:` declarations) so names read as bare locals:

```bash
unxml --hide-ns cbc,cac invoice.xml
unxml --hide-ns ALL unknown.xml      # hide every prefix
```

**Tip:** with `--auto`, unxml sniffs well-known document types (UBL, CII /
Factur-X / ZUGFeRD) and hides the right prefixes for you — no list needed.

### Boilerplate wrapper chains — `--collapse`

Single-child wrapper elements that carry no attributes or text (UBL's
`ext:UBLExtensions` is the classic offender) fold onto one `parent/child` line:

```
ext:UBLExtensions/ext:UBLExtension/ext:ExtensionContent/sig:UBLDocumentSignatures
  cbc:ID = ...
```

```bash
unxml --collapse invoice.xml                 # fold every such wrapper
unxml --collapse=ext:UBLExtensions inv.xml   # only chains starting here
```

**Tip:** under `--auto`, a sniffed UBL or CII instance folds all its wrapper
chains automatically (15–25% fewer lines) — you only need `--collapse`
explicitly for other vocabularies. See the
[README section](../README.md#collapsing-wrapper-chains---collapse) for the
full rules.

### Only the parts you want — `--select`

Render just the subtrees whose tag matches, as top-level fragments — handy for a
huge document where you only care about, say, the invoice lines:

```bash
unxml --select InvoiceLine invoice.xml   # bare name ignores prefixes
```

### Diffing two documents — `--canonical`

Rebinds prefixes to stable names and sorts siblings, so prefix- and order-only
differences disappear and a real diff stands out:

```bash
diff <(unxml --canonical a.xml) <(unxml --canonical b.xml)
```

### Just the shape, not the data — `--paths`

Collapses repeated siblings and drops values, leaving one line per distinct
element path (annotated with the attribute names seen there):

```
doc
  item(k)
```

Add `--fold` to hoist repeated subtree shapes into named `@Shape` definitions,
`--depth N` to cap nesting, and `--no-attrs` to drop attribute names — together
these turn a directory of files into a structural fingerprint for clustering.

## Format-specific modes

When a document is a known dialect, unxml rewrites its vocabulary into terse
pseudocode. These modes auto-enable by extension under `--auto`, or with an
explicit flag. Each has its own full reference:

| Mode | Flag / extension | A taste | Full reference |
| --- | --- | --- | --- |
| XSLT | `--xslt` / `.xsl` | `xsl:value-of select="X"` → `<- X` | [docs/xslt.md](xslt.md) |
| XSD | `--xsd` / `.xsd` | `xs:element name="N" type="T"` → `element N : T` | [docs/xsd.md](xsd.md) |
| Schematron | `--schematron` / `.sch` | `rule context="C"` → `rule C` | [docs/schematron.md](schematron.md) |
| WSDL | `--wsdl` / `.wsdl` | SOAP service description; embedded schema uses XSD rules | (see README) |
| MSBuild | `--msbuild` / `.targets`, `.props`, `.csproj`, ... | `Target Condition="C" ...` → `if C:` / `  Target(...)` | [docs/msbuild.md](msbuild.md) |

There is also `--special`, a set of proprietary business-workflow rules
(`include="foo"` → `if foo`, `<section name="X">` → `#X`, method references →
`File::Section(...)`, and so on). It is off by default and only useful for that
specific document family.

## Which flag do I want?

- **"The prefixes are drowning everything."** → `--hide-ns` (or `--auto` to let
  unxml pick).
- **"There's a deep stack of pointless wrapper tags."** → `--collapse`.
- **"I only care about one part of a giant file."** → `--select`.
- **"I want to diff two documents."** → `--canonical`.
- **"I just want to see the structure, not the data."** → `--paths` (add
  `--fold` if shapes repeat).
- **"It's a stylesheet / schema / Schematron."** → `--auto`, or the matching
  mode flag, then see its reference page above.
