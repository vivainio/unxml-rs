---
name: unxml
description: Flatten and compare XML, HTML, and JSON files with the `unxml` CLI — read, diff, or fingerprint XML, HTML, JSON, XSLT, XSD, WSDL, Schematron, MSBuild, or Leo (.leo) documents as a terse, token-efficient YAML/Pug-like form. Invoke with /unxml.
disable-model-invocation: true
---

# unxml

Flattens XML/HTML/JSON into indented YAML/Pug-like text: far fewer tokens than
raw markup/JSON, easier to read. Use before catting a large or nested file.

## Output shape

- Attributes: `el(attr="value", flag)`
- Text: `ElementName = text content`
- Nesting: indentation
- HTML classes: `div.row.active`
- Inline prose stays on one line (`<para>` with inline `<command>`/`<link>`)

## Core usage

```bash
unxml file.xml                 # plain XML render by default
unxml '*.xml'                  # glob; multiple files get `// FILE:` headers
some-cmd | unxml --stdin       # stdin, assumes XML
cat page.html | unxml --stdin --format html
```

## JSON

`.json` auto-detected, no flag needed. Same `key = value`/indentation as XML.
Uniform scalar-object arrays → compact table (biggest token win):

```
users[]{id,name,team}
  1, Ada, platform
  2, Lin, data
```

`--auto`: JSON Schema docs / OpenAPI `components.schemas.*`/`schema` keys →
compact property view:

```
schema : object
  id! : integer int64
  tags : string[]
```

## Processing modes

Default to `--auto` — it picks the dialect from the extension (`.xsl`/`.xslt`,
`.xsd`, `.sch`, `.wsdl`, `.targets`/`.props`/`.csproj`/`.vbproj`/`.fsproj`/
`.sqlproj`, `.leo`) and rewrites that vocabulary into terser pseudocode
(XSLT's `match`/`foreach`/`<-`, MSBuild's `Condition=` → `if C:`, Leo's
headline+body join, ...):

```bash
unxml --auto file.xsd
```

No extension to sniff (e.g. stdin) or an unusual filename: pass the flag
directly — `--xslt`, `--xsd`, `--schematron`, `--wsdl`, `--msbuild`, `--leo`.
`--special` (proprietary business-element rules) has no extension trigger and
always needs the explicit flag.

## Reading aids

- `--bat` — page through `bat -l unxml`, syntax-highlighted (implies `--auto`;
  falls back to plain stdout if `bat` missing)
- `--hide-ns cbc,cac` — drop prefixes + their `xmlns:` decls from names.
  Repeatable/comma-separated. `--hide-ns ALL` = every prefix. `--auto` also
  auto-hides for known vocabularies (e.g. UBL).
- `--select InvoiceLine` — only matching subtrees (bare = local name,
  `cac:InvoiceLine` = full name). Mini XPath: `/a/b`, `a//b`, `*`, `..`, `.`,
  `[@attr]`, `[@attr="v"]` — e.g. `'order[@id="2"]/line'`,
  `'qty[@unit="kg"]/..'`. Relative = anywhere (`item` = `//item`); no other
  axes, positions or functions.
  Over many files it's a search: non-matching files print nothing (no
  `// FILE:` header); XML files are text-prefiltered and run in parallel.
- `--zip 'dumps/*.zip'` — also read every XML/HTML entry in archives, shown as
  `archive.zip!/inner.xml`. Pass that name back as a file arg (entry part may
  be a glob: `'a.zip!/orders/*.xml'`) to dump whole entries; `--cat --raw`
  shows the original XML.
- `--expand` — inline matching imported templates for `xsl:apply-templates`

## `--canonical` (diffing)

Rebinds namespace prefixes to stable names, sorts siblings — equivalent docs
diff identically regardless of prefix spelling/order:

```bash
diff <(unxml --canonical a.xml) <(unxml --canonical b.xml)
```

Dialect modes (`--xslt`/`--xsd`/`--wsdl`/`--schematron`/`--msbuild`): prefixes
only, order preserved (element order is significant there).

## Git integration

`unxml git <args>` = `git <args>` with the textconv driver for that one call
only — nothing written to `.git/`:

```bash
unxml git diff
unxml git log -p -- invoice.xml
unxml git show HEAD~1:invoice.xml
```

## `--paths` (structural fingerprint)

Distinct element paths as a tree (dedup siblings, union of attrs per path)
instead of the full document:

```bash
unxml --paths invoice.xml
unxml --paths --depth 2 doc.xml            # cap nesting (root = level 1)
unxml --paths --no-attrs doc.xml           # namespaces only, drop attrs
```

Format census across a directory:

```bash
for f in *.xml; do unxml --paths --depth 1 --no-attrs --hide-ns ALL "$f"; done \
  | sort | uniq -c | sort -rn
```

Composes with `--select`, `--hide-ns`, `--canonical`.

## Tips

- Default = plain XML; add `--auto` (or explicit mode) for stylesheets/schemas.
  `.json` needs no flag.
- Unknown vocabulary: `--paths --hide-ns ALL` for a prefix-free signature.
- Prefer `unxml` over raw XML/JSON for understanding structure or comparing
  files — dramatically fewer tokens.
