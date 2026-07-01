---
name: unxml
description: Flatten and compare XML/HTML files with the `unxml` CLI — read, diff, or fingerprint XML, HTML, XSLT, XSD, WSDL, or Schematron documents as a terse, token-efficient YAML/Pug-like form. Invoke with /unxml.
disable-model-invocation: true
---

# unxml

`unxml` rewrites XML/HTML into a light, indented YAML/Pug-like form that is
much easier to read and diff than raw markup. Reach for it before catting a
large or deeply-nested XML file — the flattened output is a fraction of the
tokens and far more legible.

## Output shape

- Attributes go in parentheses, Pug-style and sorted: `el(attr="value", flag)`
- Text content uses `=`: `ElementName = text content`
- Nesting is shown by indentation
- HTML classes attach to the name: `div.row.active`
- Inline prose (a `<para>` with inline `<command>`/`<link>`) stays on one line

## Core usage

```bash
unxml file.xml                 # flatten one file (plain XML render by default)
unxml '*.xml'                  # glob; multiple files get `// FILE:` headers
some-cmd | unxml --stdin       # read from stdin (assumes XML)
cat page.html | unxml --stdin --format html
```

## Processing modes

Each mode rewrites a vocabulary into terser pseudocode. Pass the flag explicitly,
or use `--auto` to pick by extension (`.xsl`/`.xslt`→xslt, `.sch`→schematron,
`.xsd`→xsd). An explicit flag always wins over `--auto`.

| Flag           | For                                              |
| -------------- | ------------------------------------------------ |
| `--xslt`       | XSLT stylesheets (`match`, `foreach`, `<-` …)    |
| `--xsd`        | XML Schema                                       |
| `--schematron` | Schematron rule schemas                          |
| `--wsdl`       | WSDL 1.1 / SOAP (embedded schema via XSD rules)  |
| `--special`    | Proprietary business-element rules               |
| `--auto`       | Pick the mode from each file's extension         |

```bash
unxml --xslt transform.xslt
unxml --auto schema.xsd        # detects --xsd
```

## Reading aids

- `--bat` — pipe through `bat -l unxml` for paged, syntax-highlighted output
  (implies `--auto`; falls back to plain stdout if `bat` is missing).
- `--hide-ns cbc,cac` — drop noisy namespace prefixes (and their `xmlns:` decls)
  from element/attribute names. Repeatable/comma-separated. `--hide-ns ALL`
  strips every prefix to bare local names. Under `--auto`, well-known docs
  (e.g. UBL instances) get a sensible set hidden automatically.
- `--select InvoiceLine` — render only subtrees matching that tag (bare name
  matches local name, ignoring prefix; `cac:InvoiceLine` matches the full name).
- `--expand` — inline matching imported templates for `xsl:apply-templates`.

## Diffing two documents (`--canonical`)

`--canonical` rebinds namespace prefixes to stable names and sorts sibling
elements, so two equivalent documents that differ only in prefix spelling,
default-vs-explicit namespace, or sibling order diff cleanly:

```bash
diff <(unxml --canonical a.xml) <(unxml --canonical b.xml)
```

In a dialect mode (`--xslt`/`--xsd`/`--wsdl`/`--schematron`) element order is
significant, so `--canonical` normalises prefixes only and preserves order.

## Git integration

`unxml git <args>` transparently runs `git <args>` (diff, log -p, show, ...)
with a `textconv` driver applied for just that one invocation, so XML/HTML
render in the canonicalised flattened form and prefix- or order-only churn
drops out of the diff. Nothing is written to `.git/config` or
`.git/info/attributes` — it's a drop-in replacement for plain `git` on a
one-off basis:

```bash
unxml git diff
unxml git log -p -- invoice.xml
unxml git show HEAD~1:invoice.xml
```

`unxml --init-git` instead wires the same driver permanently into the current
repo's `.git/config`/`info/attributes` (idempotent, working tree untouched),
for when every `git diff`/`log -p`/`show` in that clone should render this way
without prefixing commands with `unxml git`.

## Structural fingerprint (`--paths`)

`--paths` dumps the set of *distinct* element paths as an indented tree (each
node once, annotated with the union of attribute names seen there) instead of
the full document — answers "what shapes exist here".

```bash
unxml --paths invoice.xml
unxml --paths --depth 2 doc.xml            # cap nesting depth (root = level 1)
unxml --paths --no-attrs doc.xml           # keep only namespaces, drop attrs
```

Format census across a directory — cluster files by structure:

```bash
for f in *.xml; do unxml --paths --depth 1 --no-attrs --hide-ns ALL "$f"; done \
  | sort | uniq -c | sort -rn
```

`--paths` composes with `--select`, `--hide-ns`, and `--canonical`.

## Tips

- Default render is plain XML — add `--auto` (or an explicit mode) for
  stylesheets/schemas.
- For unknown vocabularies, `--paths --hide-ns ALL` gives a prefix-free
  structural signature.
- Prefer `unxml` over reading raw XML when the goal is to understand structure
  or compare files; it is dramatically more token-efficient.
