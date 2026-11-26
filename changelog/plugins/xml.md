# XML plug-in changelog

Every release of the XML plug-in (`plugins/xml`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-10-24

### Added

- The first release, in every IDE. XML files by their extension (`.xml`, `.plist`, `.xsd`,
  `.xsl`, `.xslt`, `.svg`, `.wsdl`, `.pom`, `.csproj`, `.fsproj`, `.vbproj`, `.props`, `.targets`,
  `.resx`, `.xaml`, `.rss`, `.atom`, `.kml`, `.gpx`, `.opml`, `.dtd`, `.iml`) and, whatever their
  name, by their content: `<?xml`, a doctype other than HTML's, or a root element with `xmlns`.
- Highlighting of elements, attributes, values, references, comments, CDATA sections, the doctype,
  processing instructions and namespace prefixes; folding; the elements in the Structure view.
- End tags as you type: `>` after a start tag adds its end tag, `</` the open element's name.
- Problems as you type: end tags that do not match, elements not closed, end tags without a start
  tag, unfinished tags, comments and CDATA sections.
- A **New → XML File** template.
