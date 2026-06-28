<schema xmlns="http://purl.oclc.org/dsdl/schematron">
  <!-- currency rules -->
  <pattern>
    <!-- the main rule -->
    <rule context="Invoice">
      <assert test="@id">needs id</assert>
    </rule>
  </pattern>
</schema>
