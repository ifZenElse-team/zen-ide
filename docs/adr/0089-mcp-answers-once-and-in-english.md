# ADR-0089: MCP answers carry their payload once, and speak English to agents

- **Status:** Proposed
- **Date:** 2026-10-21
- **Extends:** [ADR-0033](0033-ai-access-to-every-api.md) (every capability reachable by AI),
  [ADR-0087](0087-agent-sized-mcp-answers.md) (agent-sized answers),
  [ADR-0024](0024-localisation.md) (localisation); part of developing Zen IDE through RustRonin

## Context

**Every answer went over the wire twice.** `zen_automation::handle` answered each successful
`tools/call` with the result as JSON text in a `text` content block *and* as `structuredContent`
(an object, or `{"result": value}` for anything else). Measured on GoGarden 0.38.27 with a small Go
module: `problems.file.list` 1 563 bytes for a 669-byte payload, `navigate.symbol.goTo` 14 978 bytes
for 6 982, `list_windows` 468 for 170.

What the protocol requires, MCP specification 2025-06-18, Server → Tools
(<https://modelcontextprotocol.io/specification/2025-06-18/server/tools>), read 2026-10-21:

- *Tool Result*: `content` is required in a `CallToolResult`; text goes in `{"type": "text"}` blocks.
- *Structured Content*: "Structured content is returned as a JSON object in the `structuredContent`
  field of a result. For backwards compatibility, a tool that returns structured content SHOULD
  also return the serialized JSON in a TextContent block."
- *Output Schema*: a tool MAY declare an `outputSchema`; "If an output schema is provided: Servers
  MUST provide structured results that conform to this schema."

So `structuredContent` is required only of a tool that declares an `outputSchema`, and no Zen IDE
tool declares one; the text block is what every client can read.

What Claude Code reads, measured 2026-10-21 with Claude Code 2.1.306 (`claude -p
--strict-mcp-config` against a stdio probe server whose tools answer different words in the text
and in `structuredContent`): with both present the model is given `structuredContent` serialised as
JSON and the text is dropped, with or without an `outputSchema`; with text only, the text; with
`structuredContent` only (and empty `content`), `structuredContent`. Either form alone therefore
reaches the model whole; sending both bought nothing.

**Words depended on the machine.** Error messages, reasons and other text an agent got over MCP
came from `zen_i18n::t!` in the IDE's UI language, which follows the system: on a Russian Mac an
agent got `not_unique` with a Russian `message`, and `describe_command` of an unknown id answered in
Russian. Agents work best in English, and what an agent gets should not depend on whose computer
it runs on. Tool descriptions, the server's `instructions` and command titles and descriptions
were already English (they are not Fluent messages).

## Decision

1. **One form.** A successful tool result is its JSON, compact (`serde_json` without whitespace),
   in one `text` content block; `structuredContent` is not sent. A tool may declare
   `zen_automation::Tool::output_schema`; such a tool lists `outputSchema` and answers
   `structuredContent` as well, with the same JSON in the text block, as the specification
   requires. Errors are unchanged: `isError: true` with the words (or the coded-error JSON of
   ADR-0087) as text. `zen_automation::instances::call` (the CLI's client) reads the text and falls
   back to `structuredContent` for older IDEs.
2. **English for agents, by a setting.** `zen_i18n::with_language(code, work)` gives one thread
   another language while `work` runs (nested calls: the innermost wins; other threads and the
   chosen UI language are untouched). The IDE answers every tool call inside
   `with_language(mcpLanguage)`, so every `t!` text produced for it (command errors, reasons,
   messages, Find Action names) is in that language. `mcpLanguage` is a setting, Settings → Tools →
   MCP Server → **Language for agents**, a code of `zen_i18n::LANGUAGES`, `en` by default whatever
   the system's and the UI's language; `set_settings` refuses another code. Error codes and fields
   are the same in every language; the UI keeps its own language.

## Consequences

- Answers shrink to the payload plus a constant envelope and the text's JSON escaping: measured
  with the same module, `problems.file.list` 1 563 → 862 bytes, `navigate.symbol.goTo`
  14 978 → 7 975, `list_windows` 468 → 277 (about −45 %). What the model sees in Claude Code is the
  same JSON as before (it came from `structuredContent` before, from the text now), except that a
  result that is not an object is no longer wrapped in `{"result": …}`.
- A client that read only `structuredContent` must read the text; the CLI does both.
- An agent on a Russian (or any) system gets English unless the owner chooses otherwise.
- Text produced for an agent later, outside the call (a key press answered after the next frame),
  uses the UI language; such answers carry only window state, not messages.
