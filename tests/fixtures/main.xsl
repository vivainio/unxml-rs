<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">
  <xsl:import href="shared.xsl"/>

  <xsl:template match="/">
    <Output>
      <xsl:apply-templates select="Input/Header"/>
      <xsl:apply-templates select="Input/Body"/>
    </Output>
  </xsl:template>

  <xsl:template match="Body">
    <Content>
      <xsl:value-of select="Text"/>
    </Content>
  </xsl:template>
</xsl:stylesheet>
