# demo

Sample `unxml --xsd` outputs for real-world XML Schemas. The source `.xsd`
files come from publicly available e-invoicing standards and are not
vendored here — only the rendered output is checked in, so you can browse
what the tool produces on serious schemas without chasing the originals.

| Output | Source | Lines | Bytes |
| --- | --- | --- | --- |
| `peppol-common-basic-components.xsd.pug` | `PEPPOL_CommonBasicComponents.xsd` from early [OpenPeppol BIS](https://peppol.org/) profiles | 41 → 24 (−41%) | 2.6 KB → 1.5 KB (−44%) |
| `e2b-invoice-interchange-v3p4.xsd.pug` | `e2b_Invoice_Interchange_v3p4.xsd` — legacy Norwegian e2b invoice XML format (predecessor to EHF) | 46 → 21 (−54%) | 3.2 KB → 725 B (−78%) |
| `finvoice-3.0.xsd.pug` | `Finvoice3.0.xsd` — [Finvoice 3.0](https://www.finanssiala.fi/en/topics/finvoice-standard/), Finance Finland's e-invoicing standard | 1,702 → 1,134 (−33%) | 98 KB → 47 KB (−52%) |
| `ubl-common-basic-components-2.1.xsd.pug` | `UBL-CommonBasicComponents-2.1.xsd` from [OASIS UBL 2.1](https://docs.oasis-open.org/ubl/UBL-2.1.html) (ISO/IEC 19845:2015) | 5,388 → 1,752 (−67%) | 220 KB → 92 KB (−58%) |
| `ubl-common-aggregate-components-2.1.xsd.pug` | `UBL-CommonAggregateComponents-2.1.xsd` from [OASIS UBL 2.1](https://docs.oasis-open.org/ubl/UBL-2.1.html) | 39,798 → 3,038 (−92%) | 2.4 MB → 105 KB (−96%) |

## Regenerating

```sh
unxml --xsd PATH/TO/source.xsd > demo/<name>.xsd.pug
```
