use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rgba(pub [u8; 4]);

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba([r, g, b, 0xFF])
    }

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Rgba {
        Rgba([r, g, b, a])
    }

    pub const fn r(self) -> u8 {
        self.0[0]
    }

    pub const fn g(self) -> u8 {
        self.0[1]
    }

    pub const fn b(self) -> u8 {
        self.0[2]
    }

    pub const fn a(self) -> u8 {
        self.0[3]
    }

    pub fn over(self, background: Rgba) -> Rgba {
        let alpha = u32::from(self.a());
        let mix = |front: u8, back: u8| {
            let value = (u32::from(front) * alpha + u32::from(back) * (255 - alpha) + 127) / 255;
            u8::try_from(value).unwrap_or(u8::MAX)
        };
        Rgba::rgb(mix(self.r(), background.r()), mix(self.g(), background.g()), mix(self.b(), background.b()))
    }

    pub fn towards(self, target: Rgba, amount: f32) -> Rgba {
        let amount = amount.clamp(0.0, 1.0);
        let mix = |from: u8, to: u8| (f32::from(from) + (f32::from(to) - f32::from(from)) * amount).round() as u8;
        Rgba::new(mix(self.r(), target.r()), mix(self.g(), target.g()), mix(self.b(), target.b()), self.a())
    }

    pub fn luminance(self) -> f32 {
        let channel = |value: u8| {
            let value = f32::from(value) / 255.0;
            if value <= 0.039_28 { value / 12.92 } else { ((value + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * channel(self.r()) + 0.7152 * channel(self.g()) + 0.0722 * channel(self.b())
    }

    pub fn contrast(self, other: Rgba) -> f32 {
        let (one, two) = (self.luminance(), other.luminance());
        (one.max(two) + 0.05) / (one.min(two) + 0.05)
    }

    pub fn is_light(self) -> bool {
        self.contrast(Rgba::rgb(0, 0, 0)) > self.contrast(Rgba::rgb(0xFF, 0xFF, 0xFF))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseColorError(String);

impl fmt::Display for ParseColorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "`{}` is not a colour; expected #RGB, #RRGGBB or #RRGGBBAA", self.0)
    }
}

impl std::error::Error for ParseColorError {}

impl FromStr for Rgba {
    type Err = ParseColorError;

    fn from_str(text: &str) -> Result<Rgba, ParseColorError> {
        let error = || ParseColorError(text.to_owned());
        let digits = text.strip_prefix('#').ok_or_else(error)?;
        if !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(error());
        }
        let nibble = |index: usize| u8::from_str_radix(&digits[index..=index], 16).map_err(|_| error());
        let byte = |index: usize| u8::from_str_radix(&digits[index..index + 2], 16).map_err(|_| error());
        match digits.len() {
            3 => Ok(Rgba::rgb(nibble(0)? * 0x11, nibble(1)? * 0x11, nibble(2)? * 0x11)),
            6 => Ok(Rgba::rgb(byte(0)?, byte(2)?, byte(4)?)),
            8 => Ok(Rgba::new(byte(0)?, byte(2)?, byte(4)?, byte(6)?)),
            _ => Err(error()),
        }
    }
}

impl fmt::Display for Rgba {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let [r, g, b, a] = self.0;
        if a == 0xFF { write!(f, "#{r:02X}{g:02X}{b:02X}") } else { write!(f, "#{r:02X}{g:02X}{b:02X}{a:02X}") }
    }
}

impl Serialize for Rgba {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Rgba {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Rgba, D::Error> {
        let text = Cow::<str>::deserialize(deserializer)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

impl JsonSchema for Rgba {
    fn schema_name() -> Cow<'static, str> {
        "Color".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "A colour: #RGB, #RRGGBB or #RRGGBBAA, in hexadecimal digits of either case.",
            "type": "string",
            "pattern": "^#([0-9A-Fa-f]{3}|[0-9A-Fa-f]{6}|[0-9A-Fa-f]{8})$",
        })
    }
}

/// How text is underlined.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Underline {
    /// A straight line.
    Line,
    /// A wavy line, as under a misspelt word.
    Wave,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Style {
    pub color: Rgba,
    pub background: Option<Rgba>,
    pub bold: bool,
    pub italic: bool,
    pub underline: Option<Underline>,
    pub strikethrough: bool,
}

impl Style {
    pub const fn color(color: Rgba) -> Style {
        Style { color, background: None, bold: false, italic: false, underline: None, strikethrough: false }
    }

    pub const fn italic(self) -> Style {
        Style { italic: true, ..self }
    }

    pub const fn underlined(self) -> Style {
        Style { underline: Some(Underline::Line), ..self }
    }

    pub const fn is_plain(&self) -> bool {
        self.background.is_none() && !self.bold && !self.italic && self.underline.is_none() && !self.strikethrough
    }
}

impl fmt::Display for Style {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.color)?;
        if let Some(background) = self.background {
            write!(f, " on {background}")?;
        }
        for (on, word) in [(self.bold, "bold"), (self.italic, "italic"), (self.strikethrough, "strikethrough")] {
            if on {
                write!(f, " {word}")?;
            }
        }
        match self.underline {
            Some(Underline::Line) => write!(f, " underline"),
            Some(Underline::Wave) => write!(f, " wave"),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Rgba, ParseColorError> {
        text.parse()
    }

    #[test]
    fn the_three_forms_parse() {
        assert_eq!(parse("#25282C"), Ok(Rgba::rgb(0x25, 0x28, 0x2C)));
        assert_eq!(parse("#25282c"), Ok(Rgba::rgb(0x25, 0x28, 0x2C)));
        assert_eq!(parse("#fA0"), Ok(Rgba::rgb(0xFF, 0xAA, 0x00)));
        assert_eq!(parse("#0000006E"), Ok(Rgba::new(0, 0, 0, 0x6E)));
        assert_eq!(parse("#000"), Ok(Rgba::rgb(0, 0, 0)));
        assert_eq!(parse("#FFFFFFFF"), Ok(Rgba::rgb(0xFF, 0xFF, 0xFF)));
    }

    #[test]
    fn malformed_colours_are_rejected() {
        for text in
            ["", "#", "25282C", "#25282", "#25282C0", "#25282C001", "#GG0000", "#+12345", "# 12345", "#ÿÿÿ", "#12\u{0}"]
        {
            assert!(parse(text).is_err(), "`{text}` parsed");
        }
    }

    #[test]
    fn opaque_colours_print_without_alpha() {
        assert_eq!(Rgba::rgb(0x0A, 0xBC, 0xDE).to_string(), "#0ABCDE");
        assert_eq!(Rgba::new(0x80, 0x80, 0x80, 0x30).to_string(), "#80808030");
        assert_eq!(Rgba::new(1, 2, 3, 0).to_string(), "#01020300");
    }

    #[test]
    fn printing_and_parsing_round_trip() {
        for rgba in [Rgba::rgb(0, 0, 0), Rgba::new(0xFF, 0x00, 0x7F, 0x01), Rgba::rgb(0x12, 0x34, 0x56)] {
            assert_eq!(parse(&rgba.to_string()), Ok(rgba));
        }
    }

    #[test]
    fn styles_describe_themselves() {
        let style = Style {
            background: Some(Rgba::rgb(1, 2, 3)),
            bold: true,
            underline: Some(Underline::Wave),
            ..Style::color(Rgba::rgb(0xFF, 0, 0)).italic()
        };
        assert_eq!(style.to_string(), "#FF0000 on #010203 bold italic wave");
        assert!(Style::color(Rgba::rgb(0, 0, 0)).is_plain());
        assert!(!style.is_plain());
    }
}
