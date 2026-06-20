# Schematron transformations

When `unxml` runs in Schematron mode it rewrites the ISO Schematron vocabulary
into a terse, rules-like pseudocode, so a schema reads like the assertions it
actually expresses. This page lists every transformation with side-by-side
samples.

## Enabling Schematron mode

Schematron mode is selected automatically for `.sch` files, or forced with the
`--schematron` flag:

```bash
# Auto-detected from the extension
unxml rules.sch

# Forced (e.g. when reading from stdin)
cat rules.sch | unxml --stdin --schematron
```

Both the default-namespace form (`<schema xmlns="http://purl.oclc.org/dsdl/schematron">`)
and the prefixed form (`<sch:schema>`) are recognized — an optional `sch:`
prefix is stripped before matching.

Because Schematron rules embed XSLT (in `xsl:value-of`, `xsl:let`, etc.), this
mode *also* applies the [XSLT transformations](xslt.md) to any `xsl:*` elements
it encounters.

## Quick reference

| Schematron construct | unxml output |
| --- | --- |
| `schema` | `schema [title]` |
| `title` | `title = text` |
| `ns prefix="x" uri="…"` | `ns x = …` |
| `phase id="X"` | `phase X` |
| `active pattern="P"` | `active P` |
| `pattern id="X"` | `pattern X` |
| `rule context="C"` | `rule C` |
| `assert test="T"` | `assert [id] [[flag]] T` |
| `report test="T"` | `report [id] [[flag]] T` |
| *(assert/report message)* | `= message` |
| `let name="x" value="…"` | `x := …` |

Each construct is detailed below.

## Schema and metadata

### `schema` → `schema [title]`

The wrapper is dropped and its children process at the same level. A `title`
attribute, if present, follows `schema`:

```xml
<schema title="Sample rules">
  ...
</schema>
```
```text
schema Sample rules
  ...
```

### `title` → `title = text`

A `title` *element* (the usual form) renders as an assignment:

```xml
<title>Sample rules</title>
```
```text
title = Sample rules
```

### `ns` → `ns prefix = uri`

Namespace bindings are documented so the prefixes used in contexts and tests
(`cbc:EndpointID`, `cac:Party`) are traceable:

```xml
<ns prefix="cbc" uri="urn:…:CommonBasicComponents-2"/>
```
```text
ns cbc = urn:…:CommonBasicComponents-2
```

## Phases

### `phase id="X"` → `phase X`

```xml
<phase id="check">
  ...
</phase>
```
```text
phase check
  ...
```

### `active pattern="P"` → `active P`

```xml
<active pattern="core"/>
```
```text
active core
```

## Patterns and rules

### `pattern id="X"` → `pattern X`

```xml
<pattern id="core">
  ...
</pattern>
```
```text
pattern core
  ...
```

### `rule context="C"` → `rule C`

The context XPath has its whitespace collapsed:

```xml
<rule context="cac:AccountingCustomerParty/cac:Party">
  ...
</rule>
```
```text
rule cac:AccountingCustomerParty/cac:Party
  ...
```

## Assertions

### `assert` / `report`

Both render the same way: the keyword, an optional `id`, an optional `flag` in
brackets, then the (whitespace-collapsed) `test`. The element's message text
follows on an indented `=` line.

```xml
<assert id="SAMPLE-R001" flag="fatal" test="cbc:EndpointID">Buyer electronic address MUST be provided.</assert>
```
```text
assert SAMPLE-R001 [fatal] cbc:EndpointID
  = Buyer electronic address MUST be provided.
```

`report` is identical in shape — it just flags a *positive* match instead of a
required one:

```xml
<report id="SAMPLE-R010" test="not(@currencyID)">Amount has no currencyID attribute.</report>
```
```text
report SAMPLE-R010 not(@currencyID)
  = Amount has no currencyID attribute.
```

The `id` and `flag` are independent — any combination renders sensibly
(`assert [warning] T`, `assert R002 T`, or just `assert T`).

## Variables

### `let name="x" value="…"` → `x := …`

A `let` binding collapses to an assignment, matching the `:=` form used for XSLT
variables. The value may come from the `value` attribute or the element body:

```xml
<let name="documentCurrencyCode" value="/*/cbc:DocumentCurrencyCode"/>
```
```text
documentCurrencyCode := /*/cbc:DocumentCurrencyCode
```

## A worked example

For a schema exercising the full vocabulary end-to-end, see
[`test-input/schematron-constructs.sch`](../test-input/schematron-constructs.sch)
and run `unxml --schematron test-input/schematron-constructs.sch`.
