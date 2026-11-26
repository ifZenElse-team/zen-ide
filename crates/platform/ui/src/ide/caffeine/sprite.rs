pub(crate) const SIZE: usize = 32;

pub(crate) const ICON: usize = 15;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Ink {
    Clear,
    Outline,
    Hair,
    Skin,
    Shirt,
    ShirtShade,
    White,
    Pupil,
    Red,
    Mouth,
    Cup,
    Coffee,
    Table,
    Steam,
    Snore,
}

impl Ink {
    fn solid(self) -> bool {
        !matches!(self, Ink::Clear | Ink::Outline | Ink::Steam | Ink::Snore)
    }

    fn from_char(c: char) -> Ink {
        match c {
            'o' => Ink::Outline,
            'h' => Ink::Hair,
            's' => Ink::Skin,
            'b' => Ink::Shirt,
            'B' => Ink::ShirtShade,
            'w' => Ink::White,
            'p' => Ink::Pupil,
            'r' => Ink::Red,
            'm' => Ink::Mouth,
            'c' => Ink::Cup,
            'k' => Ink::Coffee,
            't' => Ink::Table,
            '~' => Ink::Steam,
            'z' => Ink::Snore,
            _ => Ink::Clear,
        }
    }
}

pub(crate) type Grid<const N: usize> = [[Ink; N]; N];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Look {
    Asleep,
    Waiting,
    Steady,
    Wide,
    Wired,
}

impl Look {
    #[cfg(test)]
    pub(crate) const ALL: [Look; 5] = [Look::Asleep, Look::Waiting, Look::Steady, Look::Wide, Look::Wired];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Look::Asleep => "asleep",
            Look::Waiting => "waiting",
            Look::Steady => "steady",
            Look::Wide => "wide",
            Look::Wired => "wired",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cup {
    Down,
    Mid,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Eyes {
    Closed,
    Open,
    Back,
    Wide,
    Red,
}

#[derive(Clone, Copy, Debug)]
struct Step {
    cup: Cup,
    eyes: Eyes,
    steam: Option<u8>,
    snore: Option<u8>,
    shake: i32,
    nod: i32,
    ms: u32,
}

const fn step(cup: Cup, eyes: Eyes, steam: Option<u8>, ms: u32) -> Step {
    Step { cup, eyes, steam, snore: None, shake: 0, nod: 0, ms }
}

const fn snore(phase: u8, nod: i32, ms: u32) -> Step {
    Step { cup: Cup::Down, eyes: Eyes::Closed, steam: None, snore: Some(phase), shake: 0, nod, ms }
}

const fn gulp(cup: Cup, steam: Option<u8>, shake: i32, ms: u32) -> Step {
    Step { cup, eyes: Eyes::Red, steam, snore: None, shake, nod: 0, ms }
}

const ASLEEP: [Step; 4] = [snore(0, 1, 700), snore(1, 1, 700), snore(2, 0, 700), snore(3, 0, 700)];

const WAITING: [Step; 5] = [
    step(Cup::Down, Eyes::Open, None, 1600),
    step(Cup::Down, Eyes::Closed, None, 140),
    step(Cup::Down, Eyes::Open, None, 900),
    step(Cup::Down, Eyes::Back, None, 1300),
    step(Cup::Down, Eyes::Open, None, 600),
];

const STEADY: [Step; 11] = [
    step(Cup::Down, Eyes::Open, Some(0), 300),
    step(Cup::Down, Eyes::Open, Some(1), 300),
    step(Cup::Down, Eyes::Open, Some(2), 300),
    step(Cup::Down, Eyes::Open, Some(3), 300),
    step(Cup::Down, Eyes::Closed, Some(0), 140),
    step(Cup::Down, Eyes::Open, Some(1), 300),
    step(Cup::Down, Eyes::Open, Some(2), 300),
    step(Cup::Down, Eyes::Open, Some(3), 300),
    step(Cup::Mid, Eyes::Open, None, 220),
    step(Cup::Up, Eyes::Closed, None, 1000),
    step(Cup::Mid, Eyes::Open, None, 220),
];

const WIDE: [Step; 6] = [
    step(Cup::Down, Eyes::Wide, Some(0), 220),
    step(Cup::Down, Eyes::Wide, Some(1), 220),
    step(Cup::Down, Eyes::Wide, Some(2), 220),
    step(Cup::Mid, Eyes::Wide, None, 120),
    step(Cup::Up, Eyes::Wide, None, 650),
    step(Cup::Mid, Eyes::Wide, None, 120),
];

const WIRED: [Step; 8] = [
    gulp(Cup::Down, Some(0), 0, 90),
    gulp(Cup::Down, Some(2), 1, 90),
    gulp(Cup::Mid, None, 0, 60),
    gulp(Cup::Up, None, 1, 80),
    gulp(Cup::Up, None, 0, 80),
    gulp(Cup::Up, None, 1, 80),
    gulp(Cup::Up, None, 0, 80),
    gulp(Cup::Mid, None, 1, 60),
];

fn steps(look: Look) -> &'static [Step] {
    match look {
        Look::Asleep => &ASLEEP,
        Look::Waiting => &WAITING,
        Look::Steady => &STEADY,
        Look::Wide => &WIDE,
        Look::Wired => &WIRED,
    }
}

#[cfg(test)]
pub(crate) fn frames(look: Look) -> usize {
    steps(look).len()
}

pub(crate) fn cycle_ms(look: Look) -> u64 {
    steps(look).iter().map(|step| u64::from(step.ms)).sum()
}

pub(crate) fn frame_at(look: Look, ms: u64) -> (usize, u64) {
    let mut at = ms % cycle_ms(look);
    for (index, step) in steps(look).iter().enumerate() {
        let length = u64::from(step.ms);
        if at < length {
            return (index, length - at);
        }
        at -= length;
    }
    (0, u64::from(steps(look)[0].ms))
}

pub(crate) fn still(look: Look) -> usize {
    match look {
        Look::Asleep => 1,
        Look::Waiting | Look::Wide | Look::Wired => 0,
        Look::Steady => 1,
    }
}

const HEAD: [&str; 12] = [
    "...hhhhhh...",
    ".hhhhhhhhhh.",
    "hhhhhhhhhhhh",
    "hhhhhhhhhhhh",
    "hhhhssssssss",
    "hhhsssssssss",
    "hhssssssssss",
    "hsssssssssss",
    "ssssssssssss",
    "ssssssssssss",
    ".ssssssssss.",
    "..ssssssss..",
];

const SPIKES: [&str; 2] = ["..h...h..h..", "..hh.hh.hh.."];

const BODY: [&str; 16] = [
    ".....ssss.....",
    ".....ssss.....",
    "...bbbssbbb...",
    "..bbbbbsbbbbb.",
    ".Bbbbbbbbbbbbb",
    "BBbbbbbbbbbbbb",
    "BBbbbbbbbbbbbb",
    "BBbbbbbbbbbbbb",
    "BBbbbbbbbbbbbb",
    "BBbbbbbbbbbbbb",
    "BBbbbbbbbbbbbb",
    "BBbbbbbbbbbbbb",
    "BBbbbbbbbbbbbb",
    "BBbbbbbbbbbbbb",
    "BBbbbbbbbbbbbb",
    "BBbbbbbbbbbbbb",
];

const TABLE: [&str; 7] =
    ["tttttttttttt", "tttttttttttt", ".........tt.", ".........tt.", ".........tt.", ".........tt.", ".........tt."];

const MUG: [&str; 6] = ["ckkkc..", "ccccccc", "ccccc.c", "ccccc.c", "ccccccc", "ccccc.."];

const SMALL_Z: [&str; 4] = ["zzzz", "..z.", ".z..", "zzzz"];

const BIG_Z: [&str; 5] = ["zzzzz", "...z.", "..z..", ".z...", "zzzzz"];

const HEAD_AT: (i32, i32) = (4, 3);

const SHOULDER: (i32, i32) = (14, 19);

fn cup_at(cup: Cup) -> ((i32, i32), (i32, i32)) {
    match cup {
        Cup::Down => ((20, 18), (18, 21)),
        Cup::Mid => ((18, 12), (16, 15)),
        Cup::Up => ((16, 9), (15, 14)),
    }
}

struct Canvas<const N: usize> {
    grid: Grid<N>,
    shift: (i32, i32),
}

impl<const N: usize> Canvas<N> {
    fn new() -> Self {
        Self { grid: [[Ink::Clear; N]; N], shift: (0, 0) }
    }

    fn set(&mut self, x: i32, y: i32, ink: Ink) {
        let (x, y) = (x + self.shift.0, y + self.shift.1);
        if let (Ok(x), Ok(y)) = (usize::try_from(x), usize::try_from(y))
            && x < N
            && y < N
        {
            self.grid[y][x] = ink;
        }
    }

    fn stamp(&mut self, art: &[&str], at: (i32, i32)) {
        for (row, line) in art.iter().enumerate() {
            for (column, c) in line.chars().enumerate() {
                let ink = Ink::from_char(c);
                if ink != Ink::Clear {
                    self.set(at.0 + column as i32, at.1 + row as i32, ink);
                }
            }
        }
    }

    fn block(&mut self, x: i32, y: i32, w: i32, h: i32, ink: Ink) {
        for dy in 0..h {
            for dx in 0..w {
                self.set(x + dx, y + dy, ink);
            }
        }
    }

    fn line(&mut self, from: (i32, i32), to: (i32, i32), thick: i32, ink: Ink) {
        let steps = (to.0 - from.0).abs().max((to.1 - from.1).abs()).max(1);
        for step in 0..=steps {
            let x = from.0 + (to.0 - from.0) * step / steps;
            let y = from.1 + (to.1 - from.1) * step / steps;
            self.block(x - thick / 2, y - thick / 2, thick, thick, ink);
        }
    }

    fn outline(&mut self) {
        let solid = |grid: &Grid<N>, x: i32, y: i32| {
            usize::try_from(x)
                .ok()
                .zip(usize::try_from(y).ok())
                .is_some_and(|(x, y)| x < N && y < N && grid[y][x].solid())
        };
        let before = self.grid;
        for y in 0..N {
            for x in 0..N {
                if before[y][x] != Ink::Clear {
                    continue;
                }
                let (x, y) = (x as i32, y as i32);
                if [(1, 0), (-1, 0), (0, 1), (0, -1)].iter().any(|(dx, dy)| solid(&before, x + dx, y + dy)) {
                    self.grid[y as usize][x as usize] = Ink::Outline;
                }
            }
        }
    }

    fn steam(&mut self, at: (i32, i32), phase: u8, ink: Ink) {
        const ZIG: [i32; 4] = [0, 1, 1, 0];
        for wisp in 0..2_i32 {
            for rise in 0..5_i32 {
                if (rise + i32::from(phase) + wisp * 2) % 5 == 4 {
                    continue;
                }
                let x = at.0 + 1 + wisp * 2 + ZIG[((rise + i32::from(phase) + wisp) % 4) as usize];
                self.set(x, at.1 - 2 - rise, ink);
            }
        }
    }
}

pub(crate) fn frame(look: Look, index: usize) -> Grid<SIZE> {
    let steps = steps(look);
    let step = steps[index % steps.len()];
    let mut canvas = Canvas::<SIZE>::new();
    canvas.stamp(&TABLE, (19, 25));
    canvas.shift = (step.shake, 0);
    canvas.stamp(&BODY, (2, 15));
    let head = (HEAD_AT.0, HEAD_AT.1 + step.nod);
    canvas.stamp(&HEAD, head);
    if look == Look::Wired {
        canvas.stamp(&SPIKES, (head.0, head.1 - 2));
    }
    let (cup, hand) = cup_at(step.cup);
    canvas.line(SHOULDER, hand, 3, Ink::Shirt);
    canvas.stamp(&MUG, cup);
    canvas.block(hand.0 - 1, hand.1 - 1, 3, 2, Ink::Skin);
    let face = |x: i32, y: i32| (head.0 + x, head.1 + y);
    let put = |canvas: &mut Canvas<SIZE>, (x, y): (i32, i32), ink| canvas.set(x, y, ink);
    for eye in [6, 9] {
        match step.eyes {
            Eyes::Closed => {
                put(&mut canvas, face(eye - 1, 7), Ink::Pupil);
                put(&mut canvas, face(eye, 7), Ink::Pupil);
            }
            Eyes::Open => {
                put(&mut canvas, face(eye, 6), Ink::Pupil);
                put(&mut canvas, face(eye, 7), Ink::Pupil);
            }
            Eyes::Back => {
                put(&mut canvas, face(eye - 1, 6), Ink::Pupil);
                put(&mut canvas, face(eye - 1, 7), Ink::Pupil);
            }
            Eyes::Wide | Eyes::Red => {
                let white = if step.eyes == Eyes::Red { Ink::Red } else { Ink::White };
                for (x, y) in [(eye - 1, 5), (eye, 5), (eye - 1, 6), (eye, 6), (eye - 1, 7), (eye, 7)] {
                    put(&mut canvas, face(x, y), white);
                }
                put(&mut canvas, face(eye, 6), Ink::Pupil);
                if step.eyes == Eyes::Red {
                    put(&mut canvas, face(eye - 1, 8), Ink::Outline);
                    put(&mut canvas, face(eye, 8), Ink::Outline);
                } else {
                    put(&mut canvas, face(eye - 1, 3), Ink::Pupil);
                    put(&mut canvas, face(eye, 3), Ink::Pupil);
                }
            }
        }
    }
    if step.cup != Cup::Up {
        match (look, step.eyes) {
            (Look::Asleep, _) => put(&mut canvas, face(9, 9), Ink::Mouth),
            (Look::Wired, _) => {
                for (x, y) in [(8, 9), (9, 9), (10, 9), (8, 10), (9, 10), (10, 10)] {
                    put(&mut canvas, face(x, y), Ink::Mouth);
                }
            }
            _ => {
                put(&mut canvas, face(8, 9), Ink::Mouth);
                put(&mut canvas, face(9, 9), Ink::Mouth);
            }
        }
    }
    canvas.shift = (0, 0);
    canvas.outline();
    canvas.shift = (step.shake, 0);
    if let Some(phase) = step.steam {
        canvas.steam(cup, phase, Ink::Steam);
    }
    canvas.shift = (0, 0);
    if let Some(phase) = step.snore {
        let small = (18, 7 + step.nod);
        let big = (23, 0);
        match phase {
            0 => canvas.stamp(&SMALL_Z, small),
            1 => {
                canvas.stamp(&SMALL_Z, small);
                canvas.stamp(&BIG_Z, big);
            }
            2 => canvas.stamp(&BIG_Z, big),
            _ => {}
        }
    }
    canvas.grid
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Badge {
    Asleep,
    Cold,
    Hot,
    Max,
}

impl Badge {
    pub(crate) fn of(look: Look) -> Badge {
        match look {
            Look::Asleep => Badge::Asleep,
            Look::Waiting => Badge::Cold,
            Look::Steady | Look::Wide => Badge::Hot,
            Look::Wired => Badge::Max,
        }
    }
}

const ICON_MUG: [&str; 8] =
    ["ckkkkkkc..", "cccccccccc", "cccccccc.c", "cccccccc.c", "cccccccc.c", "cccccccccc", "cccccccc..", "cccccccc.."];

pub(crate) fn icon(badge: Badge) -> Grid<ICON> {
    let mut canvas = Canvas::<ICON>::new();
    canvas.stamp(&ICON_MUG, (1, 6));
    canvas.outline();
    let steam = match badge {
        Badge::Hot => Some(Ink::Steam),
        Badge::Max => Some(Ink::Red),
        Badge::Asleep | Badge::Cold => None,
    };
    if let Some(ink) = steam {
        for x in [3, 6] {
            for (dy, dx) in [(0, 0), (1, 1), (2, 1), (3, 0), (4, 0)] {
                canvas.set(x + dx, 4 - dy, ink);
            }
        }
        if badge == Badge::Max {
            for (dy, dx) in [(0, 0), (1, 1), (2, 1), (3, 0)] {
                canvas.set(9 + dx, 4 - dy, ink);
            }
        }
    }
    if badge == Badge::Asleep {
        canvas.stamp(&SMALL_Z, (11, 0));
    }
    canvas.grid
}
