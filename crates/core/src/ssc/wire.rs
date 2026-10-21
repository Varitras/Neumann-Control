//! SSC v1 wire format: paths, request lines and reply parsing.

use std::fmt;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Number, Value};

const PATH_SEPARATOR: char = '/';
const OSC_KEY: &str = "osc";
const XID_KEY: &str = "xid";
const ERROR_KEY: &str = "error";
const DESCRIPTION_KEY: &str = "desc";

/// The largest request the speakers take, in JSON tokens: every object, array, key and scalar.
/// Measured on a KH 120 II (2026-10-20): 29 tokens taken, 31 and more refused with 413
/// "request too long", whatever the byte length (855 bytes in 29 tokens were taken).
pub const MAX_REQUEST_TOKENS: usize = 29;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WireError {
    InvalidPath,
    NonFinite,
    Malformed,
    NotAnObject,
}

impl fmt::Display for WireError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            WireError::InvalidPath => "invalid SSC path",
            WireError::NonFinite => "number is not finite",
            WireError::Malformed => "malformed JSON line",
            WireError::NotAnObject => "line is not a JSON object",
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for WireError {}

/// An address such as `audio/out/level`; never empty, no empty segments.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SscPath(Vec<String>);

impl SscPath {
    pub fn parse(text: &str) -> Result<SscPath, WireError> {
        let segments: Vec<String> = text.split(PATH_SEPARATOR).map(str::to_owned).collect();
        if segments.iter().any(String::is_empty) {
            return Err(WireError::InvalidPath);
        }
        Ok(SscPath(segments))
    }

    pub fn segments(&self) -> &[String] {
        &self.0
    }

    /// The path one level deeper; `segment` is a leaf name of the catalog, never empty.
    pub fn child(&self, segment: &str) -> SscPath {
        let mut segments = self.0.clone();
        segments.push(segment.to_owned());
        SscPath(segments)
    }
}

impl fmt::Display for SscPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0.join(&PATH_SEPARATOR.to_string()))
    }
}

impl Serialize for SscPath {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for SscPath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<SscPath, D::Error> {
        let text = String::deserialize(deserializer)?;
        SscPath::parse(&text).map_err(de::Error::custom)
    }
}

/// A device error: the `[code, {"desc": text}]` pair of an `osc/error` reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceError {
    pub code: i64,
    pub description: String,
}

/// Merges `incoming` into `target`: objects recursively, anything else is replaced.
pub(crate) fn merge(target: &mut Value, incoming: Value) {
    match (target, incoming) {
        (Value::Object(existing), Value::Object(additions)) => {
            for (key, value) in additions {
                match existing.get_mut(&key) {
                    Some(slot) => merge(slot, value),
                    None => {
                        existing.insert(key, value);
                    }
                }
            }
        }
        (slot, value) => *slot = value,
    }
}

/// A JSON value whose repeated object keys are merged instead of dropped: a device can
/// send two top-level `osc` members in one line, and a standard parser keeps only the last.
struct MergedValue(Value);

impl<'de> Deserialize<'de> for MergedValue {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<MergedValue, D::Error> {
        deserializer.deserialize_any(MergedVisitor)
    }
}

struct MergedVisitor;

impl<'de> Visitor<'de> for MergedVisitor {
    type Value = MergedValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<MergedValue, E> {
        Ok(MergedValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<MergedValue, E> {
        Ok(MergedValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<MergedValue, E> {
        Ok(MergedValue(Value::Number(value.into())))
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<MergedValue, E> {
        let number = Number::from_f64(value).ok_or_else(|| E::custom("non-finite number"))?;
        Ok(MergedValue(Value::Number(number)))
    }

    fn visit_str<E>(self, value: &str) -> Result<MergedValue, E> {
        Ok(MergedValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<MergedValue, E> {
        Ok(MergedValue(Value::String(value)))
    }

    fn visit_unit<E>(self) -> Result<MergedValue, E> {
        Ok(MergedValue(Value::Null))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<MergedValue, A::Error> {
        let mut items = Vec::new();
        while let Some(item) = sequence.next_element::<MergedValue>()? {
            items.push(item.0);
        }
        Ok(MergedValue(Value::Array(items)))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut entries: A) -> Result<MergedValue, A::Error> {
        let mut object = Value::Object(Map::new());
        while let Some((key, value)) = entries.next_entry::<String, MergedValue>()? {
            merge(&mut object, Value::Object(Map::from_iter([(key, value.0)])));
        }
        Ok(MergedValue(object))
    }
}

/// Parses one reply line into a JSON object, merging duplicate keys deeply.
pub fn parse_line(line: &[u8]) -> Result<Value, WireError> {
    let text = std::str::from_utf8(line).map_err(|_| WireError::Malformed)?;
    let merged: MergedValue = serde_json::from_str(text).map_err(|_| WireError::Malformed)?;
    match merged.0 {
        Value::Object(_) => Ok(merged.0),
        _ => Err(WireError::NotAnObject),
    }
}

/// Builds a request line without terminator. The transaction id lives under `osc`; a path
/// that itself starts with `osc` shares that object. A request the id would push over
/// `MAX_REQUEST_TOKENS` (a 20-band EQ list) goes without it; its reply is matched by its path.
pub fn request_line(path: &SscPath, value: Value, xid: u64) -> String {
    let nested = path.segments().iter().rev().fold(value, |inner, segment| {
        Value::Object(Map::from_iter([(segment.clone(), inner)]))
    });
    let mut request = serde_json::json!({ OSC_KEY: { XID_KEY: xid } });
    merge(&mut request, nested.clone());
    if token_count(&request) > MAX_REQUEST_TOKENS {
        return nested.to_string();
    }
    request.to_string()
}

/// The JSON tokens of a value, as a token-counting parser sees it: the value itself, and for an
/// object each key and member, for an array each entry.
pub fn token_count(value: &Value) -> usize {
    match value {
        Value::Object(members) => {
            1 + members
                .values()
                .map(|member| 1 + token_count(member))
                .sum::<usize>()
        }
        Value::Array(entries) => 1 + entries.iter().map(token_count).sum::<usize>(),
        _ => 1,
    }
}

/// A JSON number for a set request; NaN and infinities have no JSON form.
pub fn number(value: f64) -> Result<Value, WireError> {
    Number::from_f64(value)
        .map(Value::Number)
        .ok_or(WireError::NonFinite)
}

/// The value at `path` if present and not `null`.
pub fn extract<'a>(tree: &'a Value, path: &SscPath) -> Option<&'a Value> {
    let found = path
        .segments()
        .iter()
        .try_fold(tree, |node, segment| node.get(segment))?;
    (!found.is_null()).then_some(found)
}

/// The transaction id echoed in `osc/xid`, if it is a non-negative integer.
pub fn reply_xid(tree: &Value) -> Option<u64> {
    tree.get(OSC_KEY)?.get(XID_KEY)?.as_u64()
}

/// The first `[code, {"desc": text}]` pair under `osc/error`, depth-first.
pub fn device_error(tree: &Value) -> Option<DeviceError> {
    find_pair(tree.get(OSC_KEY)?.get(ERROR_KEY)?)
}

fn find_pair(node: &Value) -> Option<DeviceError> {
    match node {
        Value::Array(items) => as_pair(items).or_else(|| items.iter().find_map(find_pair)),
        Value::Object(members) => members.values().find_map(find_pair),
        _ => None,
    }
}

fn as_pair(items: &[Value]) -> Option<DeviceError> {
    let [code, detail] = items else {
        return None;
    };
    Some(DeviceError {
        code: code.as_i64()?,
        description: detail.get(DESCRIPTION_KEY)?.as_str()?.to_owned(),
    })
}
