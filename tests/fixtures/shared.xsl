<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">

  <!-- Template with union pattern -->
  <xsl:template match="Header|Footer">
    <Section>
      <Title><xsl:value-of select="Name"/></Title>
      <Date><xsl:value-of select="Timestamp"/></Date>
    </Section>
  </xsl:template>

  <!-- Simple template -->
  <xsl:template match="Item">
    <Entry>
      <xsl:value-of select="."/>
    </Entry>
  </xsl:template>

</xsl:stylesheet>
