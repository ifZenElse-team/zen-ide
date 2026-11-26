# The user guide

These pages are the documentation on the website, at `https://ide.ifzenelse.space/docs/`.
`cargo xtask site-docs` turns them into HTML (`deploy/releases/site/docs/`); the page of keyboard shortcuts is made from the keymap itself.

- One page per file, `NN-name.md`; the number orders the menu and `name` is the page's address.
- The first line is the page's title (`# Title`), the first paragraph its summary.
- Write for people using the IDE, in English, short and exact: what a feature does, how to reach
  it (menu, shortcut, button), and what to expect. Name shortcuts as macOS shows them (⇧⌘F).
- **Every change that adds or changes something users see updates the guide in the same pull
  request** (AGENTS.md, docs/ai-workflow.md).
