<?xml version="1.0" encoding="UTF-8"?>
<!-- XSLT 2.0 feature coverage: functions, grouping, regex, sequences, etc. -->
<xsl:stylesheet version="2.0"
                xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                xmlns:xs="http://www.w3.org/2001/XMLSchema"
                xmlns:my="urn:my-functions">

  <xsl:output method="xml" indent="yes"/>

  <!-- xsl:character-map / xsl:output-character -->
  <xsl:character-map name="entities">
    <xsl:output-character character="&#160;" string="&amp;nbsp;"/>
  </xsl:character-map>

  <!-- User-defined function with typed params and result -->
  <xsl:function name="my:full-name" as="xs:string">
    <xsl:param name="first" as="xs:string"/>
    <xsl:param name="last" as="xs:string"/>
    <xsl:sequence select="concat($first, ' ', $last)"/>
  </xsl:function>

  <!-- Typed variable -->
  <xsl:variable name="threshold" as="xs:integer" select="10"/>

  <xsl:template match="/orders">
    <report>
      <!-- xsl:for-each-group with grouping keys -->
      <xsl:for-each-group select="order" group-by="@region">
        <region name="{current-grouping-key()}">
          <count><xsl:value-of select="count(current-group())"/></count>
          <xsl:perform-sort select="current-group()">
            <xsl:sort select="@total" data-type="number" order="descending"/>
          </xsl:perform-sort>
        </region>
      </xsl:for-each-group>

      <!-- group-adjacent variant -->
      <xsl:for-each-group select="line" group-adjacent="@status">
        <run status="{current-grouping-key()}" size="{count(current-group())}"/>
      </xsl:for-each-group>

      <!-- xsl:analyze-string with regex branches -->
      <xsl:analyze-string select="@code" regex="([A-Z]+)-([0-9]+)">
        <xsl:matching-substring>
          <prefix><xsl:value-of select="regex-group(1)"/></prefix>
          <number><xsl:value-of select="regex-group(2)"/></number>
        </xsl:matching-substring>
        <xsl:non-matching-substring>
          <raw><xsl:value-of select="."/></raw>
        </xsl:non-matching-substring>
      </xsl:analyze-string>

      <!-- value-of with separator (2.0) -->
      <names>
        <xsl:value-of select="order/@customer" separator=", "/>
      </names>

      <!-- xsl:namespace instruction -->
      <xsl:element name="wrapped">
        <xsl:namespace name="ext" select="'urn:ext'"/>
      </xsl:element>

      <!-- xsl:result-document to a secondary output -->
      <xsl:result-document href="summary.xml" method="xml">
        <summary total="{sum(order/@total)}"/>
      </xsl:result-document>
    </report>
  </xsl:template>

  <!-- next-match delegates to a less-specific template -->
  <xsl:template match="order[@priority='high']">
    <urgent><xsl:next-match/></urgent>
  </xsl:template>

  <xsl:template match="order">
    <entry name="{my:full-name(@first, @last)}"/>
  </xsl:template>

</xsl:stylesheet>
