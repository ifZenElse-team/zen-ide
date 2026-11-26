# ADR-0125: Kozō, the family mascot

- **Status:** Proposed
- **Date:** 2026-10-22
- **Relates to:** [ADR-0002](0002-open-source-clean-room-pure-rust.md) (clean-room),
  [ADR-0019](0019-alliterative-product-names.md) (product names and their images),
  [ADR-0033](0033-ai-access-to-every-api.md) (commands for agents)

## Context

The family has product marks (the tiles) but no character. The owner wanted one that ties the
products together, matches the Japanese images of their names (docs/naming.md) and the tone of
Caffeine's coffee drinker, the IDE's first pixel art (`crates/platform/ui/src/ide/caffeine/`). The
owner approved a design on 2026-10-22: a sketch in Python that draws every frame from shapes on a
grid of 40 by 40 pixels, with a page of the frames to look at (version 2).

Wherever Kozō shows, he is an addition to the layout the UI specification gives, as the welcome
screen's language switch is.

## Decision

1. **The character.** Kozō (小僧), a young novice monk: a big bald head with a shine and rosy
   cheeks, a kimono with wide sleeves crossed left over right and a dark obi, sitting in the lotus
   position with the hands in the meditation mudra on the feet. The art is ours, drawn for this
   project; nothing is taken from any other product.

2. **States**, each an animation of **8 frames of 160 ms**, looping:
   - **idle**: meditating, eyes closed, smiling, breathing (the whole figure sinks a pixel in frames
     2 to 5);
   - **work**: the same with the eyes open and a blink in frame 6;
   - **success**: happy eyes, a little hop, sparkles twinkling round him;
   - **error**: dizzy eyes and a flat mouth, the head shaking, a blinking "!", a drop of sweat;
   - **sleep**: the head tilted, breathing slowly, the mouth a small "o" on the breath, zZ rising.

3. **Variants.** Each product's Kozō wears a robe in the two colours of its tile (the same values as
   `icons::product_colors`) and has a prop from the image of its name, which moves in idle and work:
   GoGarden a watering can (drops fall, a sprout grows), RustRonin a wooden sword lying in front (a
   glint runs along it), PyPetal a cherry blossom on the head (a petal falls), JvmJizo a small
   stone Jizō in a red bib (a maple leaf drifts down to it), WebWave a surfboard behind him (it bobs; a wave rolls along the
   floor), CppCrane a paper crane on the head (it flaps), SharpShuriken a shuriken (it spins),
   PhpPagoda a pagoda (the spire glints), RubyRyokan a paper lantern (it sways and flickers),
   SqlScroll a scroll in the lap (it unrolls and rolls up), DevopsDaruma a daruma doll (it wobbles
   and stands back up), TestTotami a tatami mat under him (laid mat by mat), Zen Toolbox a toolbox
   (a spark comes out). The family's Kozō wears the Zen indigo without a prop; Zen Toolbox's wears
   the indigo too. A product id without a variant gets the family's. In success, error and sleep
   he has put his prop down: those frames differ between products in the robe only.

4. **Sprite rules.** Every frame is a grid of 40 by 40 **inks**, not colours, built in code from
   shapes (ellipses, rows, single pixels) and small hand-drawn props, then **outlined in one pass**
   (a clear pixel next to a solid one becomes the outline; zZ, sparkles and "!" float without one).
   Each ink has one fixed colour; only the robe's two come from the product. The grids live in a
   new crate, **`zen-mascot`** (L0: no dependencies, no UI toolkit, compiles for wasm), so that the
   UI and the build tools draw the same pixels. A test pins a fingerprint of every frame of every
   variant to the approved design, so a change to the art is a visible, reviewed change.

5. **Painting.** The UI paints a frame as one mesh of plain rectangles, scaled up by a **whole
   number of physical pixels** per art pixel and placed on the physical pixel grid, so that every
   pixel is square and sharp at any display scale. He moves only while the window is visible and
   the system's **Reduce motion** is off; otherwise he shows the **still frame** (frame 0, at rest)
   and asks for no frames at all. The platform has no setting of its own for animations; if one is
   added, Kozō follows it as Reduce motion.

6. **Where he appears.**
   - the **splash** of every IDE, under the product's mark, name and version (two points an art
     pixel);
   - the **welcome screen**, at the bottom of the left column above the settings gear, while there
     is room for him;
   - Zen Toolbox's **Projects** tab, over its empty state;
   - the **website**: the family's Kozō and those of the released products and Zen Toolbox, as
     animated PNGs (one art pixel one image pixel, scaled up with `image-rendering: pixelated`),
     with a still picture for visitors who ask for reduced motion (`prefers-reduced-motion`).
     `cargo xtask mascot` writes them from the same grids into `deploy/releases/site/img/kozo/`;
     they are committed, and a test fails while they are out of date.

   All of them show **idle**. The other states are drawn and tested, for the places that will show
   what the IDE is doing (a build or test run, an agent at work, an error); each such use is decided
   when it is built.

7. **No commands.** Kozō is decoration: he has no action of his own, so there is nothing to add for
   agents (ADR-0033); he has no text, so nothing to translate.

## Consequences

- A new L0 crate, `zen-mascot`, registered in `xtask/src/layers.rs` and docs/architecture.md; `zen-ui`
  and `xtask` depend on it.
- The system's Reduce motion is read in one place, `zen_ui::accessibility::reduce_motion`, for
  Caffeine's drinker and Kozō alike.
- A new product gets its Kozō by adding a variant (robe and prop) to `zen-mascot` and a fingerprint
  to its test; `cargo xtask mascot` is run again when it is shown on the website.
- The welcome screen and splash show Kozō; the UI specification (docs/behaviour/ide/welcome.md)
  notes it.
