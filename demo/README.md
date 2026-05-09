# demo

Sample `unxml --xsd` outputs for real-world XML Schemas. The source `.xsd`
files are not vendored here — only the rendered output is checked in, so
you can browse what the tool produces on serious schemas without chasing
the originals. Every source is downloaded from a public canonical URL
listed below; to regenerate, fetch the source and run `unxml --xsd`.

## Standalone

| Output | Source | Lines | Bytes |
| --- | --- | --- | --- |
| `finvoice-3.0.xsd.pug` | [`Finvoice3.0.xsd`](https://file.finanssiala.fi/finvoice/Finvoice3.0.xsd) — [Finvoice 3.0](https://www.finanssiala.fi/en/topics/finvoice-standard/), Finance Finland's e-invoicing standard | 1,690 → 1,133 (−33%) | 97 KB → 47 KB (−52%) |

## ubl/ — full UBL 2.1 type chain

[OASIS UBL 2.1](https://docs.oasis-open.org/ubl/UBL-2.1.html) (also
ISO/IEC 19845:2015) is layered: each layer wraps and adds semantics on
top of the one below. To resolve `ref cbc:ID` in the aggregates, you
follow `cac → cbc → udt → cct` to bottom out at `xsd:string`.

All sources downloaded from `https://docs.oasis-open.org/ubl/os-UBL-2.1/xsd/`.

| Output | What it is | Lines | Bytes |
| --- | --- | --- | --- |
| `ubl/cct.xsd.pug` | [`CCTS_CCT_SchemaModule-2.1.xsd`](https://docs.oasis-open.org/ubl/os-UBL-2.1/xsd/common/CCTS_CCT_SchemaModule-2.1.xsd) — UN/CEFACT Core Component Types (the bottom: `IdentifierType`, `AmountType`, etc., wrapping `xsd:string`/`xsd:decimal` with attributes) | 731 → 48 (−93%) | 45 KB → 1.9 KB (−96%) |
| `ubl/udt.xsd.pug` | [`UBL-UnqualifiedDataTypes-2.1.xsd`](https://docs.oasis-open.org/ubl/os-UBL-2.1/xsd/common/UBL-UnqualifiedDataTypes-2.1.xsd) — UBL's restrictions of CCTS types (`udt:IdentifierType extends ccts-cct:IdentifierType`) | 553 → 31 (−94%) | 27 KB → 1.6 KB (−94%) |
| `ubl/qdt.xsd.pug` | [`UBL-QualifiedDataTypes-2.1.xsd`](https://docs.oasis-open.org/ubl/os-UBL-2.1/xsd/common/UBL-QualifiedDataTypes-2.1.xsd) — UBL qualified types (currency-specific amounts, etc.) | 69 → 5 (−93%) | 3.6 KB → 424 B (−88%) |
| `ubl/cbc.xsd.pug` | [`UBL-CommonBasicComponents-2.1.xsd`](https://docs.oasis-open.org/ubl/os-UBL-2.1/xsd/common/UBL-CommonBasicComponents-2.1.xsd) — semantic basic components: every named field (`ID`, `IssueDate`, `LineExtensionAmount`) lives here as a global element | 5,388 → 1,752 (−67%) | 220 KB → 92 KB (−58%) |
| `ubl/cac.xsd.pug` | [`UBL-CommonAggregateComponents-2.1.xsd`](https://docs.oasis-open.org/ubl/os-UBL-2.1/xsd/common/UBL-CommonAggregateComponents-2.1.xsd) — semantic aggregates (`AddressType`, `PartyType`, `LineItemType`) composed from cbc fields via `ref cbc:Foo` | 39,798 → 3,038 (−92%) | 2.4 MB → 105 KB (−96%) |
| `ubl/cec.xsd.pug` | [`UBL-CommonExtensionComponents-2.1.xsd`](https://docs.oasis-open.org/ubl/os-UBL-2.1/xsd/common/UBL-CommonExtensionComponents-2.1.xsd) — the `UBLExtensions` slot for extending any UBL document | 222 → 39 (−82%) | 9.5 KB → 1.9 KB (−80%) |
| `ubl/invoice.xsd.pug` | [`UBL-Invoice-2.1.xsd`](https://docs.oasis-open.org/ubl/os-UBL-2.1/xsd/maindoc/UBL-Invoice-2.1.xsd) — the root document schema, composed from cac/cbc | 1,001 → 65 (−94%) | 60 KB → 2.6 KB (−96%) |

### Tracing a reference

To find the underlying primitive of an `Invoice/cbc:ID`:

```
invoice.xsd.pug:  ref cbc:ID                       (the slot)
cbc.xsd.pug:      element ID : IDType              (the global element)
cbc.xsd.pug:      type IDType extends udt:IdentifierType   (cbc wrapper)
udt.xsd.pug:      type IdentifierType extends ccts-cct:IdentifierType   (udt wrapper)
cct.xsd.pug:      type IdentifierType extends xsd:string   (the actual primitive)
```

## Regenerating

```sh
unxml --xsd PATH/TO/source.xsd > demo/<name>.xsd.pug
```
