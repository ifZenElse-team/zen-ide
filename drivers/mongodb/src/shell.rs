use bson::oid::ObjectId;
use bson::raw::CString;
use bson::spec::BinarySubtype;
use bson::{Binary, Bson, DateTime, Decimal128, Document, Regex, Timestamp, doc};
use zen_db_driver_kit::Failure;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    Cursor,
    Reply,
    Count,
    CountFromCursor,
    Distinct(String),
    Databases,
    Value,
    Written,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Statement {
    Use(String),
    Run(Run),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Run {
    pub(crate) database: Option<String>,
    pub(crate) command: Document,
    pub(crate) shape: Shape,
    pub(crate) writes: bool,
    pub(crate) limit: Option<usize>,
}

impl Run {
    fn new(command: Document, shape: Shape, writes: bool) -> Self {
        Self { database: None, command, shape, writes, limit: None }
    }
}

const READ_COMMANDS: &[&str] = &[
    "find",
    "aggregate",
    "count",
    "distinct",
    "listcollections",
    "listindexes",
    "listdatabases",
    "dbstats",
    "collstats",
    "buildinfo",
    "serverstatus",
    "hello",
    "ismaster",
    "ping",
    "connectionstatus",
    "explain",
    "hostinfo",
    "getparameter",
    "getcmdlineopts",
    "currentop",
    "getlog",
    "replsetgetstatus",
    "usersinfo",
    "rolesinfo",
    "listcommands",
    "validate",
    "datasize",
    "top",
    "getmore",
];

pub(crate) fn command_writes(command: &Document) -> bool {
    let Some(name) = command.keys().next() else { return true };
    let name = name.to_ascii_lowercase();
    if !READ_COMMANDS.contains(&name.as_str()) {
        return true;
    }
    if name == "aggregate" {
        return pipeline_writes(command.get_array("pipeline").map(Vec::as_slice).unwrap_or_default());
    }
    if name == "explain" {
        return command.get_document("explain").is_ok_and(command_writes);
    }
    false
}

fn pipeline_writes(pipeline: &[Bson]) -> bool {
    pipeline.iter().any(
        |stage| matches!(stage, Bson::Document(stage) if stage.contains_key("$out") || stage.contains_key("$merge")),
    )
}

pub(crate) fn parse(statement: &str) -> Result<Statement, Failure> {
    let mut parser = Parser::new(statement);
    let parsed = parser.statement()?;
    parser.skip_space();
    while parser.eat(';') {
        parser.skip_space();
    }
    if !parser.done() {
        return Err(parser.error("unexpected text after the statement"));
    }
    Ok(parsed)
}

#[cfg(test)]
pub(crate) fn parse_value(text: &str) -> Result<Bson, Failure> {
    let mut parser = Parser::new(text);
    let value = parser.value()?;
    parser.skip_space();
    if !parser.done() {
        return Err(parser.error("unexpected text after the value"));
    }
    Ok(value)
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn new(text: &str) -> Self {
        Self { chars: text.chars().collect(), at: 0 }
    }

    fn error(&self, message: &str) -> Failure {
        Failure::syntax(message.to_owned(), self.at + 1)
    }

    fn done(&self) -> bool {
        self.at >= self.chars.len()
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn skip_space(&mut self) {
        loop {
            match (self.peek(), self.chars.get(self.at + 1)) {
                (Some(c), _) if c.is_whitespace() => self.at += 1,
                (Some('/'), Some('/')) => {
                    while self.peek().is_some_and(|c| c != '\n') {
                        self.at += 1;
                    }
                }
                (Some('/'), Some('*')) => {
                    self.at += 2;
                    while !self.done() && !(self.peek() == Some('*') && self.chars.get(self.at + 1) == Some(&'/')) {
                        self.at += 1;
                    }
                    self.at = (self.at + 2).min(self.chars.len());
                }
                _ => return,
            }
        }
    }

    fn eat(&mut self, c: char) -> bool {
        self.skip_space();
        if self.peek() == Some(c) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, c: char) -> Result<(), Failure> {
        if self.eat(c) { Ok(()) } else { Err(self.error(&format!("`{c}` expected"))) }
    }

    fn identifier(&mut self) -> Option<String> {
        self.skip_space();
        let start = self.at;
        while self.peek().is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '$') {
            self.at += 1;
        }
        (self.at > start).then(|| self.chars[start..self.at].iter().collect())
    }

    fn word(&mut self) -> Option<String> {
        self.skip_space();
        let start = self.at;
        while self.peek().is_some_and(|c| !c.is_whitespace() && c != ';') {
            self.at += 1;
        }
        (self.at > start).then(|| self.chars[start..self.at].iter().collect())
    }

    fn statement(&mut self) -> Result<Statement, Failure> {
        self.skip_space();
        if self.peek() == Some('{') {
            let command = self.document()?;
            let writes = command_writes(&command);
            let shape = reply_shape(&command_name(&command));
            return Ok(Statement::Run(Run::new(command, shape, writes)));
        }
        let start = self.at;
        let first = self.identifier().ok_or_else(|| self.error("a statement starts with `db.`, `show` or `use`"))?;
        match first.as_str() {
            "show" => {
                let what = self.word().ok_or_else(|| self.error("`show dbs` or `show collections`"))?;
                match what.as_str() {
                    "dbs" | "databases" => {
                        let mut run = Run::new(
                            doc! { "listDatabases": 1, "nameOnly": false, "authorizedDatabases": true },
                            Shape::Databases,
                            false,
                        );
                        run.database = Some("admin".into());
                        Ok(Statement::Run(run))
                    }
                    "collections" | "tables" => Ok(Statement::Run(Run::new(
                        doc! { "listCollections": 1, "nameOnly": true, "authorizedCollections": true },
                        Shape::Cursor,
                        false,
                    ))),
                    _ => Err(self.error("`show dbs` or `show collections`")),
                }
            }
            "use" => {
                let name = self.word().ok_or_else(|| self.error("`use` names a database"))?;
                Ok(Statement::Use(name))
            }
            "db" => self.database_statement(None),
            _ => {
                self.at = start;
                Err(self.error("a statement starts with `db.`, `show` or `use`"))
            }
        }
    }

    fn database_statement(&mut self, database: Option<String>) -> Result<Statement, Failure> {
        let mut collection: Vec<String> = Vec::new();
        loop {
            if self.eat('[') {
                let name = self.string_value()?;
                self.expect(']')?;
                collection.push(name);
                continue;
            }
            if !self.eat('.') {
                return Err(self.error("a method call expected, such as `.find()`"));
            }
            let name = self.identifier().ok_or_else(|| self.error("a name expected after `.`"))?;
            self.skip_space();
            if self.peek() != Some('(') {
                collection.push(name);
                continue;
            }
            let at = self.at;
            let arguments = self.arguments()?;
            if collection.is_empty() {
                match name.as_str() {
                    "getSiblingDB" => {
                        let target =
                            string_argument(&arguments, 0).ok_or_else(|| self.error_at(at, "a database name"))?;
                        return self.database_statement(Some(target));
                    }
                    "getCollection" => {
                        collection.push(
                            string_argument(&arguments, 0).ok_or_else(|| self.error_at(at, "a collection name"))?,
                        );
                        continue;
                    }
                    _ => {
                        let mut run =
                            database_method(&name, arguments).map_err(|message| self.error_at(at, &message))?;
                        if run.database.is_none() {
                            run.database = database;
                        }
                        return Ok(Statement::Run(run));
                    }
                }
            }
            let collection = collection.join(".");
            let mut run =
                collection_method(&collection, &name, arguments).map_err(|message| self.error_at(at, &message))?;
            self.modifiers(&mut run)?;
            run.database = run.database.or(database);
            return Ok(Statement::Run(run));
        }
    }

    fn error_at(&self, at: usize, message: &str) -> Failure {
        Failure::syntax(message.to_owned(), at + 1)
    }

    fn modifiers(&mut self, run: &mut Run) -> Result<(), Failure> {
        loop {
            self.skip_space();
            let save = self.at;
            if !self.eat('.') {
                return Ok(());
            }
            let Some(name) = self.identifier() else {
                self.at = save;
                return Ok(());
            };
            let at = self.at;
            let arguments = self.arguments()?;
            modifier(run, &name, arguments).map_err(|message| self.error_at(at, &message))?;
        }
    }

    fn arguments(&mut self) -> Result<Vec<Bson>, Failure> {
        self.expect('(')?;
        let mut values = Vec::new();
        if self.eat(')') {
            return Ok(values);
        }
        loop {
            values.push(self.value()?);
            if self.eat(')') {
                return Ok(values);
            }
            self.expect(',')?;
            if self.eat(')') {
                return Ok(values);
            }
        }
    }

    fn document(&mut self) -> Result<Document, Failure> {
        match self.value()? {
            Bson::Document(document) => Ok(document),
            _ => Err(self.error("a document `{ … }` expected")),
        }
    }

    fn string_value(&mut self) -> Result<String, Failure> {
        match self.value()? {
            Bson::String(text) => Ok(text),
            _ => Err(self.error("a string expected")),
        }
    }

    fn value(&mut self) -> Result<Bson, Failure> {
        self.skip_space();
        match self.peek() {
            Some('{') => self.object(),
            Some('[') => {
                self.at += 1;
                let mut items = Vec::new();
                if self.eat(']') {
                    return Ok(Bson::Array(items));
                }
                loop {
                    items.push(self.value()?);
                    if self.eat(']') {
                        return Ok(Bson::Array(items));
                    }
                    self.expect(',')?;
                    if self.eat(']') {
                        return Ok(Bson::Array(items));
                    }
                }
            }
            Some('"' | '\'') => Ok(Bson::String(self.string()?)),
            Some('/') => self.regex(),
            Some(c) if c.is_ascii_digit() || c == '-' || c == '+' || c == '.' => self.number(),
            Some(_) => self.named(),
            None => Err(self.error("a value expected")),
        }
    }

    fn object(&mut self) -> Result<Bson, Failure> {
        self.expect('{')?;
        let mut document = Document::new();
        if self.eat('}') {
            return Ok(Bson::Document(document));
        }
        loop {
            self.skip_space();
            let key = match self.peek() {
                Some('"' | '\'') => self.string()?,
                _ => {
                    let start = self.at;
                    while self.peek().is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | '$' | '.' | '-')) {
                        self.at += 1;
                    }
                    if self.at == start {
                        return Err(self.error("a field name expected"));
                    }
                    self.chars[start..self.at].iter().collect()
                }
            };
            self.expect(':')?;
            let value = self.value()?;
            document.insert(key, value);
            if self.eat('}') {
                return Ok(extended(document));
            }
            self.expect(',')?;
            if self.eat('}') {
                return Ok(extended(document));
            }
        }
    }

    fn string(&mut self) -> Result<String, Failure> {
        let quote = self.peek().unwrap_or('"');
        let start = self.at;
        self.at += 1;
        let mut text = String::new();
        loop {
            let Some(c) = self.peek() else {
                self.at = start;
                return Err(self.error("the string is not closed"));
            };
            self.at += 1;
            match c {
                c if c == quote => return Ok(text),
                '\\' => {
                    let escaped = self.peek().ok_or_else(|| self.error("the string is not closed"))?;
                    self.at += 1;
                    match escaped {
                        'n' => text.push('\n'),
                        't' => text.push('\t'),
                        'r' => text.push('\r'),
                        'b' => text.push('\u{8}'),
                        'f' => text.push('\u{c}'),
                        '0' => text.push('\0'),
                        'u' => {
                            let hex: String = self.chars.get(self.at..self.at + 4).unwrap_or_default().iter().collect();
                            let code =
                                u32::from_str_radix(&hex, 16).map_err(|_| self.error("`\\u` needs four hex digits"))?;
                            self.at += 4;
                            text.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        other => text.push(other),
                    }
                }
                c => text.push(c),
            }
        }
    }

    fn regex(&mut self) -> Result<Bson, Failure> {
        self.at += 1;
        let mut pattern = String::new();
        let mut in_class = false;
        loop {
            let c = self.peek().ok_or_else(|| self.error("the regular expression is not closed"))?;
            self.at += 1;
            match c {
                '\\' => {
                    pattern.push(c);
                    if let Some(next) = self.peek() {
                        pattern.push(next);
                        self.at += 1;
                    }
                }
                '[' => {
                    in_class = true;
                    pattern.push(c);
                }
                ']' => {
                    in_class = false;
                    pattern.push(c);
                }
                '/' if !in_class => break,
                c => pattern.push(c),
            }
        }
        let mut options: Vec<char> = Vec::new();
        while let Some(c) = self.peek().filter(char::is_ascii_alphabetic) {
            options.push(c);
            self.at += 1;
        }
        options.sort_unstable();
        regex(&pattern, &options.into_iter().collect::<String>()).map_err(|message| self.error(&message))
    }

    fn number(&mut self) -> Result<Bson, Failure> {
        let start = self.at;
        if matches!(self.peek(), Some('-' | '+')) {
            self.at += 1;
        }
        if self.identifier_is("Infinity") {
            let negative = self.chars[start] == '-';
            return Ok(Bson::Double(if negative { f64::NEG_INFINITY } else { f64::INFINITY }));
        }
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_'))
            || (matches!(self.peek(), Some('-' | '+')) && matches!(self.chars.get(self.at - 1), Some('e' | 'E')))
        {
            self.at += 1;
        }
        let text: String = self.chars[start..self.at].iter().filter(|c| **c != '_').collect();
        number(&text).ok_or_else(|| {
            self.at = start;
            self.error(&format!("`{text}` is not a number"))
        })
    }

    fn identifier_is(&mut self, word: &str) -> bool {
        let end = self.at + word.chars().count();
        if self.chars.get(self.at..end).is_some_and(|chars| chars.iter().copied().eq(word.chars())) {
            self.at = end;
            true
        } else {
            false
        }
    }

    fn named(&mut self) -> Result<Bson, Failure> {
        let start = self.at;
        let mut name = self.identifier().ok_or_else(|| self.error("a value expected"))?;
        if name == "new" {
            name = self.identifier().ok_or_else(|| self.error("a type expected after `new`"))?;
        }
        let simple = match name.as_str() {
            "true" => Some(Bson::Boolean(true)),
            "false" => Some(Bson::Boolean(false)),
            "null" => Some(Bson::Null),
            "undefined" => Some(Bson::Undefined),
            "Infinity" => Some(Bson::Double(f64::INFINITY)),
            "NaN" => Some(Bson::Double(f64::NAN)),
            "MinKey" => Some(Bson::MinKey),
            "MaxKey" => Some(Bson::MaxKey),
            _ => None,
        };
        self.skip_space();
        if let Some(value) = simple {
            if self.peek() == Some('(') {
                self.arguments()?;
            }
            return Ok(value);
        }
        if self.peek() != Some('(') {
            self.at = start;
            return Err(self.error(&format!("`{name}` is not a value")));
        }
        let arguments = self.arguments()?;
        constructor(&name, &arguments).map_err(|message| {
            self.at = start;
            self.error(&message)
        })
    }
}

fn number(text: &str) -> Option<Bson> {
    let lower = text.to_ascii_lowercase();
    let (negative, digits) = match lower.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, lower.strip_prefix('+').unwrap_or(&lower)),
    };
    let radix = [("0x", 16), ("0o", 8), ("0b", 2)].into_iter().find(|(prefix, _)| digits.starts_with(prefix));
    if let Some((prefix, radix)) = radix {
        let value = i64::from_str_radix(&digits[prefix.len()..], radix).ok()?;
        return Some(integer(if negative { -value } else { value }));
    }
    if digits.chars().all(|c| c.is_ascii_digit())
        && !digits.is_empty()
        && let Ok(value) = text.trim_start_matches('+').parse::<i64>()
    {
        return Some(integer(value));
    }
    text.trim_start_matches('+').parse::<f64>().ok().map(Bson::Double)
}

fn integer(value: i64) -> Bson {
    i32::try_from(value).map_or(Bson::Int64(value), Bson::Int32)
}

const EXTENDED_KEYS: &[&str] = &[
    "$oid",
    "$date",
    "$numberInt",
    "$numberLong",
    "$numberDouble",
    "$numberDecimal",
    "$binary",
    "$uuid",
    "$timestamp",
    "$regularExpression",
    "$symbol",
    "$code",
    "$minKey",
    "$maxKey",
    "$undefined",
];

fn extended(document: Document) -> Bson {
    let wrapper =
        document.len() == 1 && document.keys().next().is_some_and(|key| EXTENDED_KEYS.contains(&key.as_str()));
    if !wrapper {
        return Bson::Document(document);
    }
    let canonical = Bson::Document(document.clone()).into_canonical_extjson();
    Bson::try_from(canonical).unwrap_or(Bson::Document(document))
}

fn regex(pattern: &str, options: &str) -> Result<Bson, String> {
    let pattern = CString::try_from(pattern.to_owned()).map_err(|error| error.to_string())?;
    let options = CString::try_from(options.to_owned()).map_err(|error| error.to_string())?;
    Ok(Bson::RegularExpression(Regex { pattern, options }))
}

fn string_argument(arguments: &[Bson], index: usize) -> Option<String> {
    arguments.get(index).and_then(Bson::as_str).map(str::to_owned)
}

fn int_of(value: &Bson) -> Option<i64> {
    match value {
        Bson::Int32(value) => Some(i64::from(*value)),
        Bson::Int64(value) => Some(*value),
        Bson::Double(value) if value.fract() == 0.0 => Some(*value as i64),
        Bson::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn constructor(name: &str, arguments: &[Bson]) -> Result<Bson, String> {
    let text = || string_argument(arguments, 0);
    match name {
        "ObjectId" => match text() {
            Some(hex) => {
                ObjectId::parse_str(&hex).map(Bson::ObjectId).map_err(|_| format!("`{hex}` is not an ObjectId"))
            }
            None => Ok(Bson::ObjectId(ObjectId::new())),
        },
        "ISODate" | "Date" => match arguments.first() {
            None => Ok(Bson::DateTime(DateTime::now())),
            Some(Bson::String(text)) => date(text).map(Bson::DateTime).ok_or_else(|| format!("`{text}` is not a date")),
            Some(value) => int_of(value)
                .map(|millis| Bson::DateTime(DateTime::from_millis(millis)))
                .ok_or_else(|| "a date expected".into()),
        },
        "NumberLong" | "Long" => {
            arguments.first().and_then(int_of).map(Bson::Int64).ok_or_else(|| "an integer expected".into())
        }
        "NumberInt" | "Int32" => arguments
            .first()
            .and_then(int_of)
            .and_then(|value| i32::try_from(value).ok())
            .map(Bson::Int32)
            .ok_or_else(|| "a 32-bit integer expected".into()),
        "NumberDecimal" | "Decimal128" => {
            let source = match arguments.first() {
                Some(Bson::String(text)) => text.clone(),
                Some(Bson::Int32(value)) => value.to_string(),
                Some(Bson::Int64(value)) => value.to_string(),
                Some(Bson::Double(value)) => value.to_string(),
                _ => return Err("a decimal expected".into()),
            };
            source.parse::<Decimal128>().map(Bson::Decimal128).map_err(|_| format!("`{source}` is not a decimal"))
        }
        "Double" => match arguments.first() {
            Some(Bson::Double(value)) => Ok(Bson::Double(*value)),
            Some(value) => {
                int_of(value).map(|value| Bson::Double(value as f64)).ok_or_else(|| "a number expected".into())
            }
            None => Err("a number expected".into()),
        },
        "UUID" => {
            let hex = text().ok_or("a UUID expected")?;
            let uuid = bson::Uuid::parse_str(&hex).map_err(|_| format!("`{hex}` is not a UUID"))?;
            Ok(Bson::Binary(Binary::from(uuid)))
        }
        "BinData" => {
            let subtype = arguments
                .first()
                .and_then(int_of)
                .and_then(|value| u8::try_from(value).ok())
                .ok_or("a subtype expected")?;
            let data = string_argument(arguments, 1).ok_or("Base64 data expected")?;
            let bytes = base64_decode(&data).ok_or("the data is not Base64")?;
            Ok(Bson::Binary(Binary { subtype: BinarySubtype::from(subtype), bytes }))
        }
        "Timestamp" => {
            let time = arguments
                .first()
                .and_then(int_of)
                .and_then(|value| u32::try_from(value).ok())
                .ok_or("seconds expected")?;
            let increment = arguments.get(1).and_then(int_of).and_then(|value| u32::try_from(value).ok()).unwrap_or(0);
            Ok(Bson::Timestamp(Timestamp { time, increment }))
        }
        "RegExp" => {
            let pattern = text().ok_or("a pattern expected")?;
            let mut options: Vec<char> = string_argument(arguments, 1).unwrap_or_default().chars().collect();
            options.sort_unstable();
            regex(&pattern, &options.into_iter().collect::<String>())
        }
        other => Err(format!("`{other}(…)` is not a value the console knows")),
    }
}

fn base64_decode(text: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(text).ok()
}

fn date(text: &str) -> Option<DateTime> {
    let text = text.trim();
    let candidates = [text.to_owned(), format!("{}Z", text.replace(' ', "T")), format!("{text}T00:00:00Z")];
    candidates.iter().find_map(|candidate| DateTime::parse_rfc3339_str(candidate).ok())
}

fn document_argument(arguments: &[Bson], index: usize) -> Result<Option<Document>, String> {
    match arguments.get(index) {
        None | Some(Bson::Null | Bson::Undefined) => Ok(None),
        Some(Bson::Document(document)) => Ok(Some(document.clone())),
        Some(_) => Err(format!("argument {} must be a document", index + 1)),
    }
}

fn required_document(arguments: &[Bson], index: usize, what: &str) -> Result<Document, String> {
    document_argument(arguments, index)?.ok_or_else(|| format!("{what} expected"))
}

fn command_name(command: &Document) -> String {
    command.keys().next().map(|name| name.to_ascii_lowercase()).unwrap_or_default()
}

fn reply_shape(name: &str) -> Shape {
    match name {
        "find" | "aggregate" | "listcollections" | "listindexes" => Shape::Cursor,
        "listdatabases" => Shape::Databases,
        "insert" | "update" | "delete" => Shape::Written,
        _ => Shape::Reply,
    }
}

fn database_method(name: &str, arguments: Vec<Bson>) -> Result<Run, String> {
    let read = |command: Document| Ok(Run::new(command, Shape::Reply, false));
    match name {
        "runCommand" | "adminCommand" => {
            let command = match arguments.into_iter().next() {
                Some(Bson::Document(command)) => command,
                Some(Bson::String(name)) => doc! { name: 1 },
                _ => return Err("a command document expected".into()),
            };
            let shape = reply_shape(&command_name(&command));
            let writes = command_writes(&command);
            let mut run = Run::new(command, shape, writes);
            if name == "adminCommand" {
                run.database = Some("admin".into());
            }
            Ok(run)
        }
        "getCollectionNames" => Ok(Run::new(
            doc! { "listCollections": 1, "nameOnly": true, "authorizedCollections": true },
            Shape::Cursor,
            false,
        )),
        "getCollectionInfos" => {
            let filter = document_argument(&arguments, 0)?.unwrap_or_default();
            Ok(Run::new(doc! { "listCollections": 1, "filter": filter }, Shape::Cursor, false))
        }
        "stats" => read(doc! { "dbStats": 1 }),
        "serverStatus" => read(doc! { "serverStatus": 1 }),
        "hostInfo" => read(doc! { "hostInfo": 1 }),
        "version" | "serverBuildInfo" => read(doc! { "buildInfo": 1 }),
        "hello" | "isMaster" => read(doc! { "hello": 1 }),
        "currentOp" => {
            let mut run = Run::new(doc! { "currentOp": 1 }, Shape::Reply, false);
            run.database = Some("admin".into());
            Ok(run)
        }
        "createCollection" | "createView" => {
            let collection = string_argument(&arguments, 0).ok_or("a collection name expected")?;
            let mut command = doc! { "create": collection };
            if name == "createView" {
                command.insert("viewOn", string_argument(&arguments, 1).ok_or("the source collection expected")?);
                command.insert("pipeline", arguments.get(2).cloned().unwrap_or(Bson::Array(Vec::new())));
            } else if let Some(options) = document_argument(&arguments, 1)? {
                command.extend(options);
            }
            Ok(Run::new(command, Shape::Reply, true))
        }
        "dropDatabase" => Ok(Run::new(doc! { "dropDatabase": 1 }, Shape::Reply, true)),
        "aggregate" => {
            let pipeline = arguments.into_iter().next().ok_or("a pipeline expected")?;
            let Bson::Array(stages) = pipeline else { return Err("the pipeline must be an array".into()) };
            let writes = pipeline_writes(&stages);
            Ok(Run::new(doc! { "aggregate": 1, "pipeline": stages, "cursor": {} }, Shape::Cursor, writes))
        }
        other => Err(format!("`db.{other}()` is not a method the console knows")),
    }
}

fn index_name(keys: &Document) -> String {
    keys.iter()
        .map(|(field, direction)| {
            let direction = match direction {
                Bson::String(text) => text.clone(),
                other => int_of(other).map_or_else(|| other.to_string(), |value| value.to_string()),
            };
            format!("{field}_{direction}")
        })
        .collect::<Vec<_>>()
        .join("_")
}

fn update_entry(filter: Document, update: Bson, options: Option<Document>, multi: bool) -> Document {
    let mut entry = doc! { "q": filter, "u": update, "multi": multi };
    if let Some(options) = options {
        for key in ["upsert", "arrayFilters", "collation", "hint"] {
            if let Some(value) = options.get(key) {
                entry.insert(key, value.clone());
            }
        }
    }
    entry
}

fn with_options(mut command: Document, options: Option<Document>, keys: &[&str]) -> Document {
    if let Some(options) = options {
        for key in keys {
            if let Some(value) = options.get(*key) {
                command.insert(*key, value.clone());
            }
        }
    }
    command
}

fn collection_method(collection: &str, name: &str, arguments: Vec<Bson>) -> Result<Run, String> {
    let write = |command: Document| Ok(Run::new(command, Shape::Written, true));
    match name {
        "find" | "findOne" => {
            let mut command = doc! { "find": collection };
            if let Some(filter) = document_argument(&arguments, 0)? {
                command.insert("filter", filter);
            }
            if let Some(projection) = document_argument(&arguments, 1)? {
                command.insert("projection", projection);
            }
            let mut run = Run::new(command, Shape::Cursor, false);
            if name == "findOne" {
                run.command.insert("limit", 1);
                run.command.insert("singleBatch", true);
                run.limit = Some(1);
            }
            Ok(run)
        }
        "aggregate" => {
            let stages = match arguments.first() {
                Some(Bson::Array(stages)) => stages.clone(),
                None => Vec::new(),
                Some(_) => return Err("the pipeline must be an array".into()),
            };
            let writes = pipeline_writes(&stages);
            let command = doc! { "aggregate": collection, "pipeline": stages, "cursor": {} };
            let command = with_options(
                command,
                document_argument(&arguments, 1)?,
                &["allowDiskUse", "collation", "hint", "maxTimeMS", "comment", "let"],
            );
            Ok(Run::new(command, Shape::Cursor, writes))
        }
        "countDocuments" => {
            let filter = document_argument(&arguments, 0)?.unwrap_or_default();
            let mut pipeline = vec![Bson::Document(doc! { "$match": filter })];
            if let Some(options) = document_argument(&arguments, 1)? {
                if let Some(skip) = options.get("skip") {
                    pipeline.push(Bson::Document(doc! { "$skip": skip.clone() }));
                }
                if let Some(limit) = options.get("limit") {
                    pipeline.push(Bson::Document(doc! { "$limit": limit.clone() }));
                }
            }
            pipeline.push(Bson::Document(doc! { "$group": { "_id": 1, "n": { "$sum": 1 } } }));
            Ok(Run::new(
                doc! { "aggregate": collection, "pipeline": pipeline, "cursor": {} },
                Shape::CountFromCursor,
                false,
            ))
        }
        "estimatedDocumentCount" => Ok(Run::new(doc! { "count": collection }, Shape::Count, false)),
        "count" => {
            let mut command = doc! { "count": collection };
            if let Some(filter) = document_argument(&arguments, 0)? {
                command.insert("query", filter);
            }
            Ok(Run::new(command, Shape::Count, false))
        }
        "distinct" => {
            let field = string_argument(&arguments, 0).ok_or("a field name expected")?;
            let mut command = doc! { "distinct": collection, "key": field.clone() };
            if let Some(filter) = document_argument(&arguments, 1)? {
                command.insert("query", filter);
            }
            Ok(Run::new(command, Shape::Distinct(field), false))
        }
        "insertOne" | "insert" | "insertMany" => {
            let documents: Vec<Bson> = match arguments.first() {
                Some(Bson::Document(document)) if name != "insertMany" => {
                    vec![Bson::Document(with_id(document.clone()))]
                }
                Some(Bson::Array(documents)) if name != "insertOne" => documents
                    .iter()
                    .map(|document| match document {
                        Bson::Document(document) => Ok(Bson::Document(with_id(document.clone()))),
                        _ => Err("every element must be a document".to_owned()),
                    })
                    .collect::<Result<_, _>>()?,
                _ => {
                    return Err(if name == "insertMany" {
                        "an array of documents expected"
                    } else {
                        "a document expected"
                    }
                    .into());
                }
            };
            let ordered =
                document_argument(&arguments, 1)?.and_then(|options| options.get_bool("ordered").ok()).unwrap_or(true);
            write(doc! { "insert": collection, "documents": documents, "ordered": ordered })
        }
        "updateOne" | "updateMany" | "replaceOne" | "update" => {
            let filter = required_document(&arguments, 0, "a filter")?;
            let update = arguments.get(1).cloned().ok_or("an update expected")?;
            let replacing = name == "replaceOne";
            match &update {
                Bson::Document(document) if replacing && document.keys().any(|key| key.starts_with('$')) => {
                    return Err("a replacement cannot use update operators".into());
                }
                Bson::Document(document)
                    if !replacing && name != "update" && !document.keys().all(|key| key.starts_with('$')) =>
                {
                    return Err("an update uses operators such as `$set`; use replaceOne to replace a document".into());
                }
                Bson::Document(_) => {}
                Bson::Array(_) if !replacing => {}
                _ => return Err("an update must be a document or a pipeline".into()),
            }
            let options = document_argument(&arguments, 2)?;
            let multi = name == "updateMany"
                || (name == "update"
                    && options.as_ref().is_some_and(|options| options.get_bool("multi").unwrap_or(false)));
            write(doc! { "update": collection, "updates": [update_entry(filter, update, options, multi)] })
        }
        "deleteOne" | "deleteMany" | "remove" => {
            let filter = required_document(&arguments, 0, "a filter")?;
            let limit = i32::from(
                name == "deleteOne"
                    || (name == "remove" && arguments.get(1).is_some_and(|value| matches!(value, Bson::Boolean(true)))),
            );
            write(doc! { "delete": collection, "deletes": [{ "q": filter, "limit": limit }] })
        }
        "findOneAndUpdate" | "findOneAndReplace" | "findOneAndDelete" => {
            let filter = required_document(&arguments, 0, "a filter")?;
            let mut command = doc! { "findAndModify": collection, "query": filter };
            let options_at = if name == "findOneAndDelete" {
                command.insert("remove", true);
                1
            } else {
                command.insert("update", arguments.get(1).cloned().ok_or("an update expected")?);
                2
            };
            if let Some(options) = document_argument(&arguments, options_at)? {
                if options.get_bool("returnNewDocument").unwrap_or(false)
                    || options.get_str("returnDocument").is_ok_and(|when| when == "after")
                {
                    command.insert("new", true);
                }
                if let Some(projection) = options.get("projection") {
                    command.insert("fields", projection.clone());
                }
                command =
                    with_options(command, Some(options), &["sort", "upsert", "arrayFilters", "collation", "hint"]);
            }
            Ok(Run::new(command, Shape::Value, true))
        }
        "createIndex" => {
            let keys = required_document(&arguments, 0, "the index keys")?;
            let mut index = doc! { "key": keys.clone(), "name": index_name(&keys) };
            if let Some(options) = document_argument(&arguments, 1)? {
                index.extend(options);
            }
            Ok(Run::new(doc! { "createIndexes": collection, "indexes": [index] }, Shape::Reply, true))
        }
        "createIndexes" => {
            let Some(Bson::Array(list)) = arguments.first() else {
                return Err("an array of index keys expected".into());
            };
            let indexes = list
                .iter()
                .map(|keys| match keys {
                    Bson::Document(keys) => Ok(Bson::Document(doc! { "key": keys.clone(), "name": index_name(keys) })),
                    _ => Err("every index is a document of keys".to_owned()),
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Run::new(doc! { "createIndexes": collection, "indexes": indexes }, Shape::Reply, true))
        }
        "dropIndex" => {
            let index = arguments.into_iter().next().ok_or("an index name or its keys expected")?;
            Ok(Run::new(doc! { "dropIndexes": collection, "index": index }, Shape::Reply, true))
        }
        "dropIndexes" => Ok(Run::new(doc! { "dropIndexes": collection, "index": "*" }, Shape::Reply, true)),
        "getIndexes" | "getIndices" => Ok(Run::new(doc! { "listIndexes": collection }, Shape::Cursor, false)),
        "drop" => Ok(Run::new(doc! { "drop": collection }, Shape::Reply, true)),
        "stats" => Ok(Run::new(doc! { "collStats": collection }, Shape::Reply, false)),
        "renameCollection" => {
            let to = string_argument(&arguments, 0).ok_or("the new name expected")?;
            Ok(Run::new(doc! { "renameCollection": collection, "to": to }, Shape::Reply, true))
        }
        other => Err(format!("`.{other}()` is not a collection method the console knows")),
    }
}

fn with_id(mut document: Document) -> Document {
    if !document.contains_key("_id") {
        let mut with = doc! { "_id": ObjectId::new() };
        with.extend(document);
        document = with;
    }
    document
}

fn modifier(run: &mut Run, name: &str, arguments: Vec<Bson>) -> Result<(), String> {
    let finding = run.command.contains_key("find");
    let number =
        |arguments: &[Bson]| arguments.first().and_then(int_of).ok_or_else(|| format!("`.{name}()` takes a number"));
    match name {
        "toArray" | "pretty" | "itcount" => Ok(()),
        "sort" | "projection" | "hint" | "collation" | "min" | "max" if finding => {
            let key = if name == "projection" { "projection" } else { name };
            let value = arguments.into_iter().next().ok_or_else(|| format!("`.{name}()` takes a document"))?;
            run.command.insert(key, value);
            Ok(())
        }
        "limit" if finding => {
            let limit = number(&arguments)?;
            run.command.insert("limit", limit.abs());
            run.limit = usize::try_from(limit.abs()).ok().filter(|limit| *limit > 0);
            Ok(())
        }
        "skip" | "batchSize" | "maxTimeMS" if finding => {
            run.command.insert(name, number(&arguments)?);
            Ok(())
        }
        "comment" if finding => {
            run.command.insert("comment", arguments.into_iter().next().unwrap_or(Bson::Null));
            Ok(())
        }
        "count" | "size" if finding => {
            let mut command = doc! { "count": run.command.get_str("find").unwrap_or_default() };
            if let Ok(filter) = run.command.get_document("filter") {
                command.insert("query", filter.clone());
            }
            if name == "size" {
                for key in ["skip", "limit"] {
                    if let Some(value) = run.command.get(key) {
                        command.insert(key, value.clone());
                    }
                }
            }
            run.command = command;
            run.shape = Shape::Count;
            run.limit = None;
            Ok(())
        }
        "explain" => {
            let verbosity = string_argument(&arguments, 0).unwrap_or_else(|| "queryPlanner".into());
            let inner = std::mem::take(&mut run.command);
            run.command = doc! { "explain": inner, "verbosity": verbosity };
            run.shape = Shape::Reply;
            run.writes = command_writes(&run.command);
            run.limit = None;
            Ok(())
        }
        other => Err(format!("`.{other}()` cannot follow this call")),
    }
}

#[cfg(test)]
mod tests;
