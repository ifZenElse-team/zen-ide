# ADR-0292: Plug-in tool windows declare their icon and their side

- **Status:** Accepted (the owner, 2026-11-15)
- **Date:** 2026-11-15
- **Builds on:** [ADR-0289](0289-plugin-tool-windows.md) (plug-in tool windows),
  [ADR-0290](0290-tool-window-choices-and-confirmations.md) (choices and confirmations)

## Context

The Docker and Kubernetes windows (ADR-0289, ADR-0290) all show one generic icon, always open at
the bottom left and are missing from View | Tool Windows. The owner wants them recognised at a
glance and on the right, as a tab, where the Git window sits once it is moved there. Built-in tool
windows draw their icons with vector painters in the palette's colours, so they follow light, dark
and custom themes; a plug-in is a WebAssembly module with no UI of its own and cannot paint.

## Decision

1. **An icon of the plug-in's own, as SVG markup.** A window's `"icon"` holds inline SVG that the
   plug-in's author draws from scratch (rule 2: no one else's icon or logo, and no product's
   mark). The platform keeps only the shapes' coverage and paints it in the colour the tool window
   bar uses for every icon (normal, hover, active), so a plug-in's icon follows the theme like a
   built-in one. A plug-in's own colours are ignored.
2. **The markup is untrusted input and is checked before anything renders it.** At most 4 KiB, a
   `viewBox` on the root, at most 16 levels of nesting, and only `svg` (the root alone), `g`,
   `path`, `circle`, `ellipse`, `rect`, `line`, `polyline` and `polygon`, with presentation and
   geometry attributes only. No text, images, `href` of any kind, `style`, `class`, `url(...)`,
   entities, DOCTYPE, comments or processing instructions. Rendering (resvg, already in the
   workspace, pure Rust) runs with no font database, no resources folder and image resolvers that
   resolve nothing, so nothing outside the markup is ever read. Markup that fails the check, or an
   old plain name such as `"docker"`, falls back to the generic icon, and Settings → Plugins says
   why. The mask is rendered at the device's pixel size and cached per markup and size.
3. **A default side.** `"side"`: `"left"`, `"right"` or `"bottom"` (the default), the top group of
   that side, as built-in windows have. It decides where the window appears the first time it
   enters a layout and where Restore Default Layout puts it; afterwards the person's place (drag,
   Move To) is kept in the saved layout like a built-in window's.
4. **No silent move.** A saved layout keeps a window where the person had it. When a window
   declares a side other than the one it has in the saved layout, the IDE says once (per IDE, per
   window) that it now opens on its side and that Restore Default Layout moves it there.
5. **In the menu.** View | Tool Windows lists the enabled plug-ins' windows after the built-in
   ones. Each entry runs `view.toolWindow.<id>`, so Find Action and agents reach them too.
6. `plugins.toolWindow.list` answers each window's `side` and whether its icon is drawn (`icon`);
   the markup itself is not sent to agents.

## Consequences

- The Docker and Kubernetes plug-ins declare `"side": "right"` and icons of our own (three stacked
  boxes; six dots around a centre), without anyone's logo.
- Any plug-in's window gets a recognisable icon in every theme with no platform code of its own.
- A person who had a plug-in window elsewhere keeps it there and is told once how to move it.

## Alternatives

- **Icons as images (PNG):** they would not follow the theme's colours and could not be checked as
  simply.
- **A list of named platform icons a plug-in picks from:** every new tool would need a platform
  change.
- **Moving the windows in existing layouts:** the place is the person's; rejected (decision 4).
