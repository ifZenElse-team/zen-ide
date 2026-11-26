# AI Agents in the Terminal plug-in changelog

Every release of the AI Agents in the Terminal plug-in (`plugins/ai-agents`), newest first; the
format is in [../README.md](../README.md).

## 0.1.0 - 2026-11-11

### Added

- The first release, in the base set of every IDE: resume adapters for Claude Code
  (`claude --resume <id>`), Codex (`codex resume <id>`) and Qwen Code (`qwen --resume <id>`), so a
  terminal tab continues the agent's conversation after a restart of the computer once the agent
  reports its conversation id with a session-start hook.
