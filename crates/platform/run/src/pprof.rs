use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use zen_i18n::t;

use crate::profile::{Profile, ProfileFrame, ProfileMetric, ProfileSample};

const MAX_DECODED: u64 = 1 << 30;
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];

#[derive(Clone, Copy)]
enum Field<'a> {
    Number(u64),
    Bytes(&'a [u8]),
    Fixed,
}

struct Fields<'a> {
    bytes: &'a [u8],
}

impl<'a> Fields<'a> {
    fn varint(&mut self) -> Option<u64> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let (byte, rest) = self.bytes.split_first()?;
            self.bytes = rest;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }

    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        if length > self.bytes.len() {
            return None;
        }
        let (head, rest) = self.bytes.split_at(length);
        self.bytes = rest;
        Some(head)
    }

    fn next_field(&mut self) -> Option<(u64, Field<'a>)> {
        let tag = self.varint()?;
        let field = match tag & 7 {
            0 => Field::Number(self.varint()?),
            1 => self.take(8).map(|_| Field::Fixed)?,
            2 => {
                let length = usize::try_from(self.varint()?).ok()?;
                Field::Bytes(self.take(length)?)
            }
            5 => self.take(4).map(|_| Field::Fixed)?,
            _ => return None,
        };
        Some((tag >> 3, field))
    }
}

fn each<'a>(bytes: &'a [u8], mut visit: impl FnMut(u64, Field<'a>) -> Option<()>) -> Option<()> {
    let mut fields = Fields { bytes };
    while !fields.bytes.is_empty() {
        let (number, field) = fields.next_field()?;
        visit(number, field)?;
    }
    Some(())
}

fn numbers(field: Field<'_>, into: &mut Vec<u64>) -> Option<()> {
    match field {
        Field::Number(value) => into.push(value),
        Field::Bytes(bytes) => {
            let mut packed = Fields { bytes };
            while !packed.bytes.is_empty() {
                into.push(packed.varint()?);
            }
        }
        Field::Fixed => return None,
    }
    Some(())
}

fn number(field: Field<'_>) -> Option<u64> {
    match field {
        Field::Number(value) => Some(value),
        _ => None,
    }
}

fn message(field: Field<'_>) -> Option<&[u8]> {
    match field {
        Field::Bytes(bytes) => Some(bytes),
        _ => None,
    }
}

#[derive(Default)]
struct RawLocation {
    address: u64,
    lines: Vec<u64>,
}

#[derive(Default)]
struct RawFunction {
    name: usize,
    file: usize,
    line: u64,
}

#[derive(Default)]
struct Raw<'a> {
    metrics: Vec<(usize, usize)>,
    samples: Vec<(Vec<u64>, Vec<u64>)>,
    locations: HashMap<u64, RawLocation>,
    functions: HashMap<u64, RawFunction>,
    strings: Vec<&'a [u8]>,
    default_metric: usize,
    duration_ns: u64,
}

fn index(field: Field<'_>) -> Option<usize> {
    usize::try_from(number(field)?).ok()
}

fn metric(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut parts = (0, 0);
    each(bytes, |tag, field| {
        match tag {
            1 => parts.0 = index(field)?,
            2 => parts.1 = index(field)?,
            _ => {}
        }
        Some(())
    })?;
    Some(parts)
}

fn sample(bytes: &[u8]) -> Option<(Vec<u64>, Vec<u64>)> {
    let (mut locations, mut values) = (Vec::new(), Vec::new());
    each(bytes, |tag, field| match tag {
        1 => numbers(field, &mut locations),
        2 => numbers(field, &mut values),
        _ => Some(()),
    })?;
    Some((locations, values))
}

fn location(bytes: &[u8]) -> Option<(u64, RawLocation)> {
    let (mut id, mut raw) = (0, RawLocation::default());
    each(bytes, |tag, field| {
        match tag {
            1 => id = number(field)?,
            3 => raw.address = number(field)?,
            4 => each(message(field)?, |tag, field| {
                if tag == 1 {
                    raw.lines.push(number(field)?);
                }
                Some(())
            })?,
            _ => {}
        }
        Some(())
    })?;
    Some((id, raw))
}

fn function(bytes: &[u8]) -> Option<(u64, RawFunction)> {
    let (mut id, mut raw) = (0, RawFunction::default());
    each(bytes, |tag, field| {
        match tag {
            1 => id = number(field)?,
            2 => raw.name = index(field)?,
            4 => raw.file = index(field)?,
            5 => raw.line = number(field)?,
            _ => {}
        }
        Some(())
    })?;
    Some((id, raw))
}

fn raw(bytes: &[u8]) -> Option<Raw<'_>> {
    let mut raw = Raw::default();
    let mut default_name = 0;
    each(bytes, |tag, field| {
        match tag {
            1 => raw.metrics.push(metric(message(field)?)?),
            2 => raw.samples.push(sample(message(field)?)?),
            4 => {
                let (id, entry) = location(message(field)?)?;
                raw.locations.insert(id, entry);
            }
            5 => {
                let (id, entry) = function(message(field)?)?;
                raw.functions.insert(id, entry);
            }
            6 => raw.strings.push(message(field)?),
            10 => raw.duration_ns = number(field)?,
            14 => default_name = index(field)?,
            _ => {}
        }
        Some(())
    })?;
    let last = raw.metrics.len().checked_sub(1)?;
    raw.default_metric =
        raw.metrics.iter().position(|(name, _)| default_name != 0 && *name == default_name).unwrap_or(last);
    Some(raw)
}

struct Frames<'a, 'b, R> {
    raw: &'b Raw<'a>,
    resolve: R,
    frames: Vec<ProfileFrame>,
    by_function: HashMap<u64, u32>,
    by_address: HashMap<u64, u32>,
}

impl<R: Fn(&str) -> PathBuf> Frames<'_, '_, R> {
    fn text(&self, index: usize) -> Option<String> {
        self.raw.strings.get(index).map(|bytes| String::from_utf8_lossy(bytes).into_owned())
    }

    fn add(&mut self, frame: ProfileFrame) -> u32 {
        self.frames.push(frame);
        (self.frames.len() - 1) as u32
    }

    fn of_function(&mut self, id: u64) -> Option<u32> {
        if let Some(frame) = self.by_function.get(&id) {
            return Some(*frame);
        }
        let entry = self.raw.functions.get(&id)?;
        let file = self.text(entry.file)?;
        let file = (!file.is_empty()).then(|| (self.resolve)(&file));
        let frame = ProfileFrame { function: self.text(entry.name)?, file, line: u32::try_from(entry.line).ok()? };
        let frame = self.add(frame);
        self.by_function.insert(id, frame);
        Some(frame)
    }

    fn of_address(&mut self, address: u64) -> u32 {
        if let Some(frame) = self.by_address.get(&address) {
            return *frame;
        }
        let frame = self.add(ProfileFrame { function: format!("{address:#x}"), file: None, line: 0 });
        self.by_address.insert(address, frame);
        frame
    }

    fn stack(&mut self, locations: &[u64]) -> Option<Vec<u32>> {
        let mut stack = Vec::with_capacity(locations.len());
        let raw = self.raw;
        for id in locations.iter().rev() {
            let location = raw.locations.get(id)?;
            if location.lines.is_empty() {
                stack.push(self.of_address(location.address));
            }
            for function in location.lines.iter().rev() {
                stack.push(self.of_function(*function)?);
            }
        }
        Some(stack)
    }
}

fn decoded(raw: &Raw<'_>, resolve: impl Fn(&str) -> PathBuf) -> Option<Profile> {
    let text = |index: usize| raw.strings.get(index).map(|bytes| String::from_utf8_lossy(bytes).into_owned());
    let metrics = raw
        .metrics
        .iter()
        .map(|(name, unit)| Some(ProfileMetric { name: text(*name)?, unit: text(*unit)? }))
        .collect::<Option<Vec<_>>>()?;
    let mut frames =
        Frames { raw, resolve, frames: Vec::new(), by_function: HashMap::new(), by_address: HashMap::new() };
    let mut samples = Vec::with_capacity(raw.samples.len());
    for (locations, values) in &raw.samples {
        if values.len() != metrics.len() {
            return None;
        }
        let stack = frames.stack(locations)?;
        samples.push(ProfileSample { stack, values: values.iter().map(|value| *value as i64).collect() });
    }
    let frames = frames.frames;
    Some(Profile { metrics, default_metric: raw.default_metric, frames, samples, duration_ns: raw.duration_ns })
}

fn inflated(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoded = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .take(MAX_DECODED + 1)
        .read_to_end(&mut decoded)
        .map_err(|_| t!("run-pprof-not-a-profile"))?;
    if decoded.len() as u64 > MAX_DECODED {
        return Err(t!("run-pprof-too-large"));
    }
    Ok(decoded)
}

pub fn parse(bytes: &[u8], resolve: impl Fn(&str) -> PathBuf) -> Result<Profile, String> {
    let unpacked;
    let bytes = if bytes.starts_with(&GZIP_MAGIC) {
        unpacked = inflated(bytes)?;
        &unpacked[..]
    } else {
        bytes
    };
    raw(bytes).and_then(|raw| decoded(&raw, resolve)).ok_or_else(|| t!("run-pprof-not-a-profile"))
}

pub fn read(path: &Path, resolve: impl Fn(&str) -> PathBuf) -> Result<Profile, String> {
    let failed = |error: std::io::Error| t!("run-pprof-read", error = error.to_string());
    if std::fs::metadata(path).map_err(failed)?.len() > MAX_DECODED {
        return Err(t!("run-pprof-too-large"));
    }
    parse(&std::fs::read(path).map_err(failed)?, resolve)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn varint(mut value: u64, into: &mut Vec<u8>) {
        while value >= 0x80 {
            into.push(value as u8 | 0x80);
            value >>= 7;
        }
        into.push(value as u8);
    }

    fn number(tag: u64, value: u64, into: &mut Vec<u8>) {
        varint(tag << 3, into);
        varint(value, into);
    }

    fn bytes(tag: u64, value: &[u8], into: &mut Vec<u8>) {
        varint(tag << 3 | 2, into);
        varint(value.len() as u64, into);
        into.extend_from_slice(value);
    }

    fn packed(tag: u64, values: &[u64], into: &mut Vec<u8>) {
        let mut body = Vec::new();
        values.iter().for_each(|value| varint(*value, &mut body));
        bytes(tag, &body, into);
    }

    fn fields(write: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut body = Vec::new();
        write(&mut body);
        body
    }

    const STRINGS: [&str; 8] = ["", "samples", "count", "cpu", "nanoseconds", "main.main", "main.go", "main.work"];

    fn profile() -> Vec<u8> {
        fields(|out| {
            for (name, unit) in [(1, 2), (3, 4)] {
                bytes(
                    1,
                    &fields(|m| {
                        number(1, name, m);
                        number(2, unit, m);
                    }),
                    out,
                );
            }
            bytes(
                2,
                &fields(|s| {
                    packed(1, &[20, 10], s);
                    packed(2, &[2, 2_000], s);
                }),
                out,
            );
            bytes(
                2,
                &fields(|s| {
                    number(1, 10, s);
                    number(2, 1, s);
                    number(2, 1_000, s);
                }),
                out,
            );
            bytes(
                2,
                &fields(|s| {
                    packed(1, &[30], s);
                    packed(2, &[1, 500], s);
                }),
                out,
            );
            for (id, function) in [(10, 1), (20, 2)] {
                bytes(
                    4,
                    &fields(|l| {
                        number(1, id, l);
                        bytes(4, &fields(|line| number(1, function, line)), l);
                    }),
                    out,
                );
            }
            bytes(
                4,
                &fields(|l| {
                    number(1, 30, l);
                    number(3, 0xbeef, l);
                }),
                out,
            );
            for (id, name, line) in [(1, 5, 3), (2, 7, 9)] {
                bytes(
                    5,
                    &fields(|f| {
                        number(1, id, f);
                        number(2, name, f);
                        number(4, 6, f);
                        number(5, line, f);
                    }),
                    out,
                );
            }
            for text in STRINGS {
                bytes(6, text.as_bytes(), out);
            }
            number(10, 1_500_000_000, out);
        })
    }

    fn project(file: &str) -> PathBuf {
        PathBuf::from("/work").join(file)
    }

    #[test]
    fn a_profile_gives_its_metrics_frames_and_stacks_from_the_root() {
        let profile = parse(&profile(), project).unwrap();
        assert_eq!(profile.metrics[1], ProfileMetric { name: "cpu".to_owned(), unit: "nanoseconds".to_owned() });
        assert_eq!(profile.default_metric, 1, "the last metric without a default");
        assert_eq!(profile.duration_ns, 1_500_000_000);
        let names: Vec<&str> =
            profile.samples[0].stack.iter().map(|frame| profile.frames[*frame as usize].function.as_str()).collect();
        assert_eq!(names, ["main.main", "main.work"]);
        assert_eq!(profile.samples[0].values, [2, 2_000]);
        assert_eq!(profile.samples[1].values, [1, 1_000], "values that are not packed");
        let work = &profile.frames[profile.samples[0].stack[1] as usize];
        assert_eq!((work.file.as_deref(), work.line), (Some(project("main.go").as_path()), 9));
        assert_eq!(profile.frames[profile.samples[2].stack[0] as usize].function, "0xbeef");
    }

    #[test]
    fn a_gzipped_profile_reads_like_a_plain_one() {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&profile()).unwrap();
        let gzipped = encoder.finish().unwrap();
        assert_eq!(parse(&gzipped, project).unwrap(), parse(&profile(), project).unwrap());
    }

    #[test]
    fn the_default_sample_type_chooses_the_metric() {
        let mut body = profile();
        number(14, 1, &mut body);
        assert_eq!(parse(&body, project).unwrap().default_metric, 0);
    }

    #[test]
    fn inlined_calls_are_frames_of_their_own_outermost_first() {
        let body = fields(|out| {
            bytes(1, &fields(|m| number(1, 1, m)), out);
            bytes(
                2,
                &fields(|s| {
                    number(1, 1, s);
                    number(2, 1, s);
                }),
                out,
            );
            bytes(
                4,
                &fields(|l| {
                    number(1, 1, l);
                    bytes(4, &fields(|line| number(1, 2, line)), l);
                    bytes(4, &fields(|line| number(1, 1, line)), l);
                }),
                out,
            );
            for (id, name) in [(1, 2), (2, 3)] {
                bytes(
                    5,
                    &fields(|f| {
                        number(1, id, f);
                        number(2, name, f);
                    }),
                    out,
                );
            }
            for text in ["", "n", "outer", "inner"] {
                bytes(6, text.as_bytes(), out);
            }
        });
        let profile = parse(&body, project).unwrap();
        let names: Vec<&str> =
            profile.samples[0].stack.iter().map(|frame| profile.frames[*frame as usize].function.as_str()).collect();
        assert_eq!(names, ["outer", "inner"]);
    }

    #[test]
    fn a_broken_file_is_refused_and_never_panics() {
        let whole = profile();
        for length in 0..whole.len() {
            let _ = parse(&whole[..length], project);
        }
        for byte in 0..whole.len() {
            let mut changed = whole.clone();
            changed[byte] ^= 0xff;
            let _ = parse(&changed, project);
        }
        assert!(parse(b"", project).is_err());
        assert!(parse(b"not a profile", project).is_err());
        assert!(parse(&[0x1f, 0x8b, 0, 0], project).is_err());
    }

    #[test]
    fn a_sample_that_names_an_unknown_location_is_refused() {
        let mut body = profile();
        bytes(
            2,
            &fields(|s| {
                number(1, 99, s);
                packed(2, &[1, 1], s);
            }),
            &mut body,
        );
        assert!(parse(&body, project).is_err());
    }
}
