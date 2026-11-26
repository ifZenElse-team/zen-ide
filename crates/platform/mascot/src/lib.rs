pub const SIZE: usize = 40;

pub const FRAMES: usize = 8;

pub const FRAME_MS: u64 = 160;

pub const STILL: usize = 0;

const CX: i32 = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ink {
    Clear,
    Outline,
    Skin,
    SkinShade,
    Shine,
    White,
    Obi,
    Cheek,
    Eye,
    Snore,
    Alarm,
    Sparkle,
    Metal,
    MetalShade,
    Wood,
    WoodShade,
    Leaf,
    LeafShade,
    Petal,
    PetalShade,
    Red,
    DarkRed,
    Gold,
    GoldShade,
    Straw,
    StrawShade,
    Water,
    DeepWater,
    Iron,
    Indigo,
    Steam,
    Robe,
    RobeShade,
}

impl Ink {
    pub fn from_letter(letter: char) -> Ink {
        match letter {
            'o' => Ink::Outline,
            's' => Ink::Skin,
            'S' => Ink::SkinShade,
            'h' => Ink::Shine,
            'w' => Ink::White,
            'b' => Ink::Obi,
            'c' => Ink::Cheek,
            'e' => Ink::Eye,
            'z' => Ink::Snore,
            '!' => Ink::Alarm,
            '*' => Ink::Sparkle,
            'g' => Ink::Metal,
            'G' => Ink::MetalShade,
            'd' => Ink::Wood,
            'D' => Ink::WoodShade,
            'l' => Ink::Leaf,
            'L' => Ink::LeafShade,
            'p' => Ink::Petal,
            'P' => Ink::PetalShade,
            'x' => Ink::Red,
            'X' => Ink::DarkRed,
            'y' => Ink::Gold,
            'Y' => Ink::GoldShade,
            't' => Ink::Straw,
            'T' => Ink::StrawShade,
            'u' => Ink::Water,
            'U' => Ink::DeepWater,
            'k' => Ink::Iron,
            'n' => Ink::Indigo,
            'v' => Ink::Steam,
            'r' => Ink::Robe,
            'R' => Ink::RobeShade,
            _ => Ink::Clear,
        }
    }

    pub fn letter(self) -> char {
        match self {
            Ink::Clear => '.',
            Ink::Outline => 'o',
            Ink::Skin => 's',
            Ink::SkinShade => 'S',
            Ink::Shine => 'h',
            Ink::White => 'w',
            Ink::Obi => 'b',
            Ink::Cheek => 'c',
            Ink::Eye => 'e',
            Ink::Snore => 'z',
            Ink::Alarm => '!',
            Ink::Sparkle => '*',
            Ink::Metal => 'g',
            Ink::MetalShade => 'G',
            Ink::Wood => 'd',
            Ink::WoodShade => 'D',
            Ink::Leaf => 'l',
            Ink::LeafShade => 'L',
            Ink::Petal => 'p',
            Ink::PetalShade => 'P',
            Ink::Red => 'x',
            Ink::DarkRed => 'X',
            Ink::Gold => 'y',
            Ink::GoldShade => 'Y',
            Ink::Straw => 't',
            Ink::StrawShade => 'T',
            Ink::Water => 'u',
            Ink::DeepWater => 'U',
            Ink::Iron => 'k',
            Ink::Indigo => 'n',
            Ink::Steam => 'v',
            Ink::Robe => 'r',
            Ink::RobeShade => 'R',
        }
    }

    fn floats(self) -> bool {
        matches!(self, Ink::Clear | Ink::Outline | Ink::Snore | Ink::Sparkle | Ink::Alarm)
    }

    pub fn rgb(self, variant: Variant) -> Option<[u8; 3]> {
        let (robe, shade) = variant.robe();
        Some(match self {
            Ink::Clear => return None,
            Ink::Outline | Ink::Eye => [0x2A, 0x24, 0x40],
            Ink::Skin => [0xF6, 0xD2, 0xB0],
            Ink::SkinShade => [0xE3, 0xAE, 0x8A],
            Ink::Shine => [0xFF, 0xEC, 0xDA],
            Ink::White => [0xF4, 0xF1, 0xEA],
            Ink::Obi => [0x2F, 0x27, 0x4D],
            Ink::Cheek => [0xF2, 0x9C, 0x9C],
            Ink::Snore => [0x9A, 0x90, 0xC8],
            Ink::Alarm => [0xE5, 0x48, 0x4D],
            Ink::Sparkle => [0xFF, 0xD1, 0x5C],
            Ink::Metal => [0x8E, 0x95, 0xA3],
            Ink::MetalShade => [0x5D, 0x63, 0x70],
            Ink::Wood => [0xB0, 0x7A, 0x4A],
            Ink::WoodShade => [0x7A, 0x4E, 0x2C],
            Ink::Leaf => [0x6C, 0xC2, 0x5A],
            Ink::LeafShade => [0x3A, 0x8A, 0x3C],
            Ink::Petal => [0xFF, 0xB8, 0xD0],
            Ink::PetalShade => [0xD9, 0x4F, 0x8A],
            Ink::Red => [0xD6, 0x3A, 0x3A],
            Ink::DarkRed => [0x9E, 0x24, 0x2A],
            Ink::Gold => [0xF2, 0xC2, 0x4E],
            Ink::GoldShade => [0xC4, 0x8E, 0x22],
            Ink::Straw => [0xD8, 0xC9, 0x8A],
            Ink::StrawShade => [0xA8, 0x95, 0x4E],
            Ink::Water => [0x5A, 0xA8, 0xF0],
            Ink::DeepWater => [0x25, 0x5F, 0xDB],
            Ink::Iron => [0x3B, 0x3F, 0x4A],
            Ink::Indigo => [0x6F, 0x5C, 0xD8],
            Ink::Steam => [0xC9, 0xC6, 0xD6],
            Ink::Robe => robe,
            Ink::RobeShade => shade,
        })
    }
}

pub type Grid = [[Ink; SIZE]; SIZE];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Variant {
    Zen,
    GoGarden,
    RustRonin,
    PyPetal,
    JvmJizo,
    WebWave,
    CppCrane,
    SharpShuriken,
    PhpPagoda,
    RubyRyokan,
    SqlScroll,
    DevopsDaruma,
    TestTotami,
    Toolbox,
}

impl Variant {
    pub const ALL: [Variant; 14] = [
        Variant::Zen,
        Variant::GoGarden,
        Variant::RustRonin,
        Variant::PyPetal,
        Variant::JvmJizo,
        Variant::WebWave,
        Variant::CppCrane,
        Variant::SharpShuriken,
        Variant::PhpPagoda,
        Variant::RubyRyokan,
        Variant::SqlScroll,
        Variant::DevopsDaruma,
        Variant::TestTotami,
        Variant::Toolbox,
    ];

    pub fn of(id: &str) -> Variant {
        Variant::ALL.into_iter().find(|variant| variant.id() == id).unwrap_or(Variant::Zen)
    }

    pub fn id(self) -> &'static str {
        match self {
            Variant::Zen => "zen",
            Variant::GoGarden => "gogarden",
            Variant::RustRonin => "rustronin",
            Variant::PyPetal => "pypetal",
            Variant::JvmJizo => "jvmjizo",
            Variant::WebWave => "webwave",
            Variant::CppCrane => "cppcrane",
            Variant::SharpShuriken => "sharpshuriken",
            Variant::PhpPagoda => "phppagoda",
            Variant::RubyRyokan => "rubyryokan",
            Variant::SqlScroll => "sqlscroll",
            Variant::DevopsDaruma => "devopsdaruma",
            Variant::TestTotami => "testtotami",
            Variant::Toolbox => "toolbox",
        }
    }

    pub fn robe(self) -> ([u8; 3], [u8; 3]) {
        match self {
            Variant::Zen | Variant::Toolbox => ([0x8B, 0x7C, 0xF8], [0x43, 0x38, 0xCA]),
            Variant::GoGarden => ([0x5E, 0xE7, 0xD0], [0x0E, 0x8F, 0xB0]),
            Variant::RustRonin => ([0xFF, 0x9A, 0x5A], [0xD9, 0x3A, 0x2B]),
            Variant::PyPetal => ([0xFF, 0xB8, 0xD0], [0xD9, 0x4F, 0x8A]),
            Variant::JvmJizo => ([0xB8, 0xC4, 0xA6], [0x5E, 0x6E, 0x58]),
            Variant::WebWave => ([0x6F, 0xB6, 0xFF], [0x25, 0x5F, 0xDB]),
            Variant::CppCrane => ([0x7E, 0xE0, 0x9A], [0x1F, 0x8A, 0x55]),
            Variant::SharpShuriken => ([0xA5, 0x9C, 0xFF], [0x48, 0x3B, 0xC9]),
            Variant::PhpPagoda => ([0x9F, 0xB4, 0xF5], [0x52, 0x60, 0xB8]),
            Variant::RubyRyokan => ([0xFF, 0x7A, 0x8A], [0xC0, 0x1E, 0x3C]),
            Variant::SqlScroll => ([0x6F, 0xE0, 0xC8], [0x1C, 0x7F, 0x86]),
            Variant::DevopsDaruma => ([0xFF, 0x9E, 0x8E], [0xC7, 0x3B, 0x3B]),
            Variant::TestTotami => ([0xE6, 0xD3, 0x8C], [0x9A, 0x86, 0x2E]),
        }
    }

    fn prop(self) -> Option<(&'static [&'static str], i32, i32)> {
        Some(match self {
            Variant::Zen => return None,
            Variant::GoGarden => (CAN, 29, 30),
            Variant::RustRonin => (BOKKEN, 6, 36),
            Variant::PyPetal => (BLOSSOM, 22, 3),
            Variant::JvmJizo => (JIZO, 29, 27),
            Variant::WebWave => (BOARD, 31, 16),
            Variant::CppCrane => (CRANE, 17, 1),
            Variant::SharpShuriken => (SHURIKEN, 31, 33),
            Variant::PhpPagoda => (PAGODA, 30, 28),
            Variant::RubyRyokan => (LANTERN, 31, 29),
            Variant::SqlScroll => (SCROLL, 15, 32),
            Variant::DevopsDaruma => (DARUMA, 30, 29),
            Variant::TestTotami => (TATAMI, 3, 36),
            Variant::Toolbox => (TOOLBOX, 29, 31),
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Idle,
    Work,
    Success,
    Error,
    Sleep,
}

impl State {
    pub const ALL: [State; 5] = [State::Idle, State::Work, State::Success, State::Error, State::Sleep];

    pub fn name(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Work => "work",
            State::Success => "success",
            State::Error => "error",
            State::Sleep => "sleep",
        }
    }
}

pub fn frame_at(ms: u64) -> usize {
    (ms / FRAME_MS % FRAMES as u64) as usize
}

pub fn frame(variant: Variant, state: State, t: usize) -> Grid {
    let t = t % FRAMES;
    match state {
        State::Idle | State::Work => sitting(variant, t, state == State::Idle),
        State::Success => success(t),
        State::Error => error(t),
        State::Sleep => sleep(t),
    }
}

#[rustfmt::skip]
const CAN: &[&str] = &[
    "......gg..",
    ".....g..g.",
    "gg..gggggg",
    ".gggggggGG",
    "..ggggggGG",
    "..ggggggGG",
    "..gGGGGGGG",
];

#[rustfmt::skip]
const BOKKEN: &[&str] = &[
    "..........................kk",
    "dddddddddddddddddddddddyykkD",
    "DDDDDDDDDDDDDDDDDDDDDDDYYkkD",
];

#[rustfmt::skip]
const BLOSSOM: &[&str] = &[
    ".p.p.",
    "ppPpp",
    ".pyp.",
    "ppPpp",
    ".p.p.",
];

#[rustfmt::skip]
const JIZO: &[&str] = &[
    "...gggg...",
    "..gggggg..",
    "..gGggGg..",
    "..gggggg..",
    "...gggg...",
    ".gxxxxxxg.",
    ".gxxxxxxg.",
    ".ggXxxXgg.",
    ".gggggggg.",
    "GGGGGGGGGG",
];

#[rustfmt::skip]
const MAPLE: &[&str] = &[
    "..x..",
    "xxxxx",
    ".xXx.",
    "..X..",
];

#[rustfmt::skip]
const BOARD: &[&str] = &[
    "..ww..",
    ".wwww.",
    "wwuuww",
    "wwuuww",
    "wwuuww",
    "wwuuww",
    "wwuuww",
    "wwuuww",
    "wwuuww",
    "wwuuww",
    "wwuuww",
    "wwuuww",
    "wwuuww",
    "wwuuww",
    "wwuuww",
    ".wwww.",
    "..ww..",
];

#[rustfmt::skip]
const CRANE: &[&str] = &[
    "w.......",
    "ww....w.",
    ".www.ww.",
    "..wwwww.",
    "...wwwww",
    "....www.",
];

#[rustfmt::skip]
const CRANE_UP: &[&str] = &[
    "w..w....",
    "ww.ww.w.",
    ".wwww.ww",
    "..wwwww.",
    "...wwwww",
    "....www.",
];

#[rustfmt::skip]
const SHURIKEN: &[&str] = &[
    "...g...",
    "...gg..",
    "gGGkggg",
    "..ggG..",
    "..g.g..",
];

#[rustfmt::skip]
const SHURIKEN_TURNED: &[&str] = &[
    "..g.g..",
    "..ggG..",
    "gGGkggg",
    "..gg...",
    "...g...",
];

#[rustfmt::skip]
const PAGODA: &[&str] = &[
    "....y....",
    "..xxxxx..",
    "XXXXXXXXX",
    "...ddd...",
    ".xxxxxxx.",
    "XXXXXXXXX",
    "..ddDdd..",
    "..ddDdd..",
];

#[rustfmt::skip]
const LANTERN: &[&str] = &[
    "..kkk..",
    ".xxxxx.",
    "xxwwwxx",
    "xxxxxxx",
    "xxwwwxx",
    ".xxxxx.",
    "..kkk..",
];

#[rustfmt::skip]
const SCROLL: &[&str] = &[
    "dwwwwwwwwwd",
    "Dwgwgwgwg.D",
    "dwwwwwwwwwd",
];

#[rustfmt::skip]
const DARUMA: &[&str] = &[
    "..xxxxx..",
    ".xwwwwwx.",
    "xxwkwkwxx",
    "xxwwwwwxx",
    "xxxyyyxxx",
    "xxxxxxxxx",
    ".xxxxxxx.",
    "..XXXXX..",
];

#[rustfmt::skip]
const TATAMI: &[&str] = &[
    "kTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTk",
    "kttttttttttttttttttttttttttttttttk",
    "kTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTTk",
];

#[rustfmt::skip]
const TOOLBOX: &[&str] = &[
    "...gggg...",
    "...g..g...",
    "nnnnnnnnnn",
    "nnnnyynnnn",
    "nnnnnnnnnn",
    "nnnnnnnnnn",
];

#[rustfmt::skip]
const SMALL_Z: &[&str] = &["zz", ".z", "zz"];

#[rustfmt::skip]
const BIG_Z: &[&str] = &["zzz", "..z", ".z.", "z..", "zzz"];

fn blank() -> Grid {
    [[Ink::Clear; SIZE]; SIZE]
}

fn set(g: &mut Grid, x: i32, y: i32, ink: Ink) {
    if (0..SIZE as i32).contains(&x) && (0..SIZE as i32).contains(&y) {
        g[y as usize][x as usize] = ink;
    }
}

fn ellipse(g: &mut Grid, cx: f64, cy: f64, rx: f64, ry: f64, ink: Ink) {
    for (y, row) in g.iter_mut().enumerate() {
        for (x, pixel) in row.iter_mut().enumerate() {
            let dx = (x as f64 + 0.5 - cx) / rx;
            let dy = (y as f64 + 0.5 - cy) / ry;
            if dx * dx + dy * dy <= 1.0 {
                *pixel = ink;
            }
        }
    }
}

fn rect(g: &mut Grid, x0: i32, y0: i32, x1: i32, y1: i32, ink: Ink) {
    for y in y0..=y1 {
        for x in x0..=x1 {
            set(g, x, y, ink);
        }
    }
}

fn put(g: &mut Grid, points: &[(i32, i32)], ink: Ink) {
    for &(x, y) in points {
        set(g, x, y, ink);
    }
}

fn stamp<S: AsRef<str>>(g: &mut Grid, art: &[S], x0: i32, y0: i32) {
    for (dy, row) in art.iter().enumerate() {
        for (dx, letter) in row.as_ref().chars().enumerate() {
            if letter != '.' {
                set(g, x0 + dx as i32, y0 + dy as i32, Ink::from_letter(letter));
            }
        }
    }
}

pub fn outline(g: &Grid) -> Grid {
    let mut out = *g;
    let solid = |x: i32, y: i32| {
        (0..SIZE as i32).contains(&x) && (0..SIZE as i32).contains(&y) && !g[y as usize][x as usize].floats()
    };
    for y in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            if g[y as usize][x as usize] == Ink::Clear
                && [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| solid(x + dx, y + dy))
            {
                out[y as usize][x as usize] = Ink::Outline;
            }
        }
    }
    out
}

fn head(g: &mut Grid, cx: i32, top: i32, tilt: i32) -> f64 {
    let cy = f64::from(top) + 7.8;
    let (x, t) = (f64::from(cx), f64::from(tilt));
    ellipse(g, x - 7.6 + t, cy + 1.8, 1.3, 1.6, Ink::Skin);
    ellipse(g, x + 7.4 + t, cy + 1.8, 1.3, 1.6, Ink::Skin);
    ellipse(g, x + t, cy, 7.7, 7.0, Ink::Skin);
    for (row, pixels) in g.iter_mut().enumerate() {
        for (column, pixel) in pixels.iter_mut().enumerate() {
            let dx = (column as f64 + 0.5 - x - t + 1.6) / 7.7;
            let dy = (row as f64 + 0.5 - cy + 1.6) / 7.0;
            if *pixel == Ink::Skin && dx * dx + dy * dy > 1.0 {
                *pixel = Ink::SkinShade;
            }
        }
    }
    let shine = cx - 4 + tilt;
    put(
        g,
        &[(shine, top + 2), (shine + 1, top + 2), (shine + 2, top + 2), (shine - 1, top + 3), (shine, top + 3)],
        Ink::Shine,
    );
    cy
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Eyes {
    Closed,
    Open,
    Happy,
    Dizzy,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mouth {
    Smile,
    O,
    Flat,
}

fn face(g: &mut Grid, cx: i32, cy: f64, eyes: Eyes, mouth: Mouth) {
    let (ex, ey) = (cx, (cy + 2.0) as i32);
    let (left, right) = (ex - 5, ex + 2);
    for bx in [left, right] {
        let points: &[(i32, i32)] = match eyes {
            Eyes::Closed => &[(bx, ey), (bx + 1, ey + 1), (bx + 2, ey + 1), (bx + 3, ey)],
            Eyes::Open => &[(bx + 1, ey - 1), (bx + 2, ey - 1), (bx + 1, ey), (bx + 2, ey)],
            Eyes::Happy => &[(bx, ey + 1), (bx + 1, ey), (bx + 2, ey), (bx + 3, ey + 1)],
            Eyes::Dizzy => {
                &[(bx, ey - 1), (bx + 3, ey - 1), (bx + 1, ey), (bx + 2, ey), (bx, ey + 1), (bx + 3, ey + 1)]
            }
        };
        put(g, points, Ink::Eye);
    }
    put(g, &[(left - 1, ey + 2), (left, ey + 2), (right + 3, ey + 2), (right + 4, ey + 2)], Ink::Cheek);
    let my = ey + 3;
    let points: &[(i32, i32)] = match mouth {
        Mouth::Smile => &[(ex - 1, my), (ex, my + 1), (ex + 1, my)],
        Mouth::O => &[(ex - 1, my), (ex, my), (ex - 1, my + 1), (ex, my + 1)],
        Mouth::Flat => &[(ex - 1, my + 1), (ex, my + 1), (ex + 1, my + 1)],
    };
    put(g, points, Ink::Eye);
}

fn lotus_body(g: &mut Grid, neck: i32) {
    let cx = CX;
    let x = f64::from(cx);
    let base = neck + 13;
    let b = f64::from(base);
    ellipse(g, x - 8.5, b - 1.0, 7.5, 2.8, Ink::Robe);
    ellipse(g, x + 8.5, b - 1.0, 7.5, 2.8, Ink::RobeShade);
    for y in neck..base - 2 {
        let half = 5.5 + f64::from(y - neck) * 0.35;
        for (column, pixel) in g[y as usize].iter_mut().enumerate() {
            if (column as f64 + 0.5 - x).abs() <= half {
                *pixel = Ink::Robe;
            }
        }
    }
    let n = f64::from(neck);
    ellipse(g, x - 7.0, n + 6.0, 3.3, 4.6, Ink::Robe);
    ellipse(g, x + 7.0, n + 6.0, 3.3, 4.6, Ink::RobeShade);
    rect(g, cx - 4, neck, cx + 3, neck, Ink::White);
    put(g, &[(cx - 3, neck + 1), (cx - 2, neck + 2), (cx - 1, neck + 3), (cx, neck + 4)], Ink::White);
    put(g, &[(cx + 2, neck + 1), (cx + 1, neck + 2)], Ink::White);
    put(g, &[(cx - 1, neck + 1), (cx, neck + 1), (cx + 1, neck + 1), (cx, neck + 2)], Ink::Skin);
    rect(g, cx - 6, neck + 5, cx + 5, neck + 5, Ink::Obi);
    ellipse(g, x - 6.0, b - 3.0, 2.6, 1.3, Ink::Skin);
    ellipse(g, x + 6.0, b - 3.0, 2.6, 1.3, Ink::Skin);
    put(g, &[(cx - 7, base - 3), (cx - 6, base - 3), (cx + 5, base - 3), (cx + 6, base - 3)], Ink::SkinShade);
    ellipse(g, x, b - 4.5, 4.0, 1.7, Ink::Skin);
    put(g, &[(cx - 3, base - 5), (cx + 2, base - 5)], Ink::SkinShade);
    put(g, &[(cx - 1, base - 6), (cx, base - 6)], Ink::SkinShade);
    for (row, pixels) in g.iter_mut().enumerate() {
        let y = row as i32;
        for (column, pixel) in pixels.iter_mut().enumerate() {
            if *pixel == Ink::Robe && column as f64 > f64::from(cx + 2) + f64::from(y - neck) * 0.4 && y < base - 2 {
                *pixel = Ink::RobeShade;
            }
        }
    }
}

fn prop(g: &mut Grid, variant: Variant, t: i32) {
    let Some((art, x, y)) = variant.prop() else { return };
    match variant {
        Variant::GoGarden => {
            stamp(g, art, x, y);
            for i in 0..3 {
                let dy = (t + i * 3) % 6;
                set(g, x - 1, y + 2 + dy, Ink::Water);
            }
            for h in 0..t.min(5) {
                set(g, x - 4, 38 - h, Ink::LeafShade);
            }
            if t >= 4 {
                put(g, &[(x - 5, 34), (x - 3, 35)], Ink::Leaf);
            }
        }
        Variant::RustRonin => {
            stamp(g, art, x, y);
            let gx = x + 2 + t * 3;
            put(g, &[(gx, y + 1), (gx + 1, y + 1)], Ink::White);
        }
        Variant::PyPetal => {
            stamp(g, art, x, y);
            let px = x + 3 + i32::from(t % 4 == 3) + t / 2;
            let py = y + 6 + t * 4;
            put(g, &[(px, py), (px + 1, py)], Ink::Petal);
        }
        Variant::JvmJizo => {
            stamp(g, art, x, y);
            let sway = [0, 1, 1, 0, -1, -1, 0, 1][t as usize];
            stamp(g, MAPLE, x + 3 + sway, y - 20 + t * 2 + t / 2);
        }
        Variant::WebWave => {
            let bob = i32::from(matches!(t % 4, 1 | 2));
            stamp(g, art, x, y + bob);
        }
        Variant::CppCrane => {
            stamp(g, if t % 4 < 2 { CRANE_UP } else { art }, x, y);
        }
        Variant::SharpShuriken => {
            stamp(g, if t % 2 == 0 { art } else { SHURIKEN_TURNED }, x, y);
        }
        Variant::PhpPagoda => {
            stamp(g, art, x, y);
            if t % 4 == 0 {
                put(g, &[(x + 4, y - 1), (x + 3, y), (x + 5, y)], Ink::Sparkle);
            }
        }
        Variant::RubyRyokan => {
            let sway = [0, 1, 1, 0, 0, -1, -1, 0][t as usize];
            if t % 3 == 0 {
                let lit: Vec<String> = art.iter().map(|row| row.replace('w', "y")).collect();
                stamp(g, &lit, x + sway, y);
            } else {
                stamp(g, art, x + sway, y);
            }
        }
        Variant::SqlScroll => {
            let width = [5, 7, 9, 11, 11, 9, 7, 5][t as usize];
            let rows: Vec<String> =
                art.iter().map(|row| format!("{}{}", &row[..width - 1], &row[row.len() - 1..])).collect();
            stamp(g, &rows, CX - width as i32 / 2, y);
        }
        Variant::DevopsDaruma => {
            let sway: i32 = [0, 1, 2, 1, 0, -1, -2, -1][t as usize];
            stamp(g, art, x + sway, y + i32::from(sway.abs() == 2));
        }
        Variant::TestTotami => {
            let laid = (6 + t * 4) as usize;
            let rows: Vec<&str> = art.iter().map(|row| &row[..laid.min(row.len())]).collect();
            stamp(g, &rows, x, y);
        }
        Variant::Toolbox => {
            stamp(g, art, x, y);
            if (2..=5).contains(&t) {
                put(g, &[(x + 4, y - 3 - (t - 2)), (x + 5, y - 3 - (t - 2))], Ink::Sparkle);
            }
        }
        Variant::Zen => {}
    }
}

fn sitting(variant: Variant, t: usize, idle: bool) -> Grid {
    let mut g = blank();
    let t = t as i32;
    let breath = i32::from((2..=5).contains(&t));
    let behind = variant == Variant::WebWave;
    if behind {
        prop(&mut g, variant, t);
    }
    lotus_body(&mut g, 21 + breath);
    let cy = head(&mut g, CX, 5 + breath, 0);
    let eyes = if idle || t == 6 { Eyes::Closed } else { Eyes::Open };
    face(&mut g, CX, cy, eyes, Mouth::Smile);
    if !behind {
        prop(&mut g, variant, t);
    }
    let mut g = outline(&g);
    if variant == Variant::WebWave {
        for i in 0..SIZE as i32 {
            let phase = (i - t * 2).rem_euclid(10);
            if phase < 4 {
                set(&mut g, i, 39, Ink::Water);
                if phase == 1 {
                    set(&mut g, i, 38, Ink::White);
                }
            }
        }
    }
    g
}

fn sleep(t: usize) -> Grid {
    let mut g = blank();
    let t = t as i32;
    let breath = i32::from((2..=5).contains(&t));
    lotus_body(&mut g, 22 + breath);
    let cy = head(&mut g, CX, 7 + breath, 1);
    face(&mut g, CX + 1, cy, Eyes::Closed, if (3..=5).contains(&t) { Mouth::O } else { Mouth::Flat });
    let mut g = outline(&g);
    for (i, art) in [SMALL_Z, BIG_Z, SMALL_Z].into_iter().enumerate() {
        let step = (t + i as i32 * 3) % 9;
        if step > 6 {
            continue;
        }
        let (x, y) = (29 + step / 2 + step % 2, 14 - step * 2);
        stamp(&mut g, art, x, y);
    }
    g
}

fn sparkle(g: &mut Grid, x: i32, y: i32, big: bool) {
    if big {
        put(g, &[(x, y - 1), (x - 1, y), (x, y), (x + 1, y), (x, y + 1)], Ink::Sparkle);
    } else {
        set(g, x, y, Ink::Sparkle);
    }
}

fn success(t: usize) -> Grid {
    let mut g = blank();
    let lift = [0, 1, 2, 1, 0, 0, 1, 0][t];
    lotus_body(&mut g, 21 - lift);
    let cy = head(&mut g, CX, 5 - lift, 0);
    face(&mut g, CX, cy, Eyes::Happy, Mouth::Smile);
    let mut g = outline(&g);
    let spots = [(6, 9), (33, 7), (4, 22), (35, 19), (9, 3), (30, 2)];
    for (i, (x, y)) in spots.into_iter().enumerate() {
        let phase = (t + i * 3) % 6;
        if phase < 3 {
            sparkle(&mut g, x, y, phase == 1);
        }
    }
    g
}

fn error(t: usize) -> Grid {
    let mut g = blank();
    let shake = [-1, 1, -1, 1, 0, 0, 0, 0][t];
    lotus_body(&mut g, 21);
    let cy = head(&mut g, CX + shake, 5, 0);
    face(&mut g, CX + shake, cy, Eyes::Dizzy, Mouth::Flat);
    let mut g = outline(&g);
    if t % 4 < 3 {
        put(&mut g, &[(33, 3), (33, 4), (33, 5), (33, 7)], Ink::Alarm);
    }
    if t >= 3 {
        let drop = t as i32 - 3;
        let x = CX + 7 + shake;
        put(&mut g, &[(x, 9 + drop * 2), (x, 10 + drop * 2), (x + 1, 10 + drop * 2)], Ink::Water);
    }
    g
}

#[cfg(test)]
mod tests;
