use encoding_rs::Encoding;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const BOM: char = '\u{feff}';

const UTF8_BOM: [u8; 3] = [0xef, 0xbb, 0xbf];

fn legacy() -> [&'static Encoding; 35] {
    [
        encoding_rs::BIG5,
        encoding_rs::EUC_JP,
        encoding_rs::EUC_KR,
        encoding_rs::GB18030,
        encoding_rs::GBK,
        encoding_rs::IBM866,
        encoding_rs::ISO_2022_JP,
        encoding_rs::ISO_8859_10,
        encoding_rs::ISO_8859_13,
        encoding_rs::ISO_8859_14,
        encoding_rs::ISO_8859_15,
        encoding_rs::ISO_8859_16,
        encoding_rs::ISO_8859_2,
        encoding_rs::ISO_8859_3,
        encoding_rs::ISO_8859_4,
        encoding_rs::ISO_8859_5,
        encoding_rs::ISO_8859_6,
        encoding_rs::ISO_8859_7,
        encoding_rs::ISO_8859_8,
        encoding_rs::ISO_8859_8_I,
        encoding_rs::KOI8_R,
        encoding_rs::KOI8_U,
        encoding_rs::MACINTOSH,
        encoding_rs::SHIFT_JIS,
        encoding_rs::WINDOWS_1250,
        encoding_rs::WINDOWS_1251,
        encoding_rs::WINDOWS_1252,
        encoding_rs::WINDOWS_1253,
        encoding_rs::WINDOWS_1254,
        encoding_rs::WINDOWS_1255,
        encoding_rs::WINDOWS_1256,
        encoding_rs::WINDOWS_1257,
        encoding_rs::WINDOWS_1258,
        encoding_rs::WINDOWS_874,
        encoding_rs::X_MAC_CYRILLIC,
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Charset {
    Latin1,
    Utf8,
    Utf8Bom,
    Utf16Be,
    Utf16Le,
    Ascii,
    Legacy(&'static Encoding),
}

impl Serialize for Charset {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.id())
    }
}

impl<'de> Deserialize<'de> for Charset {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Charset::named(&name).ok_or_else(|| serde::de::Error::custom(format!("unknown encoding {name}")))
    }
}

impl JsonSchema for Charset {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Charset".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": "string",
            "description": "An encoding: an EditorConfig charset (latin1, utf-8, utf-8-bom, utf-16be, utf-16le) or a \
                            name file.encoding.list gives, e.g. windows-1251."
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unmappable {
    pub character: char,
    pub line: usize,
    pub column: usize,
}

impl Charset {
    pub const ALL: [Charset; 5] = [Self::Latin1, Self::Utf8, Self::Utf8Bom, Self::Utf16Be, Self::Utf16Le];

    pub fn parse(value: &str) -> Option<Charset> {
        Self::ALL.into_iter().find(|charset| charset.id().eq_ignore_ascii_case(value.trim()))
    }

    pub fn named(value: &str) -> Option<Charset> {
        let name = value.trim();
        if let Some(charset) = Self::parse(name) {
            return Some(charset);
        }
        let lower = name.to_ascii_lowercase().replace('_', "-");
        match lower.as_str() {
            "utf8" | "unicode-1-1-utf-8" => return Some(Self::Utf8),
            "utf-8 bom" => return Some(Self::Utf8Bom),
            "us-ascii" | "ascii" => return Some(Self::Ascii),
            "iso-8859-1" | "iso8859-1" => return Some(Self::Latin1),
            "utf-16" => return Some(Self::Utf16Be),
            _ => {}
        }
        let encoding = Encoding::for_label(name.as_bytes())?;
        if encoding == encoding_rs::UTF_16LE {
            Some(Self::Utf16Le)
        } else if encoding == encoding_rs::UTF_16BE {
            Some(Self::Utf16Be)
        } else if encoding == encoding_rs::UTF_8 {
            Some(Self::Utf8)
        } else if encoding == encoding_rs::REPLACEMENT || encoding == encoding_rs::X_USER_DEFINED {
            None
        } else {
            Some(Self::Legacy(encoding))
        }
    }

    pub fn of_bom(bytes: &[u8]) -> Option<Charset> {
        match bytes {
            [0xef, 0xbb, 0xbf, ..] => Some(Self::Utf8Bom),
            [0xff, 0xfe, ..] => Some(Self::Utf16Le),
            [0xfe, 0xff, ..] => Some(Self::Utf16Be),
            _ => None,
        }
    }

    pub fn choices() -> Vec<Charset> {
        let mut others: Vec<Charset> = legacy().into_iter().map(Self::Legacy).collect();
        others.sort_by_key(|charset| charset.label().to_ascii_lowercase());
        let mut choices = vec![Self::Utf8, Self::Utf16Be, Self::Utf16Le, Self::Ascii, Self::Latin1];
        choices.append(&mut others);
        choices
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Latin1 => "latin1",
            Self::Utf8 => "utf-8",
            Self::Utf8Bom => "utf-8-bom",
            Self::Utf16Be => "utf-16be",
            Self::Utf16Le => "utf-16le",
            Self::Ascii => "us-ascii",
            Self::Legacy(encoding) => encoding.name(),
        }
    }

    pub fn is_utf8(self) -> bool {
        matches!(self, Self::Utf8 | Self::Utf8Bom)
    }

    pub fn holds_a_bom(self) -> bool {
        matches!(self, Self::Utf8 | Self::Utf8Bom | Self::Utf16Be | Self::Utf16Le)
    }

    pub fn bom_bytes(self) -> &'static [u8] {
        match self {
            Self::Utf8 | Self::Utf8Bom => &UTF8_BOM,
            Self::Utf16Be => &[0xfe, 0xff],
            Self::Utf16Le => &[0xff, 0xfe],
            _ => &[],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Latin1 => "ISO-8859-1",
            Self::Utf8 => "UTF-8",
            Self::Utf8Bom => "UTF-8 BOM",
            Self::Utf16Be => "UTF-16BE",
            Self::Utf16Le => "UTF-16LE",
            Self::Ascii => "US-ASCII",
            Self::Legacy(encoding) => encoding.name(),
        }
    }

    pub fn decode(self, bytes: &[u8]) -> Option<(String, bool)> {
        match self {
            Self::Latin1 => Some((bytes.iter().map(|&byte| char::from(byte)).collect(), false)),
            Self::Utf8 => String::from_utf8(bytes.to_vec()).ok().map(|text| (text, false)),
            Self::Utf8Bom => {
                let (rest, bom) = bytes.strip_prefix(&UTF8_BOM).map_or((bytes, false), |rest| (rest, true));
                String::from_utf8(rest.to_vec()).ok().map(|text| (text, bom))
            }
            Self::Utf16Be | Self::Utf16Le => {
                if !bytes.len().is_multiple_of(2) {
                    return None;
                }
                let units = bytes.as_chunks::<2>().0.iter().map(|&pair| {
                    if self == Self::Utf16Be { u16::from_be_bytes(pair) } else { u16::from_le_bytes(pair) }
                });
                let text: String = char::decode_utf16(units).collect::<Result<_, _>>().ok()?;
                match text.strip_prefix(BOM) {
                    Some(rest) => Some((rest.to_owned(), true)),
                    None => Some((text, false)),
                }
            }
            Self::Ascii => bytes.is_ascii().then(|| (String::from_utf8_lossy(bytes).into_owned(), false)),
            Self::Legacy(encoding) => encoding
                .decode_without_bom_handling_and_without_replacement(bytes)
                .map(|text| (text.into_owned(), false)),
        }
    }

    pub fn encode(self, text: &str, bom: bool) -> Result<Vec<u8>, Unmappable> {
        match self {
            Self::Latin1 => {
                let mut bytes = Vec::with_capacity(text.len());
                for (at, character) in text.char_indices() {
                    match u8::try_from(u32::from(character)) {
                        Ok(byte) => bytes.push(byte),
                        Err(_) => return Err(unmappable(text, at, character)),
                    }
                }
                Ok(bytes)
            }
            Self::Utf8 => Ok(text.as_bytes().to_vec()),
            Self::Utf8Bom => {
                let mut bytes = Vec::with_capacity(text.len() + 3);
                bytes.extend_from_slice(&UTF8_BOM);
                bytes.extend_from_slice(text.as_bytes());
                Ok(bytes)
            }
            Self::Utf16Be | Self::Utf16Le => {
                let mut bytes = Vec::with_capacity(text.len() * 2 + 2);
                let mark = bom.then_some(BOM as u16);
                for unit in mark.into_iter().chain(text.encode_utf16()) {
                    let pair = if self == Self::Utf16Be { unit.to_be_bytes() } else { unit.to_le_bytes() };
                    bytes.extend_from_slice(&pair);
                }
                Ok(bytes)
            }
            Self::Ascii => match text.char_indices().find(|(_, character)| !character.is_ascii()) {
                Some((at, character)) => Err(unmappable(text, at, character)),
                None => Ok(text.as_bytes().to_vec()),
            },
            Self::Legacy(encoding) => {
                let (bytes, _, lossy) = encoding.encode(text);
                if !lossy {
                    return Ok(bytes.into_owned());
                }
                let mut one = [0; 4];
                match text.char_indices().find(|(_, character)| encoding.encode(character.encode_utf8(&mut one)).2) {
                    Some((at, character)) => Err(unmappable(text, at, character)),
                    None => Ok(bytes.into_owned()),
                }
            }
        }
    }
}

fn unmappable(text: &str, at: usize, character: char) -> Unmappable {
    let before = &text[..at];
    let line = before.matches('\n').count() + 1;
    let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    Unmappable { character, line, column: before[line_start..].chars().count() + 1 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_editorconfig_value_is_parsed_in_any_case() {
        for charset in Charset::ALL {
            assert_eq!(Charset::parse(charset.id()), Some(charset));
            assert_eq!(Charset::parse(&charset.id().to_ascii_uppercase()), Some(charset));
        }
        assert_eq!(Charset::parse("koi8-r"), None);
    }

    #[test]
    fn the_names_of_the_encodings_table_are_understood() {
        let legacy = |name: &str| Charset::named(name).map(Charset::label);
        assert_eq!(legacy("windows-1251"), Some("windows-1251"));
        assert_eq!(legacy("KOI8-R"), Some("KOI8-R"));
        assert_eq!(legacy("Shift_JIS"), Some("Shift_JIS"));
        assert_eq!(legacy(" ISO-8859-5 "), Some("ISO-8859-5"));
        assert_eq!(Charset::named("ISO-8859-1"), Some(Charset::Latin1), "strict, not windows-1252");
        assert_eq!(Charset::named("US-ASCII"), Some(Charset::Ascii));
        assert_eq!(Charset::named("UTF-16"), Some(Charset::Utf16Be));
        assert_eq!(Charset::named("UTF-8"), Some(Charset::Utf8));
        assert_eq!(Charset::named("UTF-8 BOM"), Some(Charset::Utf8Bom));
        assert_eq!(Charset::named("UTF-32"), None);
        assert_eq!(Charset::named("replacement"), None);
        assert_eq!(Charset::named("no-such-charset"), None);
    }

    #[test]
    fn every_choice_is_found_again_by_its_name_and_round_trips_ascii() {
        let choices = Charset::choices();
        assert!(choices.len() > 35, "{}", choices.len());
        for charset in choices {
            assert_eq!(Charset::named(charset.label()), Some(charset), "{}", charset.label());
            let bytes = charset.encode("plain text\n", false).unwrap();
            assert_eq!(charset.decode(&bytes).map(|(text, _)| text).as_deref(), Some("plain text\n"), "{charset:?}");
        }
    }

    #[test]
    fn a_legacy_file_round_trips_and_a_foreign_character_is_refused_with_its_place() {
        let cyrillic = Charset::named("windows-1251").unwrap();
        let (text, bom) = cyrillic.decode(&[0xcf, 0xf0, 0xe8, b'\n']).unwrap();
        assert_eq!((text.as_str(), bom), ("\u{41f}\u{440}\u{438}\n", false));
        assert_eq!(cyrillic.encode(&text, false).unwrap(), [0xcf, 0xf0, 0xe8, b'\n']);
        assert_eq!(
            cyrillic.encode("ok\nab\u{4e2d}\u{4e2d}", false),
            Err(Unmappable { character: '\u{4e2d}', line: 2, column: 3 })
        );
        assert_eq!(Charset::named("ISO-8859-6").unwrap().decode(&[0xa1]), None, "an undefined byte");
        let koi = Charset::named("KOI8-R").unwrap();
        assert_eq!(koi.decode(&[0xf0, 0xd2, 0xc9]).unwrap().0, "\u{41f}\u{440}\u{438}");
        let japanese = Charset::named("Shift_JIS").unwrap();
        assert_eq!(japanese.decode(&japanese.encode("\u{3042}", false).unwrap()).unwrap().0, "\u{3042}");
    }

    #[test]
    fn ascii_refuses_what_is_not_ascii() {
        assert_eq!(Charset::Ascii.decode(&[0xe9]), None);
        assert_eq!(Charset::Ascii.decode(b"a"), Some(("a".to_owned(), false)));
        assert_eq!(
            Charset::Ascii.encode("a\u{e9}", false),
            Err(Unmappable { character: '\u{e9}', line: 1, column: 2 })
        );
    }

    #[test]
    fn a_byte_order_mark_names_the_charset() {
        assert_eq!(Charset::of_bom(b"\xef\xbb\xbfa"), Some(Charset::Utf8Bom));
        assert_eq!(Charset::of_bom(&[0xff, 0xfe, b'a', 0]), Some(Charset::Utf16Le));
        assert_eq!(Charset::of_bom(&[0xfe, 0xff, 0, b'a']), Some(Charset::Utf16Be));
        assert_eq!(Charset::of_bom(b"a"), None);
        assert_eq!(Charset::Utf16Le.bom_bytes(), [0xff, 0xfe]);
        assert_eq!(Charset::Latin1.bom_bytes(), [] as [u8; 0]);
    }

    #[test]
    fn a_charset_is_written_as_its_id_in_json() {
        let json = serde_json::to_value(Charset::Utf8Bom).unwrap();
        assert_eq!(json, "utf-8-bom");
        assert_eq!(serde_json::from_value::<Charset>(json).unwrap(), Charset::Utf8Bom);
        assert_eq!(serde_json::from_str::<Charset>("\"KOI8-R\"").unwrap(), Charset::Legacy(encoding_rs::KOI8_R));
        assert!(serde_json::from_str::<Charset>("\"no\"").is_err());
    }

    #[test]
    fn latin1_holds_up_to_u_00ff_and_says_where_the_first_other_character_is() {
        let (text, bom) = Charset::Latin1.decode(&[b'c', 0xe9, b'\n']).unwrap();
        assert_eq!((text.as_str(), bom), ("c\u{e9}\n", false));
        assert_eq!(Charset::Latin1.encode(&text, false).unwrap(), [b'c', 0xe9, b'\n']);
        assert_eq!(
            Charset::Latin1.encode("ok\r\nab\u{44f}c", false),
            Err(Unmappable { character: '\u{44f}', line: 2, column: 3 })
        );
    }

    #[test]
    fn utf_8_bom_drops_the_mark_on_reading_and_writes_it_once() {
        let (text, bom) = Charset::Utf8Bom.decode(b"\xef\xbb\xbfa").unwrap();
        assert_eq!((text.as_str(), bom), ("a", true));
        assert_eq!(Charset::Utf8Bom.encode("a", false).unwrap(), b"\xef\xbb\xbfa");
        assert_eq!(Charset::Utf8Bom.decode(b"\xef\xbb\xbf\xef\xbb\xbfa"), Some(("\u{feff}a".to_owned(), true)));
        assert_eq!(Charset::Utf8Bom.encode("\u{feff}a", true).unwrap(), b"\xef\xbb\xbf\xef\xbb\xbfa");
        assert_eq!(Charset::Utf8.encode("\u{feff}a", false).unwrap(), b"\xef\xbb\xbfa", "utf-8 keeps what is there");
        assert_eq!(Charset::Utf8.decode(b"\xff"), None);
    }

    #[test]
    fn utf_16_round_trips_both_orders_with_and_without_a_mark() {
        let text = "a\u{44f}\u{1f600}\n";
        for charset in [Charset::Utf16Be, Charset::Utf16Le] {
            for bom in [false, true] {
                let bytes = charset.encode(text, bom).unwrap();
                assert_eq!(charset.decode(&bytes), Some((text.to_owned(), bom)), "{charset:?} {bom}");
            }
        }
        assert_eq!(Charset::Utf16Le.encode("a", true).unwrap(), [0xff, 0xfe, b'a', 0]);
        assert_eq!(Charset::Utf16Be.encode("a", false).unwrap(), [0, b'a']);
        assert_eq!(Charset::Utf16Le.decode(b"a"), None, "an odd length");
        assert_eq!(Charset::Utf16Le.decode(&[0x00, 0xd8]), None, "a lone surrogate");
    }

    mod properties {
        use proptest::prelude::*;

        use super::*;

        fn utf16(charset: Charset, units: &[u16]) -> Vec<u8> {
            units
                .iter()
                .flat_map(|unit| if charset == Charset::Utf16Be { unit.to_be_bytes() } else { unit.to_le_bytes() })
                .collect()
        }

        fn utf16_charset() -> impl Strategy<Value = Charset> {
            prop_oneof![Just(Charset::Utf16Be), Just(Charset::Utf16Le)]
        }

        proptest! {
            #[test]
            fn latin1_gives_back_every_byte(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
                let (text, bom) = Charset::Latin1.decode(&bytes).unwrap();
                prop_assert!(!bom);
                prop_assert_eq!(text.chars().count(), bytes.len());
                prop_assert_eq!(Charset::Latin1.encode(&text, bom).unwrap(), bytes);
            }

            #[test]
            fn utf_8_gives_back_its_bytes_and_refuses_the_rest(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
                match Charset::Utf8.decode(&bytes) {
                    Some((text, bom)) => prop_assert_eq!(Charset::Utf8.encode(&text, bom).unwrap(), bytes),
                    None => prop_assert!(std::str::from_utf8(&bytes).is_err()),
                }
            }

            #[test]
            fn utf_8_bom_gives_back_a_file_with_its_mark_and_adds_it_to_one_without(
                text in "\u{feff}?[a\r\n\u{e9}\u{44f}\u{1f600}\u{feff}]{0,16}",
                marked in any::<bool>(),
            ) {
                let mut bytes = if marked { vec![0xef, 0xbb, 0xbf] } else { Vec::new() };
                bytes.extend_from_slice(text.as_bytes());
                let (decoded, bom) = Charset::Utf8Bom.decode(&bytes).unwrap();
                let starts_marked = !marked && text.starts_with('\u{feff}');
                let expected = if starts_marked { &text['\u{feff}'.len_utf8()..] } else { &text };
                prop_assert_eq!(bom, marked || starts_marked);
                prop_assert_eq!(decoded.as_str(), expected);
                let written = Charset::Utf8Bom.encode(&decoded, bom).unwrap();
                if bom {
                    prop_assert_eq!(written, bytes);
                } else {
                    prop_assert_eq!(&written[..3], &[0xef, 0xbb, 0xbf]);
                    prop_assert_eq!(&written[3..], bytes.as_slice());
                }
            }

            #[test]
            fn utf_16_gives_back_every_valid_file(
                charset in utf16_charset(),
                text in "\u{feff}?[a\r\n\u{e9}\u{44f}\u{ffff}\u{10000}\u{1f600}\u{10ffff}\u{feff}]{0,16}",
                marked in any::<bool>(),
            ) {
                let mut units: Vec<u16> = if marked { vec![0xfeff] } else { Vec::new() };
                units.extend(text.encode_utf16());
                let bytes = utf16(charset, &units);
                let (decoded, bom) = charset.decode(&bytes).unwrap();
                let expected = if !marked && text.starts_with('\u{feff}') { &text['\u{feff}'.len_utf8()..] } else { &text };
                prop_assert_eq!(bom, marked || text.starts_with('\u{feff}'));
                prop_assert_eq!(decoded.as_str(), expected);
                prop_assert_eq!(charset.encode(&decoded, bom).unwrap(), bytes);
            }

            #[test]
            fn utf_16_refuses_lone_surrogates_and_odd_lengths(
                charset in utf16_charset(),
                before in prop::collection::vec(0x20u16..0xd800, 0..8),
                surrogate in 0xd800u16..0xe000,
                after in prop::collection::vec(0x20u16..0xd800, 0..8),
                extra in any::<u8>(),
            ) {
                let mut units = before;
                units.push(surrogate);
                units.extend(after);
                prop_assert_eq!(charset.decode(&utf16(charset, &units)), None);
                let mut odd = utf16(charset, &units[..units.len() - 1]);
                odd.push(extra);
                prop_assert_eq!(charset.decode(&odd), None);
            }

            #[test]
            fn every_charset_reads_back_what_it_writes(
                text in "[a\r\n\u{e9}\u{ff}\u{100}\u{44f}\u{1f600}]{0,24}",
                bom in any::<bool>(),
            ) {
                for charset in Charset::ALL {
                    match charset.encode(&text, bom) {
                        Ok(bytes) => {
                            let (decoded, _) = charset.decode(&bytes).unwrap();
                            prop_assert_eq!(&decoded, &text, "{:?}", charset);
                        }
                        Err(unmappable) => {
                            prop_assert_eq!(charset, Charset::Latin1);
                            let first = text.chars().find(|character| u32::from(*character) > 0xff).unwrap();
                            prop_assert_eq!(unmappable.character, first);
                            let at = text.find(first).unwrap();
                            let line = text[..at].matches('\n').count() + 1;
                            prop_assert_eq!(unmappable.line, line);
                        }
                    }
                }
            }
        }
    }
}
