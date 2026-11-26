use std::fmt::Write as _;

const MAGIC: u32 = 0xCAFE_BABE;

const PUBLIC: u16 = 0x0001;
const PRIVATE: u16 = 0x0002;
const PROTECTED: u16 = 0x0004;
const STATIC: u16 = 0x0008;
const FINAL: u16 = 0x0010;
const SYNCHRONIZED: u16 = 0x0020;
const BRIDGE: u16 = 0x0040;
const VARARGS: u16 = 0x0080;
const NATIVE: u16 = 0x0100;
const INTERFACE: u16 = 0x0200;
const ABSTRACT: u16 = 0x0400;
const SYNTHETIC: u16 = 0x1000;
const ANNOTATION: u16 = 0x2000;
const ENUM: u16 = 0x4000;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Constant {
    Utf8(String),
    Class(u16),
    Int(i32),
    Long(i64),
    Float(u32),
    Double(u64),
    String(u16),
    Other,
    Gap,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Member {
    pub access: u16,
    pub name: String,
    pub descriptor: String,
    pub signature: Option<String>,
    pub constant: Option<String>,
    pub exceptions: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ClassFile {
    pub major: u16,
    pub access: u16,
    pub name: String,
    pub super_name: Option<String>,
    pub interfaces: Vec<String>,
    pub signature: Option<String>,
    pub source_file: Option<String>,
    pub record: bool,
    pub fields: Vec<Member>,
    pub methods: Vec<Member>,
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn take(&mut self, count: usize) -> Result<&[u8], String> {
        let end = self.at.checked_add(count).filter(|end| *end <= self.bytes.len()).ok_or("truncated")?;
        let slice = &self.bytes[self.at..end];
        self.at = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, String> {
        let b = self.take(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, String> {
        let b = self.take(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok((u64::from(self.u32()?) << 32) | u64::from(self.u32()?))
    }
}

fn modified_utf8(bytes: &[u8]) -> String {
    let mut units: Vec<u16> = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let first = u16::from(bytes[at]);
        if first & 0x80 == 0 {
            units.push(first);
            at += 1;
        } else if first & 0xE0 == 0xC0 && at + 1 < bytes.len() {
            units.push(((first & 0x1F) << 6) | (u16::from(bytes[at + 1]) & 0x3F));
            at += 2;
        } else if first & 0xF0 == 0xE0 && at + 2 < bytes.len() {
            units.push(
                ((first & 0x0F) << 12) | ((u16::from(bytes[at + 1]) & 0x3F) << 6) | (u16::from(bytes[at + 2]) & 0x3F),
            );
            at += 3;
        } else {
            units.push(u16::from(b'?'));
            at += 1;
        }
    }
    String::from_utf16_lossy(&units)
}

pub fn is_class(bytes: &[u8]) -> bool {
    bytes.len() >= 10 && bytes[..4] == MAGIC.to_be_bytes()
}

pub fn parse(bytes: &[u8]) -> Result<ClassFile, String> {
    let mut reader = Reader { bytes, at: 0 };
    if reader.u32()? != MAGIC {
        return Err("not a class file".to_owned());
    }
    let _minor = reader.u16()?;
    let major = reader.u16()?;
    let count = reader.u16()?;
    let mut pool = vec![Constant::Gap];
    while pool.len() < usize::from(count) {
        let tag = reader.u8()?;
        let constant = match tag {
            1 => {
                let length = usize::from(reader.u16()?);
                Constant::Utf8(modified_utf8(reader.take(length)?))
            }
            3 => Constant::Int(reader.u32()? as i32),
            4 => Constant::Float(reader.u32()?),
            5 => Constant::Long(reader.u64()? as i64),
            6 => Constant::Double(reader.u64()?),
            7 => Constant::Class(reader.u16()?),
            8 => Constant::String(reader.u16()?),
            9..=12 | 17 | 18 => {
                reader.take(4)?;
                Constant::Other
            }
            15 => {
                reader.take(3)?;
                Constant::Other
            }
            16 | 19 | 20 => {
                reader.take(2)?;
                Constant::Other
            }
            other => return Err(format!("unknown constant tag {other}")),
        };
        let wide = matches!(constant, Constant::Long(_) | Constant::Double(_));
        pool.push(constant);
        if wide {
            pool.push(Constant::Gap);
        }
    }
    let utf8 = |index: u16| match pool.get(usize::from(index)) {
        Some(Constant::Utf8(text)) => Ok(text.clone()),
        _ => Err(format!("constant {index} is not a name")),
    };
    let class = |index: u16| match pool.get(usize::from(index)) {
        Some(Constant::Class(name)) => utf8(*name),
        _ => Err(format!("constant {index} is not a class")),
    };
    let access = reader.u16()?;
    let name = class(reader.u16()?)?;
    let super_index = reader.u16()?;
    let super_name = if super_index == 0 { None } else { Some(class(super_index)?) };
    let interfaces = (0..reader.u16()?).map(|_| reader.u16().and_then(class)).collect::<Result<Vec<_>, _>>()?;
    let members = |reader: &mut Reader<'_>| -> Result<Vec<Member>, String> {
        let count = reader.u16()?;
        let mut found = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let access = reader.u16()?;
            let name = utf8(reader.u16()?)?;
            let descriptor = utf8(reader.u16()?)?;
            let mut member = Member { access, name, descriptor, ..Member::default() };
            for _ in 0..reader.u16()? {
                let attribute = utf8(reader.u16()?)?;
                let length = reader.u32()? as usize;
                let mut body = Reader { bytes: reader.take(length)?, at: 0 };
                match attribute.as_str() {
                    "Signature" => member.signature = Some(utf8(body.u16()?)?),
                    "ConstantValue" => {
                        member.constant = constant_text(&pool, body.u16()?, &member.descriptor);
                    }
                    "Exceptions" => {
                        for _ in 0..body.u16()? {
                            member.exceptions.push(class(body.u16()?)?);
                        }
                    }
                    _ => {}
                }
            }
            found.push(member);
        }
        Ok(found)
    };
    let fields = members(&mut reader)?;
    let methods = members(&mut reader)?;
    let mut file = ClassFile { major, access, name, super_name, interfaces, fields, methods, ..ClassFile::default() };
    for _ in 0..reader.u16()? {
        let attribute = utf8(reader.u16()?)?;
        let length = reader.u32()? as usize;
        let mut body = Reader { bytes: reader.take(length)?, at: 0 };
        match attribute.as_str() {
            "SourceFile" => file.source_file = Some(utf8(body.u16()?)?),
            "Signature" => file.signature = Some(utf8(body.u16()?)?),
            "Record" => file.record = true,
            _ => {}
        }
    }
    Ok(file)
}

fn constant_text(pool: &[Constant], index: u16, descriptor: &str) -> Option<String> {
    Some(match pool.get(usize::from(index))? {
        Constant::Int(value) => match descriptor {
            "Z" => (*value != 0).to_string(),
            "C" => char::from_u32(*value as u32).map_or_else(|| value.to_string(), |c| format!("{:?}", c)),
            _ => value.to_string(),
        },
        Constant::Long(value) => format!("{value}L"),
        Constant::Float(bits) => format!("{:?}f", f32::from_bits(*bits)),
        Constant::Double(bits) => format!("{:?}", f64::from_bits(*bits)),
        Constant::String(text) => match pool.get(usize::from(*text))? {
            Constant::Utf8(text) => format!("{text:?}"),
            _ => return None,
        },
        _ => return None,
    })
}

pub fn java_version(major: u16) -> u16 {
    if major >= 49 { major - 44 } else { 1 }
}

fn simple(name: &str) -> String {
    name.replace(['/', '$'], ".")
}

fn short(name: &str) -> &str {
    let name = name.rsplit('/').next().unwrap_or(name);
    name.rsplit('$').next().unwrap_or(name)
}

struct Types<'a> {
    text: &'a [u8],
    at: usize,
}

impl Types<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.at += 1;
        Some(byte)
    }

    fn until(&mut self, stops: &[u8]) -> String {
        let start = self.at;
        while self.peek().is_some_and(|byte| !stops.contains(&byte)) {
            self.at += 1;
        }
        String::from_utf8_lossy(&self.text[start..self.at]).into_owned()
    }

    fn type_arguments(&mut self) -> Option<String> {
        if self.peek() != Some(b'<') {
            return Some(String::new());
        }
        self.next();
        let mut arguments = Vec::new();
        while self.peek()? != b'>' {
            arguments.push(match self.peek()? {
                b'*' => {
                    self.next();
                    "?".to_owned()
                }
                b'+' => {
                    self.next();
                    format!("? extends {}", self.one()?)
                }
                b'-' => {
                    self.next();
                    format!("? super {}", self.one()?)
                }
                _ => self.one()?,
            });
        }
        self.next();
        Some(format!("<{}>", arguments.join(", ")))
    }

    fn one(&mut self) -> Option<String> {
        Some(match self.next()? {
            b'B' => "byte".to_owned(),
            b'C' => "char".to_owned(),
            b'D' => "double".to_owned(),
            b'F' => "float".to_owned(),
            b'I' => "int".to_owned(),
            b'J' => "long".to_owned(),
            b'S' => "short".to_owned(),
            b'Z' => "boolean".to_owned(),
            b'V' => "void".to_owned(),
            b'[' => format!("{}[]", self.one()?),
            b'T' => {
                let name = self.until(b";");
                self.next();
                name
            }
            b'L' => {
                let mut name = simple(&self.until(b"<;."));
                name.push_str(&self.type_arguments()?);
                while self.peek() == Some(b'.') {
                    self.next();
                    name.push('.');
                    name.push_str(&self.until(b"<;."));
                    name.push_str(&self.type_arguments()?);
                }
                self.next();
                name
            }
            _ => return None,
        })
    }

    fn type_parameters(&mut self) -> Option<String> {
        if self.peek() != Some(b'<') {
            return Some(String::new());
        }
        self.next();
        let mut parameters = Vec::new();
        while self.peek()? != b'>' {
            let name = self.until(b":");
            let mut bounds = Vec::new();
            while self.peek() == Some(b':') {
                self.next();
                if self.peek() == Some(b':') {
                    continue;
                }
                bounds.push(self.one()?);
            }
            bounds.retain(|bound| bound != "java.lang.Object");
            parameters.push(if bounds.is_empty() { name } else { format!("{name} extends {}", bounds.join(" & ")) });
        }
        self.next();
        Some(format!("<{}> ", parameters.join(", ")))
    }
}

struct Method {
    type_parameters: String,
    parameters: Vec<String>,
    returns: String,
    throws: Vec<String>,
}

fn method_types(text: &str) -> Option<Method> {
    let mut types = Types { text: text.as_bytes(), at: 0 };
    let type_parameters = types.type_parameters()?;
    if types.next()? != b'(' {
        return None;
    }
    let mut parameters = Vec::new();
    while types.peek()? != b')' {
        parameters.push(types.one()?);
    }
    types.next();
    let returns = types.one()?;
    let mut throws = Vec::new();
    while types.next() == Some(b'^') {
        throws.push(types.one()?);
    }
    Some(Method { type_parameters, parameters, returns, throws })
}

fn field_type(member: &Member) -> String {
    let text = member.signature.as_deref().unwrap_or(&member.descriptor);
    Types { text: text.as_bytes(), at: 0 }.one().unwrap_or_else(|| member.descriptor.clone())
}

fn modifiers(access: u16, for_method: bool, in_interface: bool) -> String {
    let mut words = Vec::new();
    if access & PUBLIC != 0 {
        words.push("public");
    }
    if access & PROTECTED != 0 {
        words.push("protected");
    }
    if access & PRIVATE != 0 {
        words.push("private");
    }
    if access & ABSTRACT != 0 && !in_interface {
        words.push("abstract");
    }
    if access & STATIC != 0 {
        words.push("static");
    }
    if access & FINAL != 0 {
        words.push("final");
    }
    if for_method {
        if access & SYNCHRONIZED != 0 {
            words.push("synchronized");
        }
        if access & NATIVE != 0 {
            words.push("native");
        }
        if in_interface && access & (ABSTRACT | STATIC | PRIVATE) == 0 {
            words.push("default");
        }
    } else {
        if access & 0x0040 != 0 {
            words.push("volatile");
        }
        if access & 0x0080 != 0 {
            words.push("transient");
        }
    }
    words.into_iter().map(|word| format!("{word} ")).collect()
}

pub fn stub(class: &ClassFile, notice: &str) -> String {
    let mut out = String::new();
    for line in notice.lines() {
        let _ = writeln!(out, "// {line}");
    }
    let (package, _) = class.name.rsplit_once('/').unwrap_or(("", &class.name));
    if !package.is_empty() {
        let _ = writeln!(out, "\npackage {};", package.replace('/', "."));
    }
    out.push('\n');
    let interface = class.access & INTERFACE != 0;
    let name = short(&class.name);
    let kind = if class.access & ANNOTATION != 0 {
        "@interface"
    } else if interface {
        "interface"
    } else if class.access & ENUM != 0 {
        "enum"
    } else if class.record {
        "record"
    } else {
        "class"
    };
    let access = if interface { class.access & !ABSTRACT } else { class.access };
    let mut head = modifiers(access, false, false);
    if class.access & ENUM != 0 || class.record {
        head = head.replace("final ", "");
    }
    let mut types = class.signature.as_deref().map(|text| Types { text: text.as_bytes(), at: 0 });
    let parameters = types.as_mut().and_then(Types::type_parameters).unwrap_or_default();
    let (super_type, interfaces) = match types.as_mut() {
        Some(types) => {
            let super_type = types.one();
            let mut found = Vec::new();
            while let Some(one) = types.one() {
                found.push(one);
            }
            (super_type, found)
        }
        None => (class.super_name.as_deref().map(simple), class.interfaces.iter().map(|name| simple(name)).collect()),
    };
    let _ = write!(out, "{head}{kind} {name}{}", parameters.trim_end());
    let ordinary_super = |name: &str| !matches!(name, "java.lang.Object" | "java.lang.Enum" | "java.lang.Record");
    if let Some(super_type) = super_type.filter(|name| ordinary_super(name.split('<').next().unwrap_or(name))) {
        let _ = write!(out, " extends {super_type}");
    }
    let interfaces: Vec<String> =
        interfaces.into_iter().filter(|name| name != "java.lang.annotation.Annotation").collect();
    if !interfaces.is_empty() {
        let word = if interface { "extends" } else { "implements" };
        let _ = write!(out, " {word} {}", interfaces.join(", "));
    }
    out.push_str(" {\n");
    let visible = |member: &&Member| member.access & (SYNTHETIC | BRIDGE) == 0;
    let constants: Vec<&Member> = class.fields.iter().filter(|field| field.access & ENUM != 0).collect();
    if !constants.is_empty() {
        let names: Vec<&str> = constants.iter().map(|field| field.name.as_str()).collect();
        let _ = writeln!(out, "    {};\n", names.join(",\n    "));
    }
    for field in class.fields.iter().filter(visible).filter(|field| field.access & ENUM == 0) {
        let access = if interface { field.access & !(PUBLIC | STATIC | FINAL) } else { field.access };
        let _ = write!(out, "    {}{} {}", modifiers(access, false, false), field_type(field), field.name);
        if let Some(value) = &field.constant {
            let _ = write!(out, " = {value}");
        }
        out.push_str(";\n");
    }
    if !class.fields.is_empty() {
        out.push('\n');
    }
    for method in class.methods.iter().filter(visible) {
        if method.name == "<clinit>" {
            continue;
        }
        let text = method.signature.as_deref().unwrap_or(&method.descriptor);
        let Some(types) = method_types(text).or_else(|| method_types(&method.descriptor)) else { continue };
        let access = if interface { method.access & !PUBLIC } else { method.access };
        let declared = if method.name == "<init>" {
            name.to_owned()
        } else {
            format!("{}{} {}", types.type_parameters, types.returns, method.name)
        };
        let count = types.parameters.len();
        let parameters: Vec<String> = types
            .parameters
            .iter()
            .enumerate()
            .map(|(index, kind)| {
                if index + 1 == count && method.access & VARARGS != 0 && kind.ends_with("[]") {
                    format!("{}... arg{index}", &kind[..kind.len() - 2])
                } else {
                    format!("{kind} arg{index}")
                }
            })
            .collect();
        let mut throws = types.throws;
        if throws.is_empty() {
            throws = method.exceptions.iter().map(|name| simple(name)).collect();
        }
        let throws = if throws.is_empty() { String::new() } else { format!(" throws {}", throws.join(", ")) };
        let _ = write!(out, "    {}{declared}({}){throws}", modifiers(access, true, interface), parameters.join(", "));
        if method.access & (ABSTRACT | NATIVE) != 0 {
            out.push_str(";\n");
        } else {
            out.push_str(" { /* compiled code */ }\n");
        }
    }
    out.push_str("}\n");
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn minimal(name: &str, source: &str) -> Vec<u8> {
        let mut b = Builder::default();
        let this = b.class(name);
        let object = b.class("java/lang/Object");
        let mut head = (PUBLIC | 0x0020).to_be_bytes().to_vec();
        head.extend(this.to_be_bytes());
        head.extend(object.to_be_bytes());
        head.extend(0u16.to_be_bytes());
        let mut members = 0u16.to_be_bytes().to_vec();
        members.extend(1u16.to_be_bytes());
        members.extend(b.member(PUBLIC, "run", "()V", &[]));
        let file = b.utf8(source);
        let mut attributes = 1u16.to_be_bytes().to_vec();
        attributes.extend(b.utf8("SourceFile").to_be_bytes());
        attributes.extend(2u32.to_be_bytes());
        attributes.extend(file.to_be_bytes());
        b.finish(&head, &members, &attributes)
    }

    #[derive(Default)]
    struct Builder {
        pool: Vec<Vec<u8>>,
    }

    impl Builder {
        fn utf8(&mut self, text: &str) -> u16 {
            let mut entry = vec![1];
            entry.extend((text.len() as u16).to_be_bytes());
            entry.extend(text.as_bytes());
            self.add(entry)
        }

        fn class(&mut self, name: &str) -> u16 {
            let name = self.utf8(name);
            let mut entry = vec![7];
            entry.extend(name.to_be_bytes());
            self.add(entry)
        }

        fn string(&mut self, text: &str) -> u16 {
            let text = self.utf8(text);
            let mut entry = vec![8];
            entry.extend(text.to_be_bytes());
            self.add(entry)
        }

        fn long(&mut self, value: i64) -> u16 {
            let mut entry = vec![5];
            entry.extend(value.to_be_bytes());
            let index = self.add(entry);
            self.pool.push(Vec::new());
            index
        }

        fn add(&mut self, entry: Vec<u8>) -> u16 {
            self.pool.push(entry);
            self.pool.len() as u16
        }

        fn member(&mut self, access: u16, name: &str, descriptor: &str, attributes: &[(&str, Vec<u8>)]) -> Vec<u8> {
            let mut out = access.to_be_bytes().to_vec();
            out.extend(self.utf8(name).to_be_bytes());
            out.extend(self.utf8(descriptor).to_be_bytes());
            out.extend((attributes.len() as u16).to_be_bytes());
            for (attribute, body) in attributes {
                out.extend(self.utf8(attribute).to_be_bytes());
                out.extend((body.len() as u32).to_be_bytes());
                out.extend(body);
            }
            out
        }

        fn finish(self, head: &[u8], members: &[u8], attributes: &[u8]) -> Vec<u8> {
            let mut out = MAGIC.to_be_bytes().to_vec();
            out.extend([0, 0, 0, 65]);
            out.extend((self.pool.len() as u16 + 1).to_be_bytes());
            for entry in &self.pool {
                out.extend(entry);
            }
            out.extend(head);
            out.extend(members);
            out.extend(attributes);
            out
        }
    }

    fn sample() -> Vec<u8> {
        let mut b = Builder::default();
        let this = b.class("com/acme/Box");
        let object = b.class("java/lang/Object");
        let comparable = b.class("java/lang/Comparable");
        let failure = b.class("java/io/IOException");
        let limit = b.long(42);
        let greeting = b.string("hi \"there\"");
        let mut head = (PUBLIC | FINAL | 0x0020).to_be_bytes().to_vec();
        head.extend(this.to_be_bytes());
        head.extend(object.to_be_bytes());
        head.extend(1u16.to_be_bytes());
        head.extend(comparable.to_be_bytes());
        let mut members = 3u16.to_be_bytes().to_vec();
        members.extend(b.member(
            PUBLIC | STATIC | FINAL,
            "LIMIT",
            "J",
            &[("ConstantValue", limit.to_be_bytes().to_vec())],
        ));
        members.extend(b.member(
            PRIVATE | STATIC | FINAL,
            "GREETING",
            "Ljava/lang/String;",
            &[("ConstantValue", greeting.to_be_bytes().to_vec())],
        ));
        let signature = b.utf8("Ljava/util/List<+Ljava/lang/Number;>;");
        members.extend(b.member(
            PRIVATE,
            "items",
            "Ljava/util/List;",
            &[("Signature", signature.to_be_bytes().to_vec())],
        ));
        members.extend(4u16.to_be_bytes());
        members.extend(b.member(PUBLIC, "<init>", "(I[Ljava/lang/String;)V", &[]));
        let mut exceptions = 1u16.to_be_bytes().to_vec();
        exceptions.extend(failure.to_be_bytes());
        let generic = b.utf8("<T::Ljava/lang/Comparable<TT;>;>(Ljava/util/Map<Ljava/lang/String;TT;>;[TT;)TT;");
        members.extend(b.member(
            PUBLIC | STATIC | VARARGS,
            "max",
            "(Ljava/util/Map;[Ljava/lang/Comparable;)Ljava/lang/Comparable;",
            &[("Signature", generic.to_be_bytes().to_vec()), ("Exceptions", exceptions)],
        ));
        members.extend(b.member(PUBLIC | NATIVE, "hash", "()I", &[]));
        members.extend(b.member(PUBLIC | SYNTHETIC | BRIDGE, "compareTo", "(Ljava/lang/Object;)I", &[]));
        let source = b.utf8("Box.java");
        let mut attributes = 1u16.to_be_bytes().to_vec();
        attributes.extend(b.utf8("SourceFile").to_be_bytes());
        attributes.extend(2u32.to_be_bytes());
        attributes.extend(source.to_be_bytes());
        b.finish(&head, &members, &attributes)
    }

    #[test]
    fn a_class_file_reads_into_its_declaration() {
        let bytes = sample();
        assert!(is_class(&bytes) && !is_class(b"PK\x03\x04 not a class"));
        let class = parse(&bytes).unwrap();
        assert_eq!(class.name, "com/acme/Box");
        assert_eq!(class.source_file.as_deref(), Some("Box.java"));
        assert_eq!(java_version(class.major), 21);
        assert_eq!(class.fields.len(), 3);
        assert_eq!(class.methods.len(), 4);
        assert!(parse(&bytes[..bytes.len() - 3]).is_err(), "a cut file is refused");
    }

    #[test]
    fn the_decompiled_view_declares_every_visible_member_without_code() {
        let text = stub(&parse(&sample()).unwrap(), "Decompiled\nsecond line");
        let expected = "// Decompiled
// second line

package com.acme;

public final class Box implements java.lang.Comparable {
    public static final long LIMIT = 42L;
    private static final java.lang.String GREETING = \"hi \\\"there\\\"\";
    private java.util.List<? extends java.lang.Number> items;

    public Box(int arg0, java.lang.String[] arg1) { /* compiled code */ }
    public static <T extends java.lang.Comparable<T>> T max(java.util.Map<java.lang.String, T> arg0, T... arg1) throws java.io.IOException { /* compiled code */ }
    public native int hash();
}
";
        assert_eq!(text, expected);
    }

    #[test]
    fn interfaces_keep_default_methods_and_drop_implied_modifiers() {
        let mut b = Builder::default();
        let this = b.class("a/Shape");
        let object = b.class("java/lang/Object");
        let mut head = (PUBLIC | INTERFACE | ABSTRACT).to_be_bytes().to_vec();
        head.extend(this.to_be_bytes());
        head.extend(object.to_be_bytes());
        head.extend(0u16.to_be_bytes());
        let mut members = 0u16.to_be_bytes().to_vec();
        members.extend(2u16.to_be_bytes());
        members.extend(b.member(PUBLIC | ABSTRACT, "area", "()D", &[]));
        members.extend(b.member(PUBLIC, "name", "()Ljava/lang/String;", &[]));
        let bytes = b.finish(&head, &members, &0u16.to_be_bytes());
        let text = stub(&parse(&bytes).unwrap(), "x");
        assert!(text.contains("public interface Shape {\n"), "{text}");
        assert!(text.contains("    double area();\n"), "{text}");
        assert!(text.contains("    default java.lang.String name() { /* compiled code */ }\n"), "{text}");
    }
}
