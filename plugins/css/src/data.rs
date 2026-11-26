pub(crate) struct Property {
    pub(crate) name: &'static str,
    pub(crate) values: &'static [&'static str],
    pub(crate) colour: bool,
}

const fn p(name: &'static str, values: &'static [&'static str]) -> Property {
    Property { name, values, colour: false }
}

const fn c(name: &'static str, values: &'static [&'static str]) -> Property {
    Property { name, values, colour: true }
}

pub(crate) const GLOBAL_VALUES: &[&str] = &["inherit", "initial", "unset", "revert", "revert-layer"];

const AUTO: &[&str] = &["auto"];
const NONE: &[&str] = &["none"];
const NORMAL: &[&str] = &["normal"];
const SIZE: &[&str] = &["auto", "min-content", "max-content", "fit-content", "stretch"];
const MAX_SIZE: &[&str] = &["none", "min-content", "max-content", "fit-content", "stretch"];
const DISPLAY: &[&str] = &[
    "block",
    "inline",
    "inline-block",
    "flex",
    "inline-flex",
    "grid",
    "inline-grid",
    "flow-root",
    "none",
    "contents",
    "list-item",
    "table",
    "inline-table",
    "table-row",
    "table-cell",
    "table-column",
    "table-caption",
    "table-row-group",
    "table-header-group",
    "table-footer-group",
    "table-column-group",
    "ruby",
];
const POSITION: &[&str] = &["static", "relative", "absolute", "fixed", "sticky"];
const FLEX_DIRECTION: &[&str] = &["row", "row-reverse", "column", "column-reverse"];
const FLEX_WRAP: &[&str] = &["nowrap", "wrap", "wrap-reverse"];
const FLEX_FLOW: &[&str] = &["row", "row-reverse", "column", "column-reverse", "nowrap", "wrap", "wrap-reverse"];
const FLEX: &[&str] = &["auto", "none", "content"];
const JUSTIFY_CONTENT: &[&str] = &[
    "normal",
    "flex-start",
    "flex-end",
    "start",
    "end",
    "center",
    "left",
    "right",
    "space-between",
    "space-around",
    "space-evenly",
    "stretch",
];
const ALIGN_CONTENT: &[&str] = &[
    "normal",
    "flex-start",
    "flex-end",
    "start",
    "end",
    "center",
    "space-between",
    "space-around",
    "space-evenly",
    "stretch",
    "baseline",
];
const ALIGN_ITEMS: &[&str] =
    &["normal", "stretch", "center", "start", "end", "flex-start", "flex-end", "self-start", "self-end", "baseline"];
const ALIGN_SELF: &[&str] = &[
    "auto",
    "normal",
    "stretch",
    "center",
    "start",
    "end",
    "flex-start",
    "flex-end",
    "self-start",
    "self-end",
    "baseline",
];
const JUSTIFY_ITEMS: &[&str] =
    &["normal", "stretch", "center", "start", "end", "left", "right", "self-start", "self-end", "baseline", "legacy"];
const JUSTIFY_SELF: &[&str] =
    &["auto", "normal", "stretch", "center", "start", "end", "left", "right", "self-start", "self-end", "baseline"];
const OVERFLOW: &[&str] = &["visible", "hidden", "clip", "scroll", "auto"];
const BORDER_STYLE: &[&str] =
    &["none", "hidden", "solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset"];
const OUTLINE_STYLE: &[&str] =
    &["auto", "none", "solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset"];
const BORDER_WIDTH: &[&str] = &["thin", "medium", "thick"];
const BORDER: &[&str] = &[
    "none", "hidden", "solid", "dashed", "dotted", "double", "groove", "ridge", "inset", "outset", "thin", "medium",
    "thick",
];
const TEXT_ALIGN: &[&str] = &["left", "right", "center", "justify", "start", "end", "match-parent"];
const TEXT_ALIGN_LAST: &[&str] = &["auto", "left", "right", "center", "justify", "start", "end"];
const FONT_WEIGHT: &[&str] =
    &["normal", "bold", "bolder", "lighter", "100", "200", "300", "400", "500", "600", "700", "800", "900"];
const FONT_STYLE: &[&str] = &["normal", "italic", "oblique"];
const FONT_SIZE: &[&str] = &[
    "xx-small",
    "x-small",
    "small",
    "medium",
    "large",
    "x-large",
    "xx-large",
    "xxx-large",
    "smaller",
    "larger",
    "math",
];
const FONT_FAMILY: &[&str] = &[
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "math",
    "emoji",
    "fangsong",
];
const FONT: &[&str] = &[
    "caption",
    "icon",
    "menu",
    "message-box",
    "small-caption",
    "status-bar",
    "normal",
    "italic",
    "oblique",
    "bold",
    "bolder",
    "lighter",
    "small-caps",
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
];
const FONT_VARIANT: &[&str] =
    &["normal", "none", "small-caps", "all-small-caps", "petite-caps", "all-petite-caps", "unicase", "titling-caps"];
const FONT_STRETCH: &[&str] = &[
    "normal",
    "ultra-condensed",
    "extra-condensed",
    "condensed",
    "semi-condensed",
    "semi-expanded",
    "expanded",
    "extra-expanded",
    "ultra-expanded",
];
const FONT_DISPLAY: &[&str] = &["auto", "block", "swap", "fallback", "optional"];
const FONT_KERNING: &[&str] = &["auto", "normal", "none"];
const FONT_SMOOTHING: &[&str] = &["auto", "none", "antialiased", "subpixel-antialiased"];
const WHITE_SPACE: &[&str] = &["normal", "nowrap", "pre", "pre-wrap", "pre-line", "break-spaces"];
const WHITE_SPACE_COLLAPSE: &[&str] = &["collapse", "preserve", "preserve-breaks", "preserve-spaces", "break-spaces"];
const TEXT_WRAP: &[&str] = &["wrap", "nowrap", "balance", "pretty", "stable"];
const CURSOR: &[&str] = &[
    "auto",
    "default",
    "none",
    "pointer",
    "text",
    "vertical-text",
    "move",
    "wait",
    "progress",
    "help",
    "crosshair",
    "not-allowed",
    "no-drop",
    "grab",
    "grabbing",
    "context-menu",
    "cell",
    "copy",
    "alias",
    "all-scroll",
    "col-resize",
    "row-resize",
    "n-resize",
    "e-resize",
    "s-resize",
    "w-resize",
    "ne-resize",
    "nw-resize",
    "se-resize",
    "sw-resize",
    "ew-resize",
    "ns-resize",
    "nesw-resize",
    "nwse-resize",
    "zoom-in",
    "zoom-out",
];
const VISIBILITY: &[&str] = &["visible", "hidden", "collapse"];
const BOX_SIZING: &[&str] = &["content-box", "border-box"];
const FLOAT: &[&str] = &["left", "right", "none", "inline-start", "inline-end"];
const CLEAR: &[&str] = &["left", "right", "both", "none", "inline-start", "inline-end"];
const TEXT_TRANSFORM: &[&str] = &["none", "capitalize", "uppercase", "lowercase", "full-width", "full-size-kana"];
const TEXT_DECORATION_LINE: &[&str] = &["none", "underline", "overline", "line-through"];
const TEXT_DECORATION_STYLE: &[&str] = &["solid", "double", "dotted", "dashed", "wavy"];
const TEXT_DECORATION: &[&str] =
    &["none", "underline", "overline", "line-through", "solid", "double", "dotted", "dashed", "wavy"];
const TEXT_OVERFLOW: &[&str] = &["clip", "ellipsis"];
const TEXT_UNDERLINE_POSITION: &[&str] = &["auto", "under", "left", "right", "from-font"];
const TEXT_RENDERING: &[&str] = &["auto", "optimizeSpeed", "optimizeLegibility", "geometricPrecision"];
const TEXT_ORIENTATION: &[&str] = &["mixed", "upright", "sideways"];
const POINTER_EVENTS: &[&str] =
    &["auto", "none", "all", "visiblePainted", "visibleFill", "visibleStroke", "visible", "painted", "fill", "stroke"];
const USER_SELECT: &[&str] = &["auto", "none", "text", "all", "contain"];
const OBJECT_FIT: &[&str] = &["fill", "contain", "cover", "none", "scale-down"];
const POSITION_KEYWORDS: &[&str] = &["top", "right", "bottom", "left", "center"];
const LIST_STYLE_TYPE: &[&str] = &[
    "disc",
    "circle",
    "square",
    "decimal",
    "decimal-leading-zero",
    "lower-roman",
    "upper-roman",
    "lower-alpha",
    "upper-alpha",
    "lower-latin",
    "upper-latin",
    "lower-greek",
    "disclosure-open",
    "disclosure-closed",
    "none",
];
const LIST_STYLE_POSITION: &[&str] = &["inside", "outside"];
const LIST_STYLE: &[&str] = &[
    "none",
    "inside",
    "outside",
    "disc",
    "circle",
    "square",
    "decimal",
    "lower-alpha",
    "upper-alpha",
    "lower-roman",
    "upper-roman",
];
const BACKGROUND_REPEAT: &[&str] = &["repeat", "repeat-x", "repeat-y", "no-repeat", "space", "round"];
const BACKGROUND_SIZE: &[&str] = &["auto", "cover", "contain"];
const BACKGROUND_ATTACHMENT: &[&str] = &["scroll", "fixed", "local"];
const BOX: &[&str] = &["border-box", "padding-box", "content-box"];
const BACKGROUND_CLIP: &[&str] = &["border-box", "padding-box", "content-box", "text"];
const BACKGROUND: &[&str] = &[
    "none",
    "repeat",
    "repeat-x",
    "repeat-y",
    "no-repeat",
    "space",
    "round",
    "scroll",
    "fixed",
    "local",
    "top",
    "right",
    "bottom",
    "left",
    "center",
    "cover",
    "contain",
    "border-box",
    "padding-box",
    "content-box",
];
const BLEND_MODE: &[&str] = &[
    "normal",
    "multiply",
    "screen",
    "overlay",
    "darken",
    "lighten",
    "color-dodge",
    "color-burn",
    "hard-light",
    "soft-light",
    "difference",
    "exclusion",
    "hue",
    "saturation",
    "color",
    "luminosity",
];
const WORD_BREAK: &[&str] = &["normal", "break-all", "keep-all", "break-word", "auto-phrase"];
const OVERFLOW_WRAP: &[&str] = &["normal", "break-word", "anywhere"];
const VERTICAL_ALIGN: &[&str] = &["baseline", "sub", "super", "text-top", "text-bottom", "middle", "top", "bottom"];
const EASING: &[&str] = &["ease", "ease-in", "ease-out", "ease-in-out", "linear", "step-start", "step-end"];
const TRANSITION: &[&str] = &[
    "all",
    "none",
    "ease",
    "ease-in",
    "ease-out",
    "ease-in-out",
    "linear",
    "step-start",
    "step-end",
    "allow-discrete",
];
const TRANSITION_PROPERTY: &[&str] = &["all", "none"];
const TRANSITION_BEHAVIOR: &[&str] = &["normal", "allow-discrete"];
const ANIMATION_DIRECTION: &[&str] = &["normal", "reverse", "alternate", "alternate-reverse"];
const ANIMATION_FILL_MODE: &[&str] = &["none", "forwards", "backwards", "both"];
const ANIMATION_PLAY_STATE: &[&str] = &["running", "paused"];
const ANIMATION: &[&str] = &[
    "none",
    "infinite",
    "normal",
    "reverse",
    "alternate",
    "alternate-reverse",
    "forwards",
    "backwards",
    "both",
    "running",
    "paused",
    "ease",
    "ease-in",
    "ease-out",
    "ease-in-out",
    "linear",
    "step-start",
    "step-end",
];
const INFINITE: &[&str] = &["infinite"];
const RESIZE: &[&str] = &["none", "both", "horizontal", "vertical", "block", "inline"];
const GRID_AUTO_FLOW: &[&str] = &["row", "column", "dense"];
const GRID_TEMPLATE: &[&str] = &["none", "auto", "min-content", "max-content", "subgrid", "masonry"];
const GRID_AUTO: &[&str] = &["auto", "min-content", "max-content"];
const GRID_LINE: &[&str] = &["auto", "span"];
const ISOLATION: &[&str] = &["auto", "isolate"];
const DIRECTION: &[&str] = &["ltr", "rtl"];
const WRITING_MODE: &[&str] = &["horizontal-tb", "vertical-rl", "vertical-lr", "sideways-rl", "sideways-lr"];
const UNICODE_BIDI: &[&str] = &["normal", "embed", "isolate", "bidi-override", "isolate-override", "plaintext"];
const TABLE_LAYOUT: &[&str] = &["auto", "fixed"];
const BORDER_COLLAPSE: &[&str] = &["collapse", "separate"];
const CAPTION_SIDE: &[&str] = &["top", "bottom"];
const EMPTY_CELLS: &[&str] = &["show", "hide"];
const CONTENT: &[&str] = &["none", "normal", "open-quote", "close-quote", "no-open-quote", "no-close-quote"];
const QUOTES: &[&str] = &["none", "auto"];
const SCROLL_BEHAVIOR: &[&str] = &["auto", "smooth"];
const OVERSCROLL: &[&str] = &["auto", "contain", "none"];
const SCROLL_SNAP_TYPE: &[&str] = &["none", "x", "y", "block", "inline", "both", "mandatory", "proximity"];
const SCROLL_SNAP_ALIGN: &[&str] = &["none", "start", "end", "center"];
const SCROLL_SNAP_STOP: &[&str] = &["normal", "always"];
const SCROLLBAR_WIDTH: &[&str] = &["auto", "thin", "none"];
const SCROLLBAR_GUTTER: &[&str] = &["auto", "stable", "both-edges"];
const APPEARANCE: &[&str] = &["none", "auto", "menulist-button", "textfield", "button", "checkbox", "radio"];
const BACKFACE: &[&str] = &["visible", "hidden"];
const TRANSFORM_STYLE: &[&str] = &["flat", "preserve-3d"];
const TRANSFORM_BOX: &[&str] = &["content-box", "border-box", "fill-box", "stroke-box", "view-box"];
const CONTAIN: &[&str] = &["none", "strict", "content", "size", "inline-size", "layout", "style", "paint"];
const CONTAINER_TYPE: &[&str] = &["normal", "size", "inline-size"];
const CONTENT_VISIBILITY: &[&str] = &["visible", "auto", "hidden"];
const WILL_CHANGE: &[&str] = &["auto", "scroll-position", "contents", "transform", "opacity"];
const TOUCH_ACTION: &[&str] =
    &["auto", "none", "pan-x", "pan-y", "pan-left", "pan-right", "pan-up", "pan-down", "pinch-zoom", "manipulation"];
const IMAGE_RENDERING: &[&str] = &["auto", "smooth", "high-quality", "crisp-edges", "pixelated"];
const HYPHENS: &[&str] = &["none", "manual", "auto"];
const LINE_BREAK: &[&str] = &["auto", "loose", "normal", "strict", "anywhere"];
const COLOR_SCHEME: &[&str] = &["normal", "light", "dark", "only"];
const PRINT_COLOR_ADJUST: &[&str] = &["economy", "exact"];
const FORCED_COLOR_ADJUST: &[&str] = &["auto", "none", "preserve-parent-color"];
const BREAK: &[&str] = &[
    "auto",
    "avoid",
    "always",
    "all",
    "avoid-page",
    "page",
    "left",
    "right",
    "recto",
    "verso",
    "avoid-column",
    "column",
];
const BREAK_INSIDE: &[&str] = &["auto", "avoid", "avoid-page", "avoid-column"];
const PAGE_BREAK: &[&str] = &["auto", "always", "avoid", "left", "right"];
const BOX_DECORATION_BREAK: &[&str] = &["slice", "clone"];
const COLUMN_FILL: &[&str] = &["auto", "balance"];
const COLUMN_SPAN: &[&str] = &["none", "all"];
const STROKE_LINECAP: &[&str] = &["butt", "round", "square"];
const STROKE_LINEJOIN: &[&str] = &["miter", "round", "bevel"];
const FILL_RULE: &[&str] = &["nonzero", "evenodd"];
const MASK_MODE: &[&str] = &["alpha", "luminance", "match-source"];
const BOX_SHADOW: &[&str] = &["none", "inset"];
const ACCENT: &[&str] = &["auto"];
const CARET: &[&str] = &["auto", "transparent"];
const ASPECT_RATIO: &[&str] = &["auto"];
const LINE_HEIGHT: &[&str] = &["normal"];
const COUNTER: &[&str] = &["none"];
const TEXT_EMPHASIS: &[&str] = &["none", "filled", "open", "dot", "circle", "double-circle", "triangle", "sesame"];
const TEXT_SIZE_ADJUST: &[&str] = &["none", "auto"];
const OFFSET_ROTATE: &[&str] = &["auto", "reverse"];
const ZOOM: &[&str] = &["normal", "reset"];
const VIEW_TRANSITION_NAME: &[&str] = &["none", "auto"];
const FIELD_SIZING: &[&str] = &["content", "fixed"];
const ANCHOR_NAME: &[&str] = &["none"];
const POSITION_AREA: &[&str] = &["none", "top", "bottom", "left", "right", "center", "span-all", "start", "end"];
const INTERPOLATE_SIZE: &[&str] = &["numeric-only", "allow-keywords"];

pub(crate) const PROPERTIES: &[Property] = &[
    c("accent-color", ACCENT),
    p("align-content", ALIGN_CONTENT),
    p("align-items", ALIGN_ITEMS),
    p("align-self", ALIGN_SELF),
    p("all", &[]),
    p("anchor-name", ANCHOR_NAME),
    p("animation", ANIMATION),
    p("animation-delay", &[]),
    p("animation-direction", ANIMATION_DIRECTION),
    p("animation-duration", AUTO),
    p("animation-fill-mode", ANIMATION_FILL_MODE),
    p("animation-iteration-count", INFINITE),
    p("animation-name", NONE),
    p("animation-play-state", ANIMATION_PLAY_STATE),
    p("animation-timing-function", EASING),
    p("appearance", APPEARANCE),
    p("aspect-ratio", ASPECT_RATIO),
    p("backdrop-filter", NONE),
    p("backface-visibility", BACKFACE),
    c("background", BACKGROUND),
    p("background-attachment", BACKGROUND_ATTACHMENT),
    p("background-blend-mode", BLEND_MODE),
    p("background-clip", BACKGROUND_CLIP),
    c("background-color", &[]),
    p("background-image", NONE),
    p("background-origin", BOX),
    p("background-position", POSITION_KEYWORDS),
    p("background-position-x", &["left", "center", "right"]),
    p("background-position-y", &["top", "center", "bottom"]),
    p("background-repeat", BACKGROUND_REPEAT),
    p("background-size", BACKGROUND_SIZE),
    p("block-size", SIZE),
    c("border", BORDER),
    c("border-block", BORDER),
    c("border-block-color", &[]),
    c("border-block-end", BORDER),
    c("border-block-start", BORDER),
    p("border-block-style", BORDER_STYLE),
    p("border-block-width", BORDER_WIDTH),
    c("border-bottom", BORDER),
    c("border-bottom-color", &[]),
    p("border-bottom-left-radius", &[]),
    p("border-bottom-right-radius", &[]),
    p("border-bottom-style", BORDER_STYLE),
    p("border-bottom-width", BORDER_WIDTH),
    p("border-collapse", BORDER_COLLAPSE),
    c("border-color", &[]),
    p("border-end-end-radius", &[]),
    p("border-end-start-radius", &[]),
    p("border-image", NONE),
    p("border-image-outset", &[]),
    p("border-image-repeat", &["stretch", "repeat", "round", "space"]),
    p("border-image-slice", &["fill"]),
    p("border-image-source", NONE),
    p("border-image-width", AUTO),
    c("border-inline", BORDER),
    c("border-inline-color", &[]),
    c("border-inline-end", BORDER),
    c("border-inline-start", BORDER),
    p("border-inline-style", BORDER_STYLE),
    p("border-inline-width", BORDER_WIDTH),
    c("border-left", BORDER),
    c("border-left-color", &[]),
    p("border-left-style", BORDER_STYLE),
    p("border-left-width", BORDER_WIDTH),
    p("border-radius", &[]),
    c("border-right", BORDER),
    c("border-right-color", &[]),
    p("border-right-style", BORDER_STYLE),
    p("border-right-width", BORDER_WIDTH),
    p("border-spacing", &[]),
    p("border-start-end-radius", &[]),
    p("border-start-start-radius", &[]),
    p("border-style", BORDER_STYLE),
    c("border-top", BORDER),
    c("border-top-color", &[]),
    p("border-top-left-radius", &[]),
    p("border-top-right-radius", &[]),
    p("border-top-style", BORDER_STYLE),
    p("border-top-width", BORDER_WIDTH),
    p("border-width", BORDER_WIDTH),
    p("bottom", AUTO),
    p("box-decoration-break", BOX_DECORATION_BREAK),
    c("box-shadow", BOX_SHADOW),
    p("box-sizing", BOX_SIZING),
    p("break-after", BREAK),
    p("break-before", BREAK),
    p("break-inside", BREAK_INSIDE),
    p("caption-side", CAPTION_SIDE),
    c("caret-color", CARET),
    p("clear", CLEAR),
    p("clip-path", NONE),
    c("color", &[]),
    p("color-scheme", COLOR_SCHEME),
    p("column-count", AUTO),
    p("column-fill", COLUMN_FILL),
    p("column-gap", NORMAL),
    c("column-rule", BORDER),
    c("column-rule-color", &[]),
    p("column-rule-style", BORDER_STYLE),
    p("column-rule-width", BORDER_WIDTH),
    p("column-span", COLUMN_SPAN),
    p("column-width", AUTO),
    p("columns", AUTO),
    p("contain", CONTAIN),
    p("container", NONE),
    p("container-name", NONE),
    p("container-type", CONTAINER_TYPE),
    p("content", CONTENT),
    p("content-visibility", CONTENT_VISIBILITY),
    p("counter-increment", COUNTER),
    p("counter-reset", COUNTER),
    p("counter-set", COUNTER),
    p("cursor", CURSOR),
    p("direction", DIRECTION),
    p("display", DISPLAY),
    p("empty-cells", EMPTY_CELLS),
    p("field-sizing", FIELD_SIZING),
    c("fill", NONE),
    p("fill-opacity", &[]),
    p("fill-rule", FILL_RULE),
    p("filter", NONE),
    p("flex", FLEX),
    p("flex-basis", &["auto", "content", "min-content", "max-content", "fit-content"]),
    p("flex-direction", FLEX_DIRECTION),
    p("flex-flow", FLEX_FLOW),
    p("flex-grow", &[]),
    p("flex-shrink", &[]),
    p("flex-wrap", FLEX_WRAP),
    p("float", FLOAT),
    p("font", FONT),
    p("font-display", FONT_DISPLAY),
    p("font-family", FONT_FAMILY),
    p("font-feature-settings", NORMAL),
    p("font-kerning", FONT_KERNING),
    p("font-optical-sizing", &["auto", "none"]),
    p("font-size", FONT_SIZE),
    p("font-size-adjust", NONE),
    p("font-stretch", FONT_STRETCH),
    p("font-style", FONT_STYLE),
    p("font-variant", FONT_VARIANT),
    p(
        "font-variant-numeric",
        &[
            "normal",
            "lining-nums",
            "oldstyle-nums",
            "proportional-nums",
            "tabular-nums",
            "diagonal-fractions",
            "stacked-fractions",
            "ordinal",
            "slashed-zero",
        ],
    ),
    p("font-variation-settings", NORMAL),
    p("font-weight", FONT_WEIGHT),
    p("forced-color-adjust", FORCED_COLOR_ADJUST),
    p("gap", NORMAL),
    p("grid", GRID_TEMPLATE),
    p("grid-area", GRID_LINE),
    p("grid-auto-columns", GRID_AUTO),
    p("grid-auto-flow", GRID_AUTO_FLOW),
    p("grid-auto-rows", GRID_AUTO),
    p("grid-column", GRID_LINE),
    p("grid-column-end", GRID_LINE),
    p("grid-column-start", GRID_LINE),
    p("grid-row", GRID_LINE),
    p("grid-row-end", GRID_LINE),
    p("grid-row-start", GRID_LINE),
    p("grid-template", GRID_TEMPLATE),
    p("grid-template-areas", NONE),
    p("grid-template-columns", GRID_TEMPLATE),
    p("grid-template-rows", GRID_TEMPLATE),
    p("height", SIZE),
    p("hyphens", HYPHENS),
    p("image-rendering", IMAGE_RENDERING),
    p("inline-size", SIZE),
    p("inset", AUTO),
    p("inset-block", AUTO),
    p("inset-block-end", AUTO),
    p("inset-block-start", AUTO),
    p("inset-inline", AUTO),
    p("inset-inline-end", AUTO),
    p("inset-inline-start", AUTO),
    p("interpolate-size", INTERPOLATE_SIZE),
    p("isolation", ISOLATION),
    p("justify-content", JUSTIFY_CONTENT),
    p("justify-items", JUSTIFY_ITEMS),
    p("justify-self", JUSTIFY_SELF),
    p("left", AUTO),
    p("letter-spacing", NORMAL),
    p("line-break", LINE_BREAK),
    p("line-clamp", NONE),
    p("line-height", LINE_HEIGHT),
    p("list-style", LIST_STYLE),
    p("list-style-image", NONE),
    p("list-style-position", LIST_STYLE_POSITION),
    p("list-style-type", LIST_STYLE_TYPE),
    p("margin", AUTO),
    p("margin-block", AUTO),
    p("margin-block-end", AUTO),
    p("margin-block-start", AUTO),
    p("margin-bottom", AUTO),
    p("margin-inline", AUTO),
    p("margin-inline-end", AUTO),
    p("margin-inline-start", AUTO),
    p("margin-left", AUTO),
    p("margin-right", AUTO),
    p("margin-top", AUTO),
    p("mask", NONE),
    p("mask-image", NONE),
    p("mask-mode", MASK_MODE),
    p("mask-position", POSITION_KEYWORDS),
    p("mask-repeat", BACKGROUND_REPEAT),
    p("mask-size", BACKGROUND_SIZE),
    p("max-block-size", MAX_SIZE),
    p("max-height", MAX_SIZE),
    p("max-inline-size", MAX_SIZE),
    p("max-width", MAX_SIZE),
    p("min-block-size", SIZE),
    p("min-height", SIZE),
    p("min-inline-size", SIZE),
    p("min-width", SIZE),
    p("mix-blend-mode", BLEND_MODE),
    p("object-fit", OBJECT_FIT),
    p("object-position", POSITION_KEYWORDS),
    p("offset", AUTO),
    p("offset-distance", &[]),
    p("offset-path", NONE),
    p("offset-rotate", OFFSET_ROTATE),
    p("opacity", &[]),
    p("order", &[]),
    p("orphans", &[]),
    c("outline", OUTLINE_STYLE),
    c("outline-color", &[]),
    p("outline-offset", &[]),
    p("outline-style", OUTLINE_STYLE),
    p("outline-width", BORDER_WIDTH),
    p("overflow", OVERFLOW),
    p("overflow-anchor", &["auto", "none"]),
    p("overflow-wrap", OVERFLOW_WRAP),
    p("overflow-x", OVERFLOW),
    p("overflow-y", OVERFLOW),
    p("overscroll-behavior", OVERSCROLL),
    p("overscroll-behavior-x", OVERSCROLL),
    p("overscroll-behavior-y", OVERSCROLL),
    p("padding", &[]),
    p("padding-block", &[]),
    p("padding-block-end", &[]),
    p("padding-block-start", &[]),
    p("padding-bottom", &[]),
    p("padding-inline", &[]),
    p("padding-inline-end", &[]),
    p("padding-inline-start", &[]),
    p("padding-left", &[]),
    p("padding-right", &[]),
    p("padding-top", &[]),
    p("page-break-after", PAGE_BREAK),
    p("page-break-before", PAGE_BREAK),
    p("page-break-inside", &["auto", "avoid"]),
    p("perspective", NONE),
    p("perspective-origin", POSITION_KEYWORDS),
    p("place-content", ALIGN_CONTENT),
    p("place-items", ALIGN_ITEMS),
    p("place-self", ALIGN_SELF),
    p("pointer-events", POINTER_EVENTS),
    p("position", POSITION),
    p("position-anchor", AUTO),
    p("position-area", POSITION_AREA),
    p("print-color-adjust", PRINT_COLOR_ADJUST),
    p("quotes", QUOTES),
    p("resize", RESIZE),
    p("right", AUTO),
    p("rotate", NONE),
    p("row-gap", NORMAL),
    p("scale", NONE),
    p("scroll-behavior", SCROLL_BEHAVIOR),
    p("scroll-margin", &[]),
    p("scroll-padding", AUTO),
    p("scroll-snap-align", SCROLL_SNAP_ALIGN),
    p("scroll-snap-stop", SCROLL_SNAP_STOP),
    p("scroll-snap-type", SCROLL_SNAP_TYPE),
    c("scrollbar-color", AUTO),
    p("scrollbar-gutter", SCROLLBAR_GUTTER),
    p("scrollbar-width", SCROLLBAR_WIDTH),
    p("shape-outside", NONE),
    c("stroke", NONE),
    p("stroke-dasharray", NONE),
    p("stroke-dashoffset", &[]),
    p("stroke-linecap", STROKE_LINECAP),
    p("stroke-linejoin", STROKE_LINEJOIN),
    p("stroke-opacity", &[]),
    p("stroke-width", &[]),
    p("tab-size", &[]),
    p("table-layout", TABLE_LAYOUT),
    p("text-align", TEXT_ALIGN),
    p("text-align-last", TEXT_ALIGN_LAST),
    c("text-decoration", TEXT_DECORATION),
    c("text-decoration-color", &[]),
    p("text-decoration-line", TEXT_DECORATION_LINE),
    p("text-decoration-skip-ink", &["auto", "none", "all"]),
    p("text-decoration-style", TEXT_DECORATION_STYLE),
    p("text-decoration-thickness", &["auto", "from-font"]),
    c("text-emphasis", TEXT_EMPHASIS),
    p("text-indent", &["hanging", "each-line"]),
    p("text-orientation", TEXT_ORIENTATION),
    p("text-overflow", TEXT_OVERFLOW),
    p("text-rendering", TEXT_RENDERING),
    c("text-shadow", NONE),
    p("text-size-adjust", TEXT_SIZE_ADJUST),
    p("text-transform", TEXT_TRANSFORM),
    p("text-underline-offset", AUTO),
    p("text-underline-position", TEXT_UNDERLINE_POSITION),
    p("text-wrap", TEXT_WRAP),
    p("top", AUTO),
    p("touch-action", TOUCH_ACTION),
    p("transform", NONE),
    p("transform-box", TRANSFORM_BOX),
    p("transform-origin", POSITION_KEYWORDS),
    p("transform-style", TRANSFORM_STYLE),
    p("transition", TRANSITION),
    p("transition-behavior", TRANSITION_BEHAVIOR),
    p("transition-delay", &[]),
    p("transition-duration", &[]),
    p("transition-property", TRANSITION_PROPERTY),
    p("transition-timing-function", EASING),
    p("translate", NONE),
    p("unicode-bidi", UNICODE_BIDI),
    p("user-select", USER_SELECT),
    p("vertical-align", VERTICAL_ALIGN),
    p("view-transition-name", VIEW_TRANSITION_NAME),
    p("visibility", VISIBILITY),
    p("white-space", WHITE_SPACE),
    p("white-space-collapse", WHITE_SPACE_COLLAPSE),
    p("widows", &[]),
    p("width", SIZE),
    p("will-change", WILL_CHANGE),
    p("word-break", WORD_BREAK),
    p("word-spacing", NORMAL),
    p("writing-mode", WRITING_MODE),
    p("z-index", AUTO),
    p("zoom", ZOOM),
    p("-webkit-font-smoothing", FONT_SMOOTHING),
    p("-webkit-line-clamp", NONE),
    c("-webkit-tap-highlight-color", &[]),
    c("-webkit-text-fill-color", &[]),
];

pub(crate) fn property(name: &str) -> Option<&'static Property> {
    let name = name.to_ascii_lowercase();
    let find = |name: &str| PROPERTIES.iter().find(|property| property.name == name);
    find(&name).or_else(|| {
        ["-webkit-", "-moz-", "-ms-", "-o-"].iter().find_map(|prefix| name.strip_prefix(prefix)).and_then(find)
    })
}

pub(crate) fn functions(property: Option<&Property>) -> Vec<&'static str> {
    let mut functions = vec!["var", "calc", "min", "max", "clamp", "env", "attr"];
    let Some(property) = property else {
        return functions;
    };
    if property.colour {
        functions.extend(COLOUR_FUNCTIONS);
    }
    let name = property.name;
    let extra: &[&str] = match name {
        "transform" => TRANSFORM_FUNCTIONS,
        "filter" | "backdrop-filter" => FILTER_FUNCTIONS,
        "background"
        | "background-image"
        | "list-style-image"
        | "border-image"
        | "border-image-source"
        | "mask"
        | "mask-image"
        | "content"
        | "cursor"
        | "shape-outside" => IMAGE_FUNCTIONS,
        "clip-path" | "offset-path" => &["inset", "circle", "ellipse", "polygon", "path", "url"],
        "content-visibility" => &[],
        "grid-template-columns"
        | "grid-template-rows"
        | "grid-template"
        | "grid"
        | "grid-auto-columns"
        | "grid-auto-rows" => &["repeat", "minmax", "fit-content"],
        "transition" | "transition-timing-function" | "animation" | "animation-timing-function" => {
            &["cubic-bezier", "steps", "linear"]
        }
        "width" | "height" | "min-width" | "min-height" | "max-width" | "max-height" | "flex-basis" | "inline-size"
        | "block-size" => &["fit-content"],
        "counter-reset" | "counter-increment" => &[],
        "position-anchor" | "top" | "right" | "bottom" | "left" | "inset" => &["anchor", "anchor-size"],
        _ => &[],
    };
    functions.extend(extra);
    if name == "content" {
        functions.extend(["counter", "counters"]);
    }
    functions
}

const COLOUR_FUNCTIONS: &[&str] =
    &["rgb", "rgba", "hsl", "hsla", "hwb", "lab", "lch", "oklab", "oklch", "color", "color-mix", "light-dark"];
const IMAGE_FUNCTIONS: &[&str] = &[
    "url",
    "linear-gradient",
    "radial-gradient",
    "conic-gradient",
    "repeating-linear-gradient",
    "repeating-radial-gradient",
    "repeating-conic-gradient",
    "image-set",
    "cross-fade",
];
const TRANSFORM_FUNCTIONS: &[&str] = &[
    "translate",
    "translateX",
    "translateY",
    "translateZ",
    "translate3d",
    "scale",
    "scaleX",
    "scaleY",
    "scaleZ",
    "scale3d",
    "rotate",
    "rotateX",
    "rotateY",
    "rotateZ",
    "rotate3d",
    "skew",
    "skewX",
    "skewY",
    "matrix",
    "matrix3d",
    "perspective",
];
const FILTER_FUNCTIONS: &[&str] = &[
    "blur",
    "brightness",
    "contrast",
    "drop-shadow",
    "grayscale",
    "hue-rotate",
    "invert",
    "opacity",
    "saturate",
    "sepia",
    "url",
];

pub(crate) const COLOURS: &[&str] = &[
    "aliceblue",
    "antiquewhite",
    "aqua",
    "aquamarine",
    "azure",
    "beige",
    "bisque",
    "black",
    "blanchedalmond",
    "blue",
    "blueviolet",
    "brown",
    "burlywood",
    "cadetblue",
    "chartreuse",
    "chocolate",
    "coral",
    "cornflowerblue",
    "cornsilk",
    "crimson",
    "currentcolor",
    "cyan",
    "darkblue",
    "darkcyan",
    "darkgoldenrod",
    "darkgray",
    "darkgreen",
    "darkgrey",
    "darkkhaki",
    "darkmagenta",
    "darkolivegreen",
    "darkorange",
    "darkorchid",
    "darkred",
    "darksalmon",
    "darkseagreen",
    "darkslateblue",
    "darkslategray",
    "darkslategrey",
    "darkturquoise",
    "darkviolet",
    "deeppink",
    "deepskyblue",
    "dimgray",
    "dimgrey",
    "dodgerblue",
    "firebrick",
    "floralwhite",
    "forestgreen",
    "fuchsia",
    "gainsboro",
    "ghostwhite",
    "gold",
    "goldenrod",
    "gray",
    "green",
    "greenyellow",
    "grey",
    "honeydew",
    "hotpink",
    "indianred",
    "indigo",
    "ivory",
    "khaki",
    "lavender",
    "lavenderblush",
    "lawngreen",
    "lemonchiffon",
    "lightblue",
    "lightcoral",
    "lightcyan",
    "lightgoldenrodyellow",
    "lightgray",
    "lightgreen",
    "lightgrey",
    "lightpink",
    "lightsalmon",
    "lightseagreen",
    "lightskyblue",
    "lightslategray",
    "lightslategrey",
    "lightsteelblue",
    "lightyellow",
    "lime",
    "limegreen",
    "linen",
    "magenta",
    "maroon",
    "mediumaquamarine",
    "mediumblue",
    "mediumorchid",
    "mediumpurple",
    "mediumseagreen",
    "mediumslateblue",
    "mediumspringgreen",
    "mediumturquoise",
    "mediumvioletred",
    "midnightblue",
    "mintcream",
    "mistyrose",
    "moccasin",
    "navajowhite",
    "navy",
    "oldlace",
    "olive",
    "olivedrab",
    "orange",
    "orangered",
    "orchid",
    "palegoldenrod",
    "palegreen",
    "paleturquoise",
    "palevioletred",
    "papayawhip",
    "peachpuff",
    "peru",
    "pink",
    "plum",
    "powderblue",
    "purple",
    "rebeccapurple",
    "red",
    "rosybrown",
    "royalblue",
    "saddlebrown",
    "salmon",
    "sandybrown",
    "seagreen",
    "seashell",
    "sienna",
    "silver",
    "skyblue",
    "slateblue",
    "slategray",
    "slategrey",
    "snow",
    "springgreen",
    "steelblue",
    "tan",
    "teal",
    "thistle",
    "tomato",
    "transparent",
    "turquoise",
    "violet",
    "wheat",
    "white",
    "whitesmoke",
    "yellow",
    "yellowgreen",
];

pub(crate) fn is_colour_name(word: &str) -> bool {
    let mut buffer = [0u8; 20];
    let Some(lower) = buffer.get_mut(..word.len()) else {
        return false;
    };
    lower.copy_from_slice(word.as_bytes());
    lower.make_ascii_lowercase();
    COLOURS.binary_search_by(|colour| colour.as_bytes().cmp(lower)).is_ok()
}

pub(crate) const UNITS: &[&str] = &[
    "px", "%", "em", "rem", "vw", "vh", "vmin", "vmax", "svh", "lvh", "dvh", "svw", "lvw", "dvw", "vi", "vb", "ch",
    "ex", "cap", "ic", "lh", "rlh", "cqw", "cqh", "cqi", "cqb", "cqmin", "cqmax", "fr", "cm", "mm", "Q", "in", "pt",
    "pc", "deg", "rad", "grad", "turn", "s", "ms", "Hz", "kHz", "dpi", "dpcm", "dppx", "x",
];

pub(crate) const AT_RULES: &[(&str, &str)] = &[
    ("charset", " \"$0\";"),
    ("container", " $0"),
    ("counter-style", " $0"),
    ("font-face", " {\n\t$0\n}"),
    ("font-feature-values", " $0"),
    ("font-palette-values", " --$0"),
    ("import", " \"$0\";"),
    ("keyframes", " $0"),
    ("layer", " $0"),
    ("media", " $0"),
    ("namespace", " $0;"),
    ("page", " $0"),
    ("position-try", " --$0"),
    ("property", " --$0"),
    ("scope", " ($0)"),
    ("starting-style", " {\n\t$0\n}"),
    ("supports", " ($0)"),
    ("view-transition", " {\n\t$0\n}"),
];

pub(crate) const SCSS_AT_RULES: &[(&str, &str)] = &[
    ("at-root", " $0"),
    ("content", ";"),
    ("debug", " $0;"),
    ("each", " $0 in "),
    ("else", " {\n\t$0\n}"),
    ("else if", " $0"),
    ("error", " $0;"),
    ("extend", " $0;"),
    ("for", " $0 from  through "),
    ("forward", " \"$0\";"),
    ("function", " $0()"),
    ("if", " $0"),
    ("include", " $0;"),
    ("mixin", " $0"),
    ("return", " $0;"),
    ("use", " \"$0\";"),
    ("warn", " $0;"),
    ("while", " $0"),
];

pub(crate) const PSEUDO_CLASSES: &[&str] = &[
    "active",
    "any-link",
    "autofill",
    "checked",
    "default",
    "defined",
    "dir()",
    "disabled",
    "empty",
    "enabled",
    "first",
    "first-child",
    "first-of-type",
    "focus",
    "focus-visible",
    "focus-within",
    "fullscreen",
    "has()",
    "host",
    "host()",
    "host-context()",
    "hover",
    "in-range",
    "indeterminate",
    "invalid",
    "is()",
    "lang()",
    "last-child",
    "last-of-type",
    "left",
    "link",
    "modal",
    "not()",
    "nth-child()",
    "nth-last-child()",
    "nth-last-of-type()",
    "nth-of-type()",
    "only-child",
    "only-of-type",
    "open",
    "optional",
    "out-of-range",
    "paused",
    "picture-in-picture",
    "placeholder-shown",
    "playing",
    "popover-open",
    "read-only",
    "read-write",
    "required",
    "right",
    "root",
    "scope",
    "state()",
    "target",
    "user-invalid",
    "user-valid",
    "valid",
    "visited",
    "where()",
];

pub(crate) const PSEUDO_ELEMENTS: &[&str] = &[
    "after",
    "backdrop",
    "before",
    "cue",
    "details-content",
    "file-selector-button",
    "first-letter",
    "first-line",
    "grammar-error",
    "highlight()",
    "marker",
    "part()",
    "placeholder",
    "selection",
    "slotted()",
    "spelling-error",
    "target-text",
    "view-transition",
    "view-transition-group()",
    "view-transition-image-pair()",
    "view-transition-new()",
    "view-transition-old()",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_are_clean() {
        assert!(PROPERTIES.windows(2).all(|pair| pair[0].name < pair[1].name || pair[1].name.starts_with('-')));
        for list in [COLOURS, UNITS, PSEUDO_CLASSES, PSEUDO_ELEMENTS] {
            let mut sorted = list.to_vec();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), list.len());
        }
        assert_eq!(COLOURS.len(), 150);
        assert!(COLOURS.windows(2).all(|pair| pair[0] < pair[1]));
        for property in PROPERTIES {
            let mut values = property.values.to_vec();
            values.sort_unstable();
            values.dedup();
            assert_eq!(values.len(), property.values.len(), "{}", property.name);
        }
    }

    #[test]
    fn finds_properties() {
        assert_eq!(property("Display").map(|p| p.name), Some("display"));
        assert_eq!(property("-webkit-transform").map(|p| p.name), Some("transform"));
        assert_eq!(property("-webkit-font-smoothing").map(|p| p.name), Some("-webkit-font-smoothing"));
        assert!(property("colour").is_none());
        assert!(is_colour_name("CurrentColor") && !is_colour_name("block"));
        assert!(functions(property("color")).contains(&"oklch"));
        assert!(functions(property("transform")).contains(&"rotate"));
        assert!(!functions(property("display")).contains(&"rgb"));
    }
}
