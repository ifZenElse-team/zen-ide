use schemars::JsonSchema;
use serde::Serialize;

use crate::value::{Typed, Value};

const MOST_DEPTH: usize = 32;
const ARC_STEPS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Point,
    LineString,
    Polygon,
    MultiPoint,
    MultiLineString,
    MultiPolygon,
    GeometryCollection,
    CircularString,
    CompoundCurve,
    CurvePolygon,
    MultiCurve,
    MultiSurface,
    PolyhedralSurface,
    Tin,
    Triangle,
}

const KINDS: [(Kind, u32, &str); 15] = [
    (Kind::Point, 1, "POINT"),
    (Kind::LineString, 2, "LINESTRING"),
    (Kind::Polygon, 3, "POLYGON"),
    (Kind::MultiPoint, 4, "MULTIPOINT"),
    (Kind::MultiLineString, 5, "MULTILINESTRING"),
    (Kind::MultiPolygon, 6, "MULTIPOLYGON"),
    (Kind::GeometryCollection, 7, "GEOMETRYCOLLECTION"),
    (Kind::CircularString, 8, "CIRCULARSTRING"),
    (Kind::CompoundCurve, 9, "COMPOUNDCURVE"),
    (Kind::CurvePolygon, 10, "CURVEPOLYGON"),
    (Kind::MultiCurve, 11, "MULTICURVE"),
    (Kind::MultiSurface, 12, "MULTISURFACE"),
    (Kind::PolyhedralSurface, 15, "POLYHEDRALSURFACE"),
    (Kind::Tin, 16, "TIN"),
    (Kind::Triangle, 17, "TRIANGLE"),
];

impl Kind {
    fn from_code(code: u32) -> Option<Self> {
        KINDS.iter().find(|(_, known, _)| *known == code).map(|(kind, _, _)| *kind)
    }

    fn from_name(name: &str) -> Option<Self> {
        KINDS.iter().find(|(_, _, known)| known.eq_ignore_ascii_case(name)).map(|(kind, _, _)| *kind)
    }

    pub fn name(self) -> &'static str {
        KINDS.iter().find(|(kind, _, _)| *kind == self).map_or("GEOMETRY", |(_, _, name)| name)
    }

    fn member(self) -> Option<Kind> {
        match self {
            Kind::MultiPoint => Some(Kind::Point),
            Kind::MultiLineString | Kind::CompoundCurve | Kind::CurvePolygon | Kind::MultiCurve => {
                Some(Kind::LineString)
            }
            Kind::MultiPolygon | Kind::MultiSurface | Kind::PolyhedralSurface => Some(Kind::Polygon),
            Kind::Tin => Some(Kind::Triangle),
            _ => None,
        }
    }

    fn shape(self) -> Shape {
        match self {
            Kind::Point => Shape::Point,
            Kind::LineString | Kind::CircularString => Shape::Line,
            Kind::Polygon | Kind::Triangle => Shape::Rings,
            _ => Shape::Parts,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Shape {
    Point,
    Line,
    Rings,
    Parts,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Coord {
    pub x: f64,
    pub y: f64,
    pub z: Option<f64>,
    pub m: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Body {
    Point(Option<Coord>),
    Line(Vec<Coord>),
    Rings(Vec<Vec<Coord>>),
    Parts(Vec<Geometry>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    pub kind: Kind,
    pub z: bool,
    pub m: bool,
    pub body: Body,
}

/// Where a geometry was read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Encoding {
    /// OGC well-known binary.
    Wkb,
    /// PostGIS's extended WKB, with its SRID.
    Ewkb,
    /// MySQL's stored form: a four-byte SRID and WKB.
    Mysql,
    /// A GeoPackage blob.
    Geopackage,
    /// Well-known text.
    Wkt,
    /// PostGIS's extended WKT, `SRID=4326;…`.
    Ewkt,
    /// GeoJSON.
    Geojson,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Spatial {
    pub geometry: Geometry,
    pub srid: Option<i32>,
    pub encoding: Encoding,
}

/// The smallest rectangle around a geometry's positions.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    /// The least x.
    pub min_x: f64,
    /// The least y.
    pub min_y: f64,
    /// The greatest x.
    pub max_x: f64,
    /// The greatest y.
    pub max_y: f64,
}

impl Bounds {
    fn of(x: f64, y: f64) -> Self {
        Self { min_x: x, min_y: y, max_x: x, max_y: y }
    }

    pub fn union(self, other: Bounds) -> Bounds {
        Bounds {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }
}

/// What a geometry holds, for answers sized for agents.
#[derive(Clone, Debug, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    /// Its kind as WKT names it (`POLYGON`).
    pub kind: String,
    /// Its spatial reference system's id, when the value carries one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub srid: Option<i32>,
    /// Its axes: `xy`, `xyz`, `xym` or `xyzm`.
    pub dimensions: String,
    /// Where it was read from.
    pub encoding: Encoding,
    /// The positions it has.
    pub points: u64,
    /// Its members: a collection's geometries, a polygon's rings, 1 for a point or a line.
    pub parts: u64,
    /// Whether it holds nothing.
    pub empty: bool,
    /// The rectangle around it; absent when empty.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Outlines {
    pub points: Vec<[f64; 2]>,
    pub lines: Vec<Vec<[f64; 2]>>,
    pub areas: Vec<Vec<Vec<[f64; 2]>>>,
}

impl Geometry {
    pub fn is_empty(&self) -> bool {
        match &self.body {
            Body::Point(point) => point.is_none(),
            Body::Line(line) => line.is_empty(),
            Body::Rings(rings) => rings.iter().all(Vec::is_empty),
            Body::Parts(parts) => parts.iter().all(Geometry::is_empty),
        }
    }

    fn each(&self, visit: &mut impl FnMut(&Coord)) {
        match &self.body {
            Body::Point(point) => point.iter().for_each(&mut *visit),
            Body::Line(line) => line.iter().for_each(&mut *visit),
            Body::Rings(rings) => rings.iter().flatten().for_each(&mut *visit),
            Body::Parts(parts) => parts.iter().for_each(|part| part.each(visit)),
        }
    }

    pub fn points(&self) -> u64 {
        let mut count = 0;
        self.each(&mut |_| count += 1);
        count
    }

    pub fn outline_points(&self) -> u64 {
        match (&self.body, self.kind) {
            (Body::Line(line), Kind::CircularString) => line.len() as u64 * ARC_STEPS as u64,
            (Body::Parts(parts), _) => parts.iter().map(Geometry::outline_points).sum(),
            _ => self.points(),
        }
    }

    pub fn bounds(&self) -> Option<Bounds> {
        let mut bounds: Option<Bounds> = None;
        self.each(&mut |coord| {
            if coord.x.is_finite() && coord.y.is_finite() {
                let here = Bounds::of(coord.x, coord.y);
                bounds = Some(bounds.map_or(here, |known| known.union(here)));
            }
        });
        bounds
    }

    fn parts(&self) -> u64 {
        match &self.body {
            Body::Point(_) | Body::Line(_) => 1,
            Body::Rings(rings) => rings.len() as u64,
            Body::Parts(parts) => parts.len() as u64,
        }
    }

    fn dimensions(&self) -> &'static str {
        match (self.z, self.m) {
            (false, false) => "xy",
            (true, false) => "xyz",
            (false, true) => "xym",
            (true, true) => "xyzm",
        }
    }

    fn path(&self) -> Vec<[f64; 2]> {
        match (&self.body, self.kind) {
            (Body::Line(line), Kind::CircularString) => arcs(line),
            (Body::Line(line), _) => line.iter().map(|coord| [coord.x, coord.y]).collect(),
            (Body::Parts(parts), _) => {
                let mut path: Vec<[f64; 2]> = Vec::new();
                for part in parts {
                    let next = part.path();
                    let skip = usize::from(path.last().is_some() && path.last() == next.first());
                    path.extend(next.into_iter().skip(skip));
                }
                path
            }
            (Body::Rings(rings), _) => {
                rings.first().map(|ring| ring.iter().map(|coord| [coord.x, coord.y]).collect()).unwrap_or_default()
            }
            (Body::Point(point), _) => point.iter().map(|coord| [coord.x, coord.y]).collect(),
        }
    }

    pub fn outlines(&self) -> Outlines {
        let mut outlines = Outlines::default();
        self.flatten(&mut outlines);
        outlines
    }

    fn flatten(&self, out: &mut Outlines) {
        match (&self.body, self.kind) {
            (Body::Point(Some(coord)), _) => out.points.push([coord.x, coord.y]),
            (Body::Point(None), _) => {}
            (Body::Line(_), _) => {
                let path = self.path();
                if path.len() == 1 {
                    out.points.push(path[0]);
                } else if !path.is_empty() {
                    out.lines.push(path);
                }
            }
            (Body::Rings(rings), _) => {
                let rings: Vec<Vec<[f64; 2]>> = rings
                    .iter()
                    .filter(|ring| !ring.is_empty())
                    .map(|ring| ring.iter().map(|coord| [coord.x, coord.y]).collect())
                    .collect();
                if !rings.is_empty() {
                    out.areas.push(rings);
                }
            }
            (Body::Parts(parts), Kind::CurvePolygon) => {
                let rings: Vec<Vec<[f64; 2]>> =
                    parts.iter().map(Geometry::path).filter(|ring| !ring.is_empty()).collect();
                if !rings.is_empty() {
                    out.areas.push(rings);
                }
            }
            (Body::Parts(_), Kind::CompoundCurve) => {
                let path = self.path();
                if !path.is_empty() {
                    out.lines.push(path);
                }
            }
            (Body::Parts(parts), _) => parts.iter().for_each(|part| part.flatten(out)),
        }
    }

    pub fn wkt(&self, chars: usize) -> (String, usize, bool) {
        let mut sink = Sink { out: String::new(), cap: chars, length: 0 };
        self.write(&mut sink, true);
        let truncated = sink.length > sink.out.len();
        (sink.out, sink.length, truncated)
    }

    fn write(&self, sink: &mut Sink, tagged: bool) {
        if tagged {
            sink.push(self.kind.name());
            sink.push(match (self.z, self.m) {
                (false, false) => "",
                (true, false) => " Z",
                (false, true) => " M",
                (true, true) => " ZM",
            });
            sink.push(" ");
        }
        if self.is_empty() && !matches!(&self.body, Body::Parts(parts) if !parts.is_empty()) {
            sink.push("EMPTY");
            return;
        }
        match &self.body {
            Body::Point(Some(coord)) => {
                sink.push("(");
                self.coord(sink, coord);
                sink.push(")");
            }
            Body::Point(None) => sink.push("EMPTY"),
            Body::Line(line) => self.line(sink, line),
            Body::Rings(rings) => {
                sink.push("(");
                for (index, ring) in rings.iter().enumerate() {
                    if index > 0 {
                        sink.push(",");
                    }
                    self.line(sink, ring);
                }
                sink.push(")");
            }
            Body::Parts(parts) => {
                sink.push("(");
                for (index, part) in parts.iter().enumerate() {
                    if index > 0 {
                        sink.push(",");
                    }
                    part.write(sink, self.kind.member() != Some(part.kind));
                }
                sink.push(")");
            }
        }
    }

    fn line(&self, sink: &mut Sink, line: &[Coord]) {
        if line.is_empty() {
            sink.push("EMPTY");
            return;
        }
        sink.push("(");
        for (index, coord) in line.iter().enumerate() {
            if index > 0 {
                sink.push(",");
            }
            self.coord(sink, coord);
        }
        sink.push(")");
    }

    fn coord(&self, sink: &mut Sink, coord: &Coord) {
        sink.push(&number(coord.x));
        sink.push(" ");
        sink.push(&number(coord.y));
        if self.z {
            sink.push(" ");
            sink.push(&number(coord.z.unwrap_or(0.0)));
        }
        if self.m {
            sink.push(" ");
            sink.push(&number(coord.m.unwrap_or(0.0)));
        }
    }
}

struct Sink {
    out: String,
    cap: usize,
    length: usize,
}

impl Sink {
    fn push(&mut self, text: &str) {
        self.length += text.len();
        if self.out.len() < self.cap {
            let room = self.cap - self.out.len();
            self.out.push_str(&text[..text.len().min(room)]);
        }
    }
}

fn number(value: f64) -> String {
    if value == 0.0 { "0".to_owned() } else { value.to_string() }
}

fn arcs(line: &[Coord]) -> Vec<[f64; 2]> {
    let mut path = Vec::new();
    let Some(first) = line.first() else { return path };
    path.push([first.x, first.y]);
    let mut at = 0;
    while at + 2 < line.len() {
        let (a, b, c) = (line[at], line[at + 1], line[at + 2]);
        path.extend(arc([a.x, a.y], [b.x, b.y], [c.x, c.y]));
        at += 2;
    }
    path.extend(line[at + 1..].iter().map(|coord| [coord.x, coord.y]));
    path
}

fn arc(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Vec<[f64; 2]> {
    let tau = std::f64::consts::TAU;
    let square = |p: [f64; 2]| p[0] * p[0] + p[1] * p[1];
    let (ux, uy, full) = if a == c {
        ((a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, true)
    } else {
        let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
        if d.abs() < f64::EPSILON {
            return vec![b, c];
        }
        let ux = (square(a) * (b[1] - c[1]) + square(b) * (c[1] - a[1]) + square(c) * (a[1] - b[1])) / d;
        let uy = (square(a) * (c[0] - b[0]) + square(b) * (a[0] - c[0]) + square(c) * (b[0] - a[0])) / d;
        (ux, uy, false)
    };
    let radius = (a[0] - ux).hypot(a[1] - uy);
    let angle = |p: [f64; 2]| (p[1] - uy).atan2(p[0] - ux);
    let (start, middle, end) = (angle(a), angle(b), angle(c));
    let forward = |from: f64, to: f64| (to - from).rem_euclid(tau);
    let sweep = if full {
        tau
    } else if forward(start, middle) <= forward(start, end) {
        forward(start, end)
    } else {
        -(tau - forward(start, end))
    };
    let steps = if full { ARC_STEPS * 2 } else { ARC_STEPS };
    (1..=steps)
        .map(|step| {
            if step == steps {
                return c;
            }
            let at = start + sweep * step as f64 / steps as f64;
            [ux + radius * at.cos(), uy + radius * at.sin()]
        })
        .collect()
}

impl Spatial {
    pub fn summary(&self) -> Summary {
        Summary {
            kind: self.geometry.kind.name().to_owned(),
            srid: self.srid,
            dimensions: self.geometry.dimensions().to_owned(),
            encoding: self.encoding,
            points: self.geometry.points(),
            parts: self.geometry.parts(),
            empty: self.geometry.is_empty(),
            bounds: self.geometry.bounds(),
        }
    }

    pub fn ewkt(&self, chars: usize) -> (String, usize, bool) {
        let prefix = match self.srid {
            Some(srid) if srid != 0 => format!("SRID={srid};"),
            _ => String::new(),
        };
        let (text, length, truncated) = self.geometry.wkt(chars.saturating_sub(prefix.len()));
        (format!("{prefix}{text}"), prefix.len() + length, truncated)
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn left(&self) -> usize {
        self.bytes.len() - self.at
    }

    fn take<const N: usize>(&mut self) -> Option<[u8; N]> {
        let bytes = self.bytes.get(self.at..self.at + N)?.try_into().ok()?;
        self.at += N;
        Some(bytes)
    }

    fn byte(&mut self) -> Option<u8> {
        self.take::<1>().map(|[byte]| byte)
    }

    fn u32(&mut self, little: bool) -> Option<u32> {
        let bytes = self.take::<4>()?;
        Some(if little { u32::from_le_bytes(bytes) } else { u32::from_be_bytes(bytes) })
    }

    fn f64(&mut self, little: bool) -> Option<f64> {
        let bytes = self.take::<8>()?;
        Some(if little { f64::from_le_bytes(bytes) } else { f64::from_be_bytes(bytes) })
    }

    fn count(&mut self, little: bool, least_each: usize) -> Option<usize> {
        let count = usize::try_from(self.u32(little)?).ok()?;
        (count.checked_mul(least_each)? <= self.left()).then_some(count)
    }

    fn coord(&mut self, little: bool, z: bool, m: bool) -> Option<Coord> {
        let x = self.f64(little)?;
        let y = self.f64(little)?;
        let z = if z { Some(self.f64(little)?) } else { None };
        let m = if m { Some(self.f64(little)?) } else { None };
        Some(Coord { x, y, z, m })
    }

    fn coords(&mut self, little: bool, z: bool, m: bool) -> Option<Vec<Coord>> {
        let size = 8 * (2 + usize::from(z) + usize::from(m));
        let count = self.count(little, size)?;
        (0..count).map(|_| self.coord(little, z, m)).collect()
    }

    fn geometry(&mut self, depth: usize, srid: &mut Option<i32>, extended: &mut bool) -> Option<Geometry> {
        if depth > MOST_DEPTH {
            return None;
        }
        let little = match self.byte()? {
            0 => false,
            1 => true,
            _ => return None,
        };
        let raw = self.u32(little)?;
        let (mut z, mut m) = (raw & 0x8000_0000 != 0, raw & 0x4000_0000 != 0);
        if z || m {
            *extended = true;
        }
        if raw & 0x2000_0000 != 0 {
            *extended = true;
            let id = self.u32(little)? as i32;
            if depth == 0 {
                *srid = Some(id);
            }
        }
        let code = raw & 0x0fff_ffff;
        match code / 1000 {
            0 => {}
            1 => z = true,
            2 => m = true,
            3 => (z, m) = (true, true),
            _ => return None,
        }
        let kind = Kind::from_code(code % 1000)?;
        let body = match kind.shape() {
            Shape::Point => {
                let coord = self.coord(little, z, m)?;
                Body::Point((!(coord.x.is_nan() && coord.y.is_nan())).then_some(coord))
            }
            Shape::Line => Body::Line(self.coords(little, z, m)?),
            Shape::Rings => {
                let count = self.count(little, 4)?;
                Body::Rings((0..count).map(|_| self.coords(little, z, m)).collect::<Option<_>>()?)
            }
            Shape::Parts => {
                let count = self.count(little, 5)?;
                let mut parts = Vec::with_capacity(count);
                for _ in 0..count {
                    parts.push(self.geometry(depth + 1, &mut None, extended)?);
                }
                Body::Parts(parts)
            }
        };
        Some(Geometry { kind, z, m, body })
    }
}

pub fn from_wkb(bytes: &[u8]) -> Option<Spatial> {
    let mut reader = Reader { bytes, at: 0 };
    let (mut srid, mut extended) = (None, false);
    let geometry = reader.geometry(0, &mut srid, &mut extended)?;
    (reader.left() == 0).then_some(Spatial {
        geometry,
        srid,
        encoding: if extended { Encoding::Ewkb } else { Encoding::Wkb },
    })
}

fn from_geopackage(bytes: &[u8]) -> Option<Spatial> {
    let flags = *bytes.get(3)?;
    if !bytes.starts_with(b"GP") || flags & 0b0010_0000 != 0 {
        return None;
    }
    let little = flags & 1 == 1;
    let id: [u8; 4] = bytes.get(4..8)?.try_into().ok()?;
    let srid = if little { i32::from_le_bytes(id) } else { i32::from_be_bytes(id) };
    let envelope = match (flags >> 1) & 0b111 {
        0 => 0,
        1 => 32,
        2 | 3 => 48,
        4 => 64,
        _ => return None,
    };
    let mut spatial = from_wkb(bytes.get(8 + envelope..)?)?;
    spatial.srid = Some(srid);
    spatial.encoding = Encoding::Geopackage;
    Some(spatial)
}

fn from_mysql(bytes: &[u8]) -> Option<Spatial> {
    let id: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    let mut spatial = from_wkb(&bytes[4..])?;
    if spatial.encoding != Encoding::Wkb {
        return None;
    }
    spatial.srid = Some(i32::from_le_bytes(id));
    spatial.encoding = Encoding::Mysql;
    Some(spatial)
}

pub fn from_bytes(bytes: &[u8]) -> Option<Spatial> {
    if bytes.len() < 9 {
        return None;
    }
    if bytes.starts_with(b"GP") {
        return from_geopackage(bytes);
    }
    from_wkb(bytes).or_else(|| from_mysql(bytes))
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).ok()).collect()
}

pub fn from_text(text: &str) -> Option<Spatial> {
    let text = text.trim();
    let first = text.bytes().next()?;
    if first == b'{' {
        return from_geojson(text);
    }
    let hex = text.strip_prefix("\\x").unwrap_or(text);
    if hex.len() >= 18
        && let Some(spatial) = unhex(hex).and_then(|bytes| from_bytes(&bytes))
    {
        return Some(spatial);
    }
    if first.is_ascii_alphabetic() { from_wkt(text) } else { None }
}

pub fn from_value(value: &Value) -> Option<Spatial> {
    match value {
        Value::Bytes(bytes) => from_bytes(bytes),
        Value::Text(text) | Value::Json(text) | Value::Other(Typed { text, .. }) => from_text(text),
        _ => None,
    }
}

struct Wkt<'a> {
    text: &'a [u8],
    at: usize,
    z: Option<bool>,
    m: Option<bool>,
}

impl Wkt<'_> {
    fn skip(&mut self) {
        while self.text.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip();
        self.text.get(self.at).copied()
    }

    fn eat(&mut self, byte: u8) -> Option<()> {
        (self.peek()? == byte).then(|| self.at += 1)
    }

    fn word(&mut self) -> Option<String> {
        self.skip();
        let start = self.at;
        while self.text.get(self.at).is_some_and(u8::is_ascii_alphabetic) {
            self.at += 1;
        }
        (self.at > start).then(|| String::from_utf8_lossy(&self.text[start..self.at]).to_ascii_uppercase())
    }

    fn empty(&mut self) -> bool {
        let start = self.at;
        if self.word().as_deref() == Some("EMPTY") {
            return true;
        }
        self.at = start;
        false
    }

    fn number(&mut self) -> Option<f64> {
        self.skip();
        let start = self.at;
        while self.text.get(self.at).is_some_and(|byte| byte.is_ascii_digit() || b"+-.eE".contains(byte)) {
            self.at += 1;
        }
        std::str::from_utf8(&self.text[start..self.at]).ok()?.parse().ok()
    }

    fn coord(&mut self) -> Option<Coord> {
        let mut values = vec![self.number()?, self.number()?];
        while values.len() < 4 && self.peek().is_some_and(|byte| byte.is_ascii_digit() || b"+-.".contains(&byte)) {
            values.push(self.number()?);
        }
        let (z, m) = match (self.z, self.m, values.len()) {
            (None, None, 2) => (false, false),
            (None, None, 3) => (true, false),
            (None, None, _) => (true, true),
            (Some(z), Some(m), count) if count == 2 + usize::from(z) + usize::from(m) => (z, m),
            _ => return None,
        };
        (self.z, self.m) = (Some(z), Some(m));
        let mut extra = values[2..].iter().copied();
        Some(Coord {
            x: values[0],
            y: values[1],
            z: if z { extra.next() } else { None },
            m: if m { extra.next() } else { None },
        })
    }

    fn line(&mut self) -> Option<Vec<Coord>> {
        if self.empty() {
            return Some(Vec::new());
        }
        self.eat(b'(')?;
        let mut line = vec![self.coord()?];
        while self.eat(b',').is_some() {
            line.push(self.coord()?);
        }
        self.eat(b')')?;
        Some(line)
    }

    fn body(&mut self, kind: Kind, depth: usize) -> Option<Body> {
        if depth > MOST_DEPTH {
            return None;
        }
        if kind.shape() == Shape::Line {
            return Some(Body::Line(self.line()?));
        }
        if self.empty() {
            return Some(match kind.shape() {
                Shape::Point => Body::Point(None),
                Shape::Rings => Body::Rings(Vec::new()),
                _ => Body::Parts(Vec::new()),
            });
        }
        self.eat(b'(')?;
        let body = match kind.shape() {
            Shape::Point => Body::Point(Some(self.coord()?)),
            Shape::Rings => {
                let mut rings = vec![self.line()?];
                while self.eat(b',').is_some() {
                    rings.push(self.line()?);
                }
                Body::Rings(rings)
            }
            _ => {
                let mut parts = vec![self.part(kind, depth)?];
                while self.eat(b',').is_some() {
                    parts.push(self.part(kind, depth)?);
                }
                Body::Parts(parts)
            }
        };
        self.eat(b')')?;
        Some(body)
    }

    fn part(&mut self, parent: Kind, depth: usize) -> Option<Geometry> {
        let member = parent.member();
        let next = self.peek()?;
        let kind = match member {
            Some(Kind::Point) if next != b'(' && !next.is_ascii_alphabetic() => {
                let coord = self.coord()?;
                return Some(self.made(Kind::Point, Body::Point(Some(coord))));
            }
            Some(member) if next == b'(' => member,
            Some(member) if self.empty() => {
                self.at -= "EMPTY".len();
                member
            }
            _ => return self.tagged(depth + 1),
        };
        let body = self.body(kind, depth + 1)?;
        Some(self.made(kind, body))
    }

    fn made(&self, kind: Kind, body: Body) -> Geometry {
        Geometry { kind, z: self.z.unwrap_or(false), m: self.m.unwrap_or(false), body }
    }

    fn tagged(&mut self, depth: usize) -> Option<Geometry> {
        let word = self.word()?;
        let (name, tag) = match Kind::from_name(&word) {
            Some(_) => (word.as_str(), ""),
            None => {
                let at = word.len() - if word.ends_with("ZM") { 2 } else { 1 };
                (&word[..at], &word[at..])
            }
        };
        let kind = Kind::from_name(name)?;
        let start = self.at;
        let tag = match (tag, self.word().as_deref()) {
            ("", Some(dims @ ("Z" | "M" | "ZM"))) => dims.to_owned(),
            ("", Some("EMPTY")) | ("", None) => {
                self.at = start;
                String::new()
            }
            (tag @ ("Z" | "M" | "ZM"), None) => tag.to_owned(),
            (tag @ ("Z" | "M" | "ZM"), Some("EMPTY")) => {
                self.at = start;
                tag.to_owned()
            }
            _ => return None,
        };
        match tag.as_str() {
            "Z" => (self.z, self.m) = (Some(true), Some(false)),
            "M" => (self.z, self.m) = (Some(false), Some(true)),
            "ZM" => (self.z, self.m) = (Some(true), Some(true)),
            _ => {}
        }
        let body = self.body(kind, depth)?;
        Some(self.made(kind, body))
    }
}

pub fn from_wkt(text: &str) -> Option<Spatial> {
    let text = text.trim();
    let (srid, rest) = match text.get(..5) {
        Some(head) if head.eq_ignore_ascii_case("SRID=") => {
            let (id, rest) = text[5..].split_once(';')?;
            (Some(id.trim().parse::<i32>().ok()?), rest)
        }
        _ => (None, text),
    };
    let mut parser = Wkt { text: rest.as_bytes(), at: 0, z: None, m: None };
    let geometry = parser.tagged(0)?;
    parser.skip();
    (parser.at == rest.len()).then_some(Spatial {
        geometry,
        srid,
        encoding: if srid.is_some() { Encoding::Ewkt } else { Encoding::Wkt },
    })
}

fn position(value: &serde_json::Value) -> Option<Coord> {
    let items = value.as_array()?;
    let at = |index: usize| items.get(index).and_then(serde_json::Value::as_f64);
    Some(Coord { x: at(0)?, y: at(1)?, z: at(2), m: None })
}

fn positions(value: &serde_json::Value) -> Option<Vec<Coord>> {
    value.as_array()?.iter().map(position).collect()
}

fn geojson_geometry(value: &serde_json::Value, depth: usize) -> Option<Geometry> {
    if depth > MOST_DEPTH {
        return None;
    }
    let coordinates = value.get("coordinates");
    let lines = |value: &serde_json::Value| value.as_array()?.iter().map(positions).collect::<Option<Vec<_>>>();
    let (kind, body) = match value.get("type")?.as_str()? {
        "Point" => {
            let coordinates = coordinates?;
            let empty = coordinates.as_array().is_some_and(Vec::is_empty);
            (Kind::Point, Body::Point(if empty { None } else { Some(position(coordinates)?) }))
        }
        "LineString" => (Kind::LineString, Body::Line(positions(coordinates?)?)),
        "Polygon" => (Kind::Polygon, Body::Rings(lines(coordinates?)?)),
        "MultiPoint" => (
            Kind::MultiPoint,
            Body::Parts(
                positions(coordinates?)?
                    .into_iter()
                    .map(|coord| Geometry { kind: Kind::Point, z: false, m: false, body: Body::Point(Some(coord)) })
                    .collect(),
            ),
        ),
        "MultiLineString" => (
            Kind::MultiLineString,
            Body::Parts(
                lines(coordinates?)?
                    .into_iter()
                    .map(|line| Geometry { kind: Kind::LineString, z: false, m: false, body: Body::Line(line) })
                    .collect(),
            ),
        ),
        "MultiPolygon" => (
            Kind::MultiPolygon,
            Body::Parts(
                coordinates?
                    .as_array()?
                    .iter()
                    .map(|rings| {
                        Some(Geometry { kind: Kind::Polygon, z: false, m: false, body: Body::Rings(lines(rings)?) })
                    })
                    .collect::<Option<_>>()?,
            ),
        ),
        "GeometryCollection" => (
            Kind::GeometryCollection,
            Body::Parts(
                value
                    .get("geometries")?
                    .as_array()?
                    .iter()
                    .map(|part| geojson_geometry(part, depth + 1))
                    .collect::<Option<_>>()?,
            ),
        ),
        "Feature" => return geojson_geometry(value.get("geometry")?, depth + 1),
        "FeatureCollection" => (
            Kind::GeometryCollection,
            Body::Parts(
                value
                    .get("features")?
                    .as_array()?
                    .iter()
                    .filter_map(|feature| feature.get("geometry").filter(|geometry| !geometry.is_null()))
                    .map(|part| geojson_geometry(part, depth + 1))
                    .collect::<Option<_>>()?,
            ),
        ),
        _ => return None,
    };
    let mut geometry = Geometry { kind, z: false, m: false, body };
    let mut z = false;
    geometry.each(&mut |coord| z |= coord.z.is_some());
    set_z(&mut geometry, z);
    Some(geometry)
}

fn set_z(geometry: &mut Geometry, z: bool) {
    geometry.z = z;
    if let Body::Parts(parts) = &mut geometry.body {
        parts.iter_mut().for_each(|part| set_z(part, z));
    }
}

pub fn from_geojson(text: &str) -> Option<Spatial> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let geometry = geojson_geometry(&value, 0)?;
    Some(Spatial { geometry, srid: None, encoding: Encoding::Geojson })
}

#[cfg(test)]
mod tests;
