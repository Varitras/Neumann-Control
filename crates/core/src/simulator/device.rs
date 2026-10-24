//! The simulated speaker: a value tree plus the answering rules recorded from real devices.
//! No I/O here; the server module moves the lines.

use serde_json::{json, Map, Number, Value};

use super::SimModel;
use crate::ssc::wire::{merge, token_count, DeviceError, SscPath, MAX_REQUEST_TOKENS};

const OSC_KEY: &str = "osc";
const XID_KEY: &str = "xid";
const ERROR_KEY: &str = "error";
const SCHEMA_KEY: &str = "schema";
const LIMITS_KEY: &str = "limits";
const VERSION_KEY: &str = "version";
const PING_KEY: &str = "ping";
const FEATURE_KEY: &str = "feature";
const PATH_SEPARATOR: &str = "/";

const OSC_VERSION: &str = "1.1";
const OSC_PATTERN_FEATURE: &str = "*?";
const FEATURE_PATTERN: &str = "pattern";
const FEATURE_SUBSCRIPTION: &str = "subscription";

const TYPE_KEY: &str = "type";
const TYPE_NUMBER: &str = "Number";
const TYPE_STRING: &str = "String";
const TYPE_BOOLEAN: &str = "Boolean";

struct ErrorKind {
    code: i64,
    description: &'static str,
}

const BAD_REQUEST: ErrorKind = ErrorKind {
    code: 400,
    description: "message not understood",
};
const NOT_FOUND: ErrorKind = ErrorKind {
    code: DeviceError::NOT_FOUND,
    description: "address not found",
};
const NOT_AUTHORISED: ErrorKind = ErrorKind {
    code: DeviceError::LOCAL_MODE,
    description: "authorisation needed",
};
const NOT_ALLOWED: ErrorKind = ErrorKind {
    code: DeviceError::READ_ONLY,
    description: "method not allowed",
};
const NOT_ACCEPTABLE: ErrorKind = ErrorKind {
    code: DeviceError::NOT_ACCEPTABLE,
    description: "not acceptable",
};
const REQUEST_TOO_LONG: ErrorKind = ErrorKind {
    code: 413,
    description: "request too long",
};
const PARAMETER_NOT_FOUND: ErrorKind = ErrorKind {
    code: DeviceError::PARAMETER_NOT_FOUND,
    description: "parameter address not found",
};

/// While the rear CONTROL switch is on LOCAL, every set answers 401 and gets still work.
/// Measured on the KH 750 and the KH 120 II.
const CONTROL_MODE_PATH: &str = "ui/control_mode";
const LOCAL_CONTROL_MODE: &str = "LOCAL";

const KH_120_II_TREE: &str = include_str!("kh120ii.json");
const KH_750_TREE: &str = include_str!("kh750.json");

/// Read-only on every model: identity, live metering, derived values.
const READ_ONLY_COMMON: &[&str] = &[
    "device/identity/product",
    "device/identity/vendor",
    "device/identity/serial",
    "device/identity/version",
    "device/identity/hw_version",
    "device/standby/countdown",
    "device/temperature",
    "audio/in/current_input",
    "m/in/level",
    "m/in/clip",
    "m/out/level",
    "m/out/clip",
    "warnings",
];

/// Refused with 405 on the KH 120 II in the hardware write tests; `ui/control_mode` follows
/// the rear CONTROL switch.
const READ_ONLY_KH_120_II: &[&str] = &[
    "ui/control_mode",
    "ui/input_gain",
    "ui/input_select",
    "ui/bass_gain",
    "ui/mid_gain",
    "ui/treble_gain",
    "ui/output_level",
];

/// Refused with 405 on the KH 750 in the hardware write tests (`ui/control_mode` and
/// `ui/auto_standby` follow rear switches), plus the sensors the integration models as
/// read-only. `device/standby/enabled` is writable on the KH 120 II only.
const READ_ONLY_KH_750: &[&str] = &[
    "ui/control_mode",
    "ui/auto_standby",
    "ui/bass_management",
    "ui/channel_b_input_mode",
    "ui/subwoofer_input_gain",
    "ui/subwoofer_low_cut",
    "ui/subwoofer_output_level",
    "ui/subwoofer_phase",
    "ui/subwoofer_phase_inversion",
    "audio/digital_bypass",
    "audio/out/label",
    "audio/out1/label",
    "audio/out2/label",
    "audio/out1/loudspeaker",
    "audio/out2/loudspeaker",
    "device/standby/enabled",
];

/// The reply to one request, before the transaction id is attached.
pub struct Answer {
    pub body: Value,
    pub xid: Option<Value>,
}

pub struct SimulatedDevice {
    tree: Value,
    read_only_for_model: &'static [&'static str],
    /// Containers whose `osc/schema` is refused, as a protected node of a firmware might be.
    refused_schemas: Vec<Vec<String>>,
}

impl SimulatedDevice {
    pub fn new(model: SimModel) -> SimulatedDevice {
        let (source, read_only_for_model) = match model {
            SimModel::Kh120Ii => (KH_120_II_TREE, READ_ONLY_KH_120_II),
            SimModel::Kh750 => (KH_750_TREE, READ_ONLY_KH_750),
        };
        SimulatedDevice {
            tree: serde_json::from_str(source).expect("embedded value tree is valid JSON"),
            read_only_for_model,
            refused_schemas: Vec::new(),
        }
    }

    pub fn value(&self, path: &str) -> Option<Value> {
        let path = SscPath::parse(path).ok()?;
        lookup(&self.tree, path.segments()).cloned()
    }

    pub fn set_value(&mut self, path: &str, value: Value) {
        let path = SscPath::parse(path).expect("a valid SSC path");
        let mut node = &mut self.tree;
        for segment in path.segments() {
            if !node.is_object() {
                *node = Value::Object(Map::new());
            }
            node = node
                .as_object_mut()
                .expect("made an object above")
                .entry(segment.clone())
                .or_insert(Value::Null);
        }
        *node = value;
    }

    pub fn refuse_schema(&mut self, path: &str) {
        let path = SscPath::parse(path).expect("a valid SSC path");
        self.refused_schemas.push(path.segments().to_vec());
    }

    pub fn remove_value(&mut self, path: &str) {
        let path = SscPath::parse(path).expect("a valid SSC path");
        let Some((name, parents)) = path.segments().split_last() else {
            return;
        };
        let parent = parents
            .iter()
            .try_fold(&mut self.tree, |node, segment| node.get_mut(segment));
        if let Some(members) = parent.and_then(Value::as_object_mut) {
            members.remove(name);
        }
    }

    pub fn answer(&mut self, request: &Map<String, Value>) -> Answer {
        // Measured: the speaker gives up before it reads the xid, so the refusal carries none.
        if token_count(&Value::Object(request.clone())) > MAX_REQUEST_TOKENS {
            return Answer {
                body: error_reply(&REQUEST_TOO_LONG),
                xid: None,
            };
        }
        let xid = request
            .get(OSC_KEY)
            .and_then(|osc| osc.get(XID_KEY))
            .cloned();
        let body = self.answer_body(request, xid.is_some());
        Answer { body, xid }
    }

    fn answer_body(&mut self, request: &Map<String, Value>, has_xid: bool) -> Value {
        let Some(osc) = request.get(OSC_KEY) else {
            return self.answer_values(request);
        };
        let Some(osc) = osc.as_object() else {
            return error_reply(&BAD_REQUEST);
        };
        if let Some(argument) = osc.get(SCHEMA_KEY) {
            return self.schema(argument);
        }
        if let Some(argument) = osc.get(LIMITS_KEY) {
            return self.limits(argument);
        }
        let Some(methods) = osc_methods(osc) else {
            return error_reply(&BAD_REQUEST);
        };
        let mut values = request.clone();
        values.remove(OSC_KEY);
        if values.is_empty() && methods.is_empty() && !has_xid {
            return error_reply(&BAD_REQUEST);
        }
        let mut body = if values.is_empty() {
            Value::Object(Map::new())
        } else {
            self.answer_values(&values)
        };
        if !methods.is_empty() {
            merge(&mut body, json!({ OSC_KEY: methods }));
        }
        body
    }

    fn answer_values(&mut self, request: &Map<String, Value>) -> Value {
        let leaves = collect_leaves(request, &[]);
        if leaves.is_empty() {
            return error_reply(&BAD_REQUEST);
        }
        let bundled = leaves.len() > 1;
        let mut body = Value::Object(Map::new());
        let mut errors = Vec::new();
        for (path, value) in leaves {
            match self.answer_leaf(&path, value, bundled) {
                Ok(subtree) => merge(&mut body, subtree),
                Err(entry) => errors.push(entry),
            }
        }
        if !errors.is_empty() {
            body[OSC_KEY] = json!({ ERROR_KEY: errors });
        }
        body
    }

    /// One leaf of a request: the answer subtree, or the error entry nested at its node.
    fn answer_leaf(
        &mut self,
        path: &[String],
        value: Value,
        bundled: bool,
    ) -> Result<Value, Value> {
        let current = match lookup(&self.tree, path) {
            Some(found) if !found.is_object() => found.clone(),
            found => {
                let node = if found.is_none() {
                    &path[..self.first_missing_depth(path)]
                } else {
                    path
                };
                // Recorded: inside a bundle the 404 sits at the parent of the failing node.
                let node = if bundled && node.len() > 1 {
                    &node[..node.len() - 1]
                } else {
                    node
                };
                return Err(error_entry(node, &NOT_FOUND));
            }
        };
        if value.is_null() {
            return Ok(nest(path, current));
        }
        if self.is_in_local_mode() {
            return Err(error_entry(path, &NOT_AUTHORISED));
        }
        if self.is_read_only(path) {
            return Err(error_entry(path, &NOT_ALLOWED));
        }
        if !same_kind(&current, &value) {
            return Err(error_entry(path, &NOT_ACCEPTABLE));
        }
        let resulting = resulting_value(&current, value);
        self.store(path, resulting.clone());
        Ok(nest(path, resulting))
    }

    fn first_missing_depth(&self, path: &[String]) -> usize {
        let mut node = &self.tree;
        for (depth, segment) in path.iter().enumerate() {
            match node.get(segment) {
                Some(child) => node = child,
                None => return depth + 1,
            }
        }
        path.len()
    }

    fn is_in_local_mode(&self) -> bool {
        self.value(CONTROL_MODE_PATH)
            .as_ref()
            .and_then(Value::as_str)
            == Some(LOCAL_CONTROL_MODE)
    }

    fn is_read_only(&self, path: &[String]) -> bool {
        let joined = path.join(PATH_SEPARATOR);
        READ_ONLY_COMMON.contains(&joined.as_str())
            || self.read_only_for_model.contains(&joined.as_str())
    }

    fn store(&mut self, path: &[String], value: Value) {
        if let Some(slot) = path
            .iter()
            .try_fold(&mut self.tree, |node, segment| node.get_mut(segment))
        {
            *slot = value;
        }
    }

    /// Everything `osc/schema` describes: the device tree plus the `osc` container.
    fn address_tree(&self) -> Value {
        let mut tree = self.tree.clone();
        tree[OSC_KEY] = osc_tree();
        tree
    }

    fn schema(&self, argument: &Value) -> Value {
        let tree = self.address_tree();
        if argument.is_null() {
            return osc_reply(SCHEMA_KEY, json!([schema_level(&tree)]));
        }
        let path = match introspection_path(argument, SCHEMA_KEY) {
            Ok(path) => path,
            Err(refusal) => return refusal,
        };
        if self.refused_schemas.contains(&path) {
            return introspection_error(SCHEMA_KEY, &NOT_ALLOWED);
        }
        let Some(node) = lookup(&tree, &path) else {
            return introspection_error(SCHEMA_KEY, &PARAMETER_NOT_FOUND);
        };
        let described = if node.is_object() {
            schema_level(node)
        } else {
            Value::Null
        };
        osc_reply(SCHEMA_KEY, json!([nest(&path, described)]))
    }

    fn limits(&self, argument: &Value) -> Value {
        let path = match introspection_path(argument, LIMITS_KEY) {
            Ok(path) => path,
            Err(refusal) => return refusal,
        };
        let tree = self.address_tree();
        let Some(node) = lookup(&tree, &path).filter(|node| !node.is_object()) else {
            return introspection_error(LIMITS_KEY, &PARAMETER_NOT_FOUND);
        };
        // Only the type is reproduced; the other fields (units, range, options) are not
        // recorded per path, and none is invented.
        let description = if path[0] == OSC_KEY {
            json!({})
        } else {
            json!({ TYPE_KEY: limits_type(node) })
        };
        osc_reply(LIMITS_KEY, json!([nest(&path, json!([description]))]))
    }
}

/// The serialized line: the transaction id rides in `osc/xid`. An error reply already has an
/// `osc` member, and real devices then send a second one for the id instead of merging.
pub fn encode_line(body: &Value, xid: Option<&Value>) -> String {
    let Some(xid) = xid else {
        return body.to_string();
    };
    let has_error = body
        .get(OSC_KEY)
        .and_then(|osc| osc.get(ERROR_KEY))
        .is_some();
    if has_error {
        let text = body.to_string();
        let opened = text.strip_suffix('}').unwrap_or(&text);
        return format!("{opened},\"{OSC_KEY}\":{{\"{XID_KEY}\":{xid}}}}}");
    }
    let mut tagged = body.clone();
    merge(&mut tagged, json!({ OSC_KEY: { XID_KEY: xid } }));
    tagged.to_string()
}

fn lookup<'a>(tree: &'a Value, path: &[String]) -> Option<&'a Value> {
    path.iter()
        .try_fold(tree, |node, segment| node.get(segment))
}

fn nest(path: &[String], value: Value) -> Value {
    path.iter().rev().fold(value, |inner, segment| {
        Value::Object(Map::from_iter([(segment.clone(), inner)]))
    })
}

/// Flattens a request into (path, value) pairs, stopping at non-object values and `{}`.
fn collect_leaves(members: &Map<String, Value>, prefix: &[String]) -> Vec<(Vec<String>, Value)> {
    let mut leaves = Vec::new();
    for (name, value) in members {
        let mut path = prefix.to_vec();
        path.push(name.clone());
        match value.as_object().filter(|nested| !nested.is_empty()) {
            Some(nested) => leaves.extend(collect_leaves(nested, &path)),
            None => leaves.push((path, value.clone())),
        }
    }
    leaves
}

fn error_pair(kind: &ErrorKind) -> Value {
    json!([kind.code, { "desc": kind.description }])
}

fn error_entry(node: &[String], kind: &ErrorKind) -> Value {
    nest(node, error_pair(kind))
}

/// A flat error for requests whose real reply was never recorded.
fn error_reply(kind: &ErrorKind) -> Value {
    json!({ OSC_KEY: { ERROR_KEY: error_pair(kind) } })
}

/// A refused introspection request: the error sits at `osc/<method>`, as recorded.
fn introspection_error(method: &str, kind: &ErrorKind) -> Value {
    json!({ OSC_KEY: { ERROR_KEY: [nest(&[OSC_KEY.to_owned(), method.to_owned()], error_pair(kind))] } })
}

fn osc_reply(method: &str, payload: Value) -> Value {
    json!({ OSC_KEY: { method: payload } })
}

/// The address an `osc/schema` or `osc/limits` request names. Recorded: the argument is a
/// list holding one address tree that ends in `null`; `[null]`, `[{}]` or no list is a 454,
/// a tree ending in `{}` a 406.
fn introspection_path(argument: &Value, method: &str) -> Result<Vec<String>, Value> {
    let tree = argument
        .as_array()
        .and_then(|items| items.first())
        .and_then(Value::as_object)
        .filter(|members| !members.is_empty())
        .ok_or_else(|| introspection_error(method, &PARAMETER_NOT_FOUND))?;
    let mut leaves = collect_leaves(tree, &[]);
    let (Some((path, end)), true) = (leaves.pop(), leaves.is_empty()) else {
        return Err(error_reply(&BAD_REQUEST));
    };
    if !end.is_null() {
        return Err(introspection_error(method, &NOT_ACCEPTABLE));
    }
    Ok(path)
}

/// One level of the address tree as `osc/schema` reports it: containers `{}`, leaves `null`.
fn schema_level(node: &Value) -> Value {
    let members = node.as_object().into_iter().flatten();
    Value::Object(
        members
            .map(|(name, child)| {
                let described = if child.is_object() {
                    json!({})
                } else {
                    Value::Null
                };
                (name.clone(), described)
            })
            .collect(),
    )
}

/// The `osc` container as the devices describe it; every method is a leaf.
fn osc_tree() -> Value {
    json!({
        "state": {
            "auth": { "access": null },
            "prettyprint": null,
            "subscribe": null,
            "subscribe_reserve": null
        },
        "feature": { "timetag": null, "baseaddr": null, "subscription": null, "pattern": null },
        "limits": null,
        "schema": null,
        "version": null,
        "xid": null,
        "ping": null,
        "error": null
    })
}

/// The recorded answers of `osc/version`, `osc/ping` and single `osc/feature` gets; `None`
/// for anything else (sets, containers), whose reply was never recorded.
fn osc_methods(osc: &Map<String, Value>) -> Option<Map<String, Value>> {
    let mut answer = Map::new();
    for (name, value) in osc.iter().filter(|(name, _)| name.as_str() != XID_KEY) {
        match (name.as_str(), value) {
            (VERSION_KEY, Value::Null) => answer.insert(name.clone(), json!(OSC_VERSION)),
            (PING_KEY, Value::Null) => answer.insert(name.clone(), Value::Null),
            (FEATURE_KEY, Value::Object(asked)) if !asked.is_empty() => {
                answer.insert(name.clone(), features(asked)?)
            }
            _ => return None,
        };
    }
    Some(answer)
}

fn features(asked: &Map<String, Value>) -> Option<Value> {
    let mut answer = Map::new();
    for (name, value) in asked {
        let known = match name.as_str() {
            FEATURE_PATTERN => json!(OSC_PATTERN_FEATURE),
            FEATURE_SUBSCRIPTION => json!(true),
            _ => return None,
        };
        if !value.is_null() {
            return None;
        }
        answer.insert(name.clone(), known);
    }
    Some(Value::Object(answer))
}

fn limits_type(value: &Value) -> &'static str {
    match value {
        Value::Array(items) => items.first().map_or(TYPE_STRING, limits_type),
        Value::Bool(_) => TYPE_BOOLEAN,
        Value::Number(_) => TYPE_NUMBER,
        _ => TYPE_STRING,
    }
}

/// Whether a set carries the kind of value the leaf holds; lists must keep their length.
fn same_kind(current: &Value, value: &Value) -> bool {
    match (current, value) {
        (Value::Bool(_), Value::Bool(_))
        | (Value::Number(_), Value::Number(_))
        | (Value::String(_), Value::String(_)) => true,
        (Value::Array(held), Value::Array(given)) => {
            held.len() == given.len()
                && held
                    .iter()
                    .zip(given)
                    .all(|(left, right)| right.is_null() || same_kind(left, right))
        }
        _ => false,
    }
}

/// A float leaf stays a float: `90` is stored and echoed as `90.0`, as the devices do. A `null`
/// in an array keeps that entry (measured on EQ arrays: only the given band changes).
fn resulting_value(current: &Value, value: Value) -> Value {
    match (current, &value) {
        (_, Value::Null) => current.clone(),
        (Value::Number(held), Value::Number(given)) if held.is_f64() => given
            .as_f64()
            .and_then(Number::from_f64)
            .map_or(value, Value::Number),
        (Value::Array(held), Value::Array(given)) => Value::Array(
            held.iter()
                .zip(given)
                .map(|(left, right)| resulting_value(left, right.clone()))
                .collect(),
        ),
        _ => value,
    }
}
