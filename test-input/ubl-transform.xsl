<?xml version="1.0" encoding="UTF-8"?>
<!-- A stylesheet that PRODUCES UBL. Sniffing must NOT hide cbc:/cac: here:
     they are literal result elements and XPath, not instance noise. -->
<xsl:stylesheet version="1.0"
                xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                xmlns:cac="urn:oasis:names:specification:ubl:schema:xsd:CommonAggregateComponents-2"
                xmlns:cbc="urn:oasis:names:specification:ubl:schema:xsd:CommonBasicComponents-2">
  <xsl:template match="/Invoice">
    <cac:Party>
      <cbc:Name><xsl:value-of select="cbc:Name"/></cbc:Name>
    </cac:Party>
  </xsl:template>
</xsl:stylesheet>
