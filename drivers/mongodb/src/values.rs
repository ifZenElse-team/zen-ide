use bson::spec::BinarySubtype;
use bson::{Bson, Document};
use serde_json::{Value as Json, json};
use zen_db_driver_kit::value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Columns {
    names: Vec<String>,
    types: Vec<Option<&'static str>>,
}

impl Columns {
    pub(crate) fn of<'a>(documents: impl IntoIterator<Item = &'a Document>) -> Self {
        let mut columns = Self::default();
        for document in documents {
            for (name, value) in document {
                let at = match columns.names.iter().position(|known| known == name) {
                    Some(at) => at,
                    None => {
                        let at = if name == "_id" { 0 } else { columns.names.len() };
                        columns.names.insert(at, name.clone());
                        columns.types.insert(at, None);
                        at
                    }
                };
                if matches!(value, Bson::Null | Bson::Undefined) {
                    continue;
                }
                let kind = type_name(value);
                columns.types[at] = match columns.types[at] {
                    None => Some(kind),
                    Some(known) if known == kind => Some(known),
                    Some(_) => Some("mixed"),
                };
            }
        }
        columns
    }

    pub(crate) fn wire(&self) -> Vec<Json> {
        self.names.iter().zip(&self.types).map(|(name, kind)| value::column(name, kind.unwrap_or("null"))).collect()
    }

    pub(crate) fn row(&self, document: &Document) -> Json {
        Json::Array(self.names.iter().map(|name| document.get(name).map_or_else(value::null, cell)).collect())
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

pub(crate) fn type_name(value: &Bson) -> &'static str {
    match value {
        Bson::Double(_) => "double",
        Bson::String(_) => "string",
        Bson::Array(_) => "array",
        Bson::Document(_) => "object",
        Bson::Boolean(_) => "bool",
        Bson::Null => "null",
        Bson::RegularExpression(_) => "regex",
        Bson::JavaScriptCode(_) => "javascript",
        Bson::JavaScriptCodeWithScope(_) => "javascriptWithScope",
        Bson::Int32(_) => "int",
        Bson::Int64(_) => "long",
        Bson::Timestamp(_) => "timestamp",
        Bson::Binary(_) => "binData",
        Bson::ObjectId(_) => "objectId",
        Bson::DateTime(_) => "date",
        Bson::Symbol(_) => "symbol",
        Bson::Decimal128(_) => "decimal",
        Bson::Undefined => "undefined",
        Bson::MaxKey => "maxKey",
        Bson::MinKey => "minKey",
        Bson::DbPointer(_) => "dbPointer",
    }
}

pub(crate) fn shell_text(value: &Bson) -> String {
    match value {
        Bson::ObjectId(id) => format!("ObjectId(\"{id}\")"),
        Bson::DateTime(date) => format!("ISODate(\"{}\")", iso(*date)),
        Bson::Int64(number) => format!("NumberLong({number})"),
        Bson::Decimal128(number) => format!("NumberDecimal(\"{number}\")"),
        Bson::Timestamp(stamp) => format!("Timestamp({}, {})", stamp.time, stamp.increment),
        Bson::RegularExpression(regex) => format!("/{}/{}", regex.pattern, regex.options),
        Bson::MinKey => "MinKey()".into(),
        Bson::MaxKey => "MaxKey()".into(),
        Bson::Undefined => "undefined".into(),
        other => other.clone().into_relaxed_extjson().to_string(),
    }
}

fn iso(date: bson::DateTime) -> String {
    date.try_to_rfc3339_string().unwrap_or_else(|_| date.timestamp_millis().to_string())
}

pub(crate) fn cell(value: &Bson) -> Json {
    match value {
        Bson::Null => value::null(),
        Bson::Boolean(flag) => value::boolean(*flag),
        Bson::Int32(number) => value::integer(i64::from(*number)),
        Bson::Int64(number) => value::integer(*number),
        Bson::Double(number) => value::float(*number),
        Bson::Decimal128(number) => value::decimal(number.to_string()),
        Bson::String(text) | Bson::Symbol(text) => value::text(text.clone()),
        Bson::DateTime(date) => value::temporal(iso(*date), "date"),
        Bson::Binary(binary) if binary.subtype == BinarySubtype::Uuid && binary.bytes.len() == 16 => {
            match binary.to_uuid() {
                Ok(uuid) => value::uuid(uuid.to_string()),
                Err(_) => value::bytes(&binary.bytes),
            }
        }
        Bson::Binary(binary) => value::bytes(&binary.bytes),
        Bson::Document(_) | Bson::Array(_) => value::json_text(json_text(value)),
        other => value::other(&shell_text(other), type_name(other)),
    }
}

pub(crate) fn json_text(value: &Bson) -> String {
    value.clone().into_relaxed_extjson().to_string()
}

pub(crate) fn reply_row(mut reply: Document) -> Document {
    for key in ["ok", "$clusterTime", "operationTime", "$gleStats", "lastCommittedOpTime", "electionId"] {
        reply.remove(key);
    }
    reply
}

pub(crate) fn table(documents: &[Document]) -> (Json, Json) {
    let columns = Columns::of(documents);
    let rows: Vec<Json> = documents.iter().map(|document| columns.row(document)).collect();
    (json!(columns.wire()), Json::Array(rows))
}

#[cfg(test)]
mod tests;
