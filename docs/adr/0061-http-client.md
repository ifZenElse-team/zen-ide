# ADR-0061: The HTTP Client: `.http` files in a plug-in, sending in the platform

- **Status:** Proposed
- **Date:** 2026-10-19
- **Extends:** [ADR-0050](0050-products-are-plugin-bundles.md), [ADR-0040](0040-https-pure-rust-tls.md),
  [ADR-0033](0033-ai-access-to-every-api.md)

## Context

The requirement is an HTTP Client for `.http` files: requests written in
`.http` and `.rest` files, a run icon in the gutter for each, the response in a tool window, variables
from environment files, over the pure-Rust HTTPS of ADR-0040, in every IDE (ADR-0050). The file format
is the `.http` request format: requests separated by `###`, a request line, headers, a blank line and a body, `# @name`,
`{{variables}}`, `http-client.env.json` and `http-client.private.env.json`. Responses are not kept in the
project's `.idea/httpRequests`: we never write into `.idea` (ADR-0051).

A plug-in is a WebAssembly module without imports (ADR-0031, ADR-0056): it highlights, folds and
outlines, but it cannot reach the network, put icons in the gutter or show a tool window.

## Decision

1. **The language is a plug-in, `plugins/http`** (ABI v1): highlighting of methods, URLs, headers,
   `{{variables}}`, in-file variables, comments, directives, JSON bodies, body files and response
   handlers; one fold per request; the requests in the Structure view. It is in the base set
   (`BASE_PLUGINS`), since the HTTP Client is in every IDE.
2. **Sending is the platform's.** A new L2 crate, `zen-http`, reads `.http` files (the plug-in's
   lexer is its own, as a plug-in depends on no workspace crate), finds the environment files (the
   nearest folder, from the file's own up to the project's root, that has one; private values win;
   `$shared` goes into every environment), resolves variables (in-file `@name = value` first, then
   the environment, then `$uuid`, `$timestamp` and `$randomInt`), reads `< file` bodies relative to
   the request file and only from inside the project (links resolved), and sends the request with `ureq` over HTTP or over rustls with RustCrypto,
   exactly the stack of ADR-0040 and no new dependency. Any status is an answer. Only `http` and `https` URLs are sent. Redirects are
   followed by `zen-http` itself (10 at most) unless `# @no-redirect`: only to `http` and `https`
   addresses, `Authorization` and `Cookie` dropped when the server changes, `POST` turned into `GET`
   on 301, 302 and 303. No proxy; 15 s to connect and one minute for the whole exchange; at most
   16 MiB of body read into memory; certificates always checked. An unresolved variable refuses the request instead of sending it as written.
3. **Nothing is sent by itself** (AGENTS.md rule 10). A request goes out only when
   `http.request.run` runs: the gutter's run icon of that request, or an agent. Listing requests,
   opening, highlighting and drawing the file, selecting an environment and reading the history never
   connect anywhere; tests check it against a local listener.
4. **Agents need the consent the IDE asks for elsewhere**: the MCP server, which is off until the
   user turns it on (Settings → Tools → MCP Server, or `ZEN_AUTOMATION` for one run, ADR-0033). It is
   the same consent that lets an agent start programs (`run.process.start`) and type into terminals;
   there is no second prompt. `zen-cli` runs the command only when asked on its command line.
5. **Commands** (`zen_workbench::http`): `http.requests.list { path }`,
   `http.environment.select { env? }`, `http.request.run { path, index | name, env?, waitMs? }`,
   `http.responses.list { path?, limit? }` and `http.response.get { id, waitMs? }`. A run returns its
   exchange at once (state `running`) and finishes on a thread of its own, waking the window;
   `waitMs` waits for the answer (at most 60 s).
6. **The history is in the IDE's data folder, per project**:
   `<data>/http-client/<folder name>-<hash of the path>/history.json`, the newest 50 exchanges, each
   with the resolved request, the status, headers, the body as text (1 MiB at most; binary bodies are
   marked, not kept) and the time, and at most 16 MiB of bodies in all (the oldest go first). Values
   that come from `http-client.private.env.json` are sent but stay `{{name}}` in what is kept and
   returned. Nothing is written into the project: a data folder inside it keeps the history in
   memory only. Without a project or a data
   folder (`zen-cli`) it stays in memory.
7. **The UI:** a run icon on each request line, the **Services** tool window
   (⌘8) with the responses, newest first, and the one selected as the response console shows it
   (request line, status line, headers, the body with JSON indented, the summary), and the
   environment selector in its header. The selector stays in the tool window until the editor of
   a `.http` file has a toolbar.

## Consequences

- Every IDE gets the HTTP Client; the plug-in's catalog entry and release are the owner's
  (`cargo xtask release plugin http 0.1.0`).
- Not supported yet: response handler scripts (`> {% %}`, JavaScript), `>> file` redirections,
  `<> previous-response` comparisons, multipart bodies, GraphQL and WebSocket requests, cookies kept
  between requests, client certificates, proxies, and "Run all requests". The handlers and
  redirections are recognised, highlighted and left out of the body.
- A server with a self-signed certificate is refused, as for the IDE's own downloads (Mozilla's roots).
