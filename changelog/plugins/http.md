# HTTP Client plug-in changelog

Every release of the HTTP Client plug-in (`plugins/http`), newest first; the format is in
[../README.md](../README.md).

## 0.1.0 - 2026-10-19

### Added

- The first release, for `*.http` and `*.rest` files: highlighting of methods, URLs, protocol
  versions, headers, `{{variables}}`, in-file variables, comments, `# @name` and `###` separators,
  JSON bodies, body files (`< file`) and response handlers; folding of each request; the requests in
  the Structure view. The IDE itself runs the requests (the gutter's run icons).
