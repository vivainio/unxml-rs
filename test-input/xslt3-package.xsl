<?xml version="1.0" encoding="UTF-8"?>
<!-- XSLT 3.0 packaging: xsl:package as root, expose, use-package, override. -->
<xsl:package name="urn:my-utils" package-version="1.0" version="3.0"
             xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
             xmlns:xs="http://www.w3.org/2001/XMLSchema">

  <!-- Consume another package, overriding one component -->
  <xsl:use-package name="urn:base-lib" package-version="2.0">
    <xsl:override>
      <xsl:template match="widget">
        <widget-v2><xsl:apply-templates/></widget-v2>
      </xsl:template>
    </xsl:override>
  </xsl:use-package>

  <!-- Control component visibility -->
  <xsl:expose component="function" names="my:*" visibility="public"/>
  <xsl:expose component="template" names="internal" visibility="private"/>

  <!-- A public function this package offers -->
  <xsl:function name="my:square" as="xs:integer" visibility="public">
    <xsl:param name="n" as="xs:integer"/>
    <xsl:sequence select="$n * $n"/>
  </xsl:function>

  <xsl:template name="internal" visibility="private">
    <xsl:sequence select="'hidden'"/>
  </xsl:template>

</xsl:package>
