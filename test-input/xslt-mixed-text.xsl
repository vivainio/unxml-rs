<?xml version="1.0" encoding="UTF-8"?>
<!-- Loose literal text directly inside xsl:* containers (mixed content).
     Each piece of text should survive as a quoted line in the output. -->
<xsl:stylesheet version="1.0" xmlns:xsl="http://www.w3.org/1999/XSL/Transform">

  <xsl:template match="title">
    Title: <span><xsl:value-of select="."/></span>
  </xsl:template>

  <xsl:template match="cd">
    <xsl:for-each select="track">
      Track: <xsl:value-of select="."/>
    </xsl:for-each>
    <xsl:if test="@featured">
      Featured! <xsl:value-of select="@featured"/>
    </xsl:if>
    <xsl:choose>
      <xsl:when test="price &gt; 10">
        Expensive: <xsl:value-of select="price"/>
      </xsl:when>
      <xsl:otherwise>
        Cheap: <xsl:value-of select="price"/>
      </xsl:otherwise>
    </xsl:choose>
    <xsl:element name="note">
      Inline note <xsl:value-of select="@id"/>
    </xsl:element>
    <price>
      <xsl:value-of select="price">0.00</xsl:value-of>
    </price>
  </xsl:template>

</xsl:stylesheet>
