<?xml version="1.0" encoding="UTF-8"?>
<!-- XSLT 3.0 feature coverage: maps, iterate, try/catch, merge, evaluate, etc. -->
<xsl:stylesheet version="3.0"
                xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                xmlns:xs="http://www.w3.org/2001/XMLSchema"
                expand-text="yes">

  <!-- Mode declaration with streaming + no-match behaviour -->
  <xsl:mode name="main" streamable="yes" on-no-match="shallow-copy"/>

  <!-- Accumulator (3.0) -->
  <xsl:accumulator name="total" as="xs:decimal" initial-value="0">
    <xsl:accumulator-rule match="item" select="$value + xs:decimal(@amount)"/>
  </xsl:accumulator>

  <!-- Global context item declaration -->
  <xsl:global-context-item as="document-node()"/>

  <xsl:template match="/data" mode="main">
    <result>
      <!-- Text value template (expand-text) -->
      <greeting>Total so far: {accumulator-after('total')}</greeting>

      <!-- xsl:map / xsl:map-entry -->
      <xsl:variable name="lookup" as="map(xs:string, xs:string)">
        <xsl:map>
          <xsl:map-entry key="'a'" select="'alpha'"/>
          <xsl:map-entry key="'b'" select="'beta'"/>
        </xsl:map>
      </xsl:variable>

      <!-- xsl:iterate with running parameter, break and on-completion -->
      <xsl:iterate select="item">
        <xsl:param name="running" select="0"/>
        <xsl:on-completion>
          <grand-total><xsl:value-of select="$running"/></grand-total>
        </xsl:on-completion>
        <xsl:if test="$running gt 1000">
          <xsl:break>
            <stopped-early/>
          </xsl:break>
        </xsl:if>
        <xsl:next-iteration>
          <xsl:with-param name="running" select="$running + @amount"/>
        </xsl:next-iteration>
      </xsl:iterate>

      <!-- xsl:try / xsl:catch -->
      <xsl:try>
        <parsed><xsl:value-of select="xs:integer(@count)"/></parsed>
        <xsl:catch errors="*:FORG0001">
          <error code="{$err:code}">bad number</error>
        </xsl:catch>
      </xsl:try>

      <!-- xsl:evaluate dynamic XPath -->
      <xsl:evaluate xpath="@expr" context-item="."/>

      <!-- xsl:merge across sources -->
      <xsl:merge>
        <xsl:merge-source name="a" select="setA/row" for-each-source="()">
          <xsl:merge-key select="@id"/>
        </xsl:merge-source>
        <xsl:merge-source name="b" select="setB/row">
          <xsl:merge-key select="@id"/>
        </xsl:merge-source>
        <xsl:merge-action>
          <merged id="{@id}"><xsl:sequence select="current-merge-group()"/></merged>
        </xsl:merge-action>
      </xsl:merge>

      <!-- xsl:where-populated / xsl:on-empty / xsl:on-non-empty -->
      <xsl:where-populated>
        <notes>
          <xsl:on-non-empty><heading>Notes</heading></xsl:on-non-empty>
          <xsl:apply-templates select="note"/>
          <xsl:on-empty><none/></xsl:on-empty>
        </notes>
      </xsl:where-populated>

      <!-- xsl:source-document (3.0 streaming-friendly input) -->
      <xsl:source-document href="extra.xml" streamable="yes">
        <xsl:apply-templates select="*"/>
      </xsl:source-document>

      <!-- xsl:fork -->
      <xsl:fork>
        <xsl:sequence select="'one'"/>
        <xsl:sequence select="'two'"/>
      </xsl:fork>

      <!-- xsl:assert -->
      <xsl:assert test="count(item) gt 0" error-code="my:empty">
        no items present
      </xsl:assert>
    </result>
  </xsl:template>

  <!-- Context-item declaration on a named template -->
  <xsl:template name="init">
    <xsl:context-item as="element()" use="required"/>
    <xsl:sequence select="."/>
  </xsl:template>

</xsl:stylesheet>
