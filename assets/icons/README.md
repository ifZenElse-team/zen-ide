# Zen icon set

The product icons of the Zen IDE family, designed by the ifzenelse team in Figma. **The set is not
final**: refresh it whenever the design changes.

- **Source:** Figma file `ZenIcons` (the design file is private; the owner has the link), frame
  "Product — dark · Solid · Pastel" (node `10:417`), chosen by the owner on 2026-10-16. The component
  "Zen Icon" (node `3:164`) has every product in three styles (Line, Solid, Abstract); the earlier
  frame "Product — dark · Solid" (node `5:2`) has the same glyphs on saturated tiles.
- **Files:** `products/<id>.svg`, one per product id (`docs/naming.md`): a 104 × 104 rounded pastel tile
  in the product's colour with a dark glyph. Only the tile and the glyph are kept; Figma's export also
  contains the card behind the tile, which is removed so that the corners stay transparent.
- **Used by:** the product marks in Zen Toolbox and on the IDE welcome screen
  (`zen-ui::icons::product_mark`), the macOS application icons built by `cargo xtask release`
  (rendered with `resvg`, the tile covering the central 80 % of the canvas), and the landing page
  (`/icons/products/<id>.svg`).

## Refreshing the icons

1. In Figma, for each product, export the **Tile** frame of the "Product — dark · Solid · Pastel" frame as SVG
   (an agent with the Figma MCP server: `download_assets` with `defaultFormat: svg` on the tile nodes
   below).
2. Run `cargo xtask icon-import <export.svg> <id>`: it keeps only the tile and the glyph and writes
   `products/<id>.svg`.
3. Run `cargo xtask ci`, then release the affected products: the new icons reach users with the update.

| Product | Tile node |
|---|---|
| rustronin | `10:424` |
| gogarden | `10:431` |
| pypetal | `10:438` |
| jvmjizo | ours, not in Figma yet (2026-10-25) |
| webwave | `10:452` |
| cppcrane | `10:460` |
| sharpshuriken | `10:467` |
| phppagoda | `10:474` |
| rubyryokan | `10:481` |
| sqlscroll | `10:488` |
| devopsdaruma | ours, not in Figma yet (2026-10-18) |
| testtotami | ours, not in Figma yet (2026-10-19) |

`products/toolbox.svg` is Zen Toolbox's icon from the Figma file (2026-10-18, "Toolbox / Torii A"
in the frame "Zen Toolbox — in interface", node `50:737`): a white torii on a blue gradient tile
(`#8FB0FA` to `#3B54D6`), 128 units with a 31-unit corner radius. The tray (menu bar) icon is the
same torii drawn in code as a black template image (`zen_ui::tray`).
