<?xml version="1.0" encoding="UTF-8"?>
<schema xmlns="http://purl.oclc.org/dsdl/schematron" queryBinding="xslt2">
  <title>Sample rules</title>
  <ns prefix="cbc" uri="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2"/>
  <ns prefix="cac" uri="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"/>

  <phase id="check">
    <active pattern="core"/>
  </phase>

  <let name="documentCurrencyCode" value="/*/cbc:DocumentCurrencyCode"/>

  <pattern id="core">
    <rule context="cac:AccountingCustomerParty/cac:Party">
      <assert id="SAMPLE-R001" flag="fatal" test="cbc:EndpointID">Buyer electronic address MUST be provided.</assert>
      <assert id="SAMPLE-R002" flag="warning" test="count(cac:Contact) &lt;= 1">No more than one contact may be provided.</assert>
    </rule>
    <rule context="cbc:Amount">
      <report id="SAMPLE-R010" test="not(@currencyID)">Amount has no currencyID attribute.</report>
    </rule>
  </pattern>
</schema>
