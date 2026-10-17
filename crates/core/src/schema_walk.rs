//! Reads every value a speaker has by walking `osc/schema`, for the "read everything" view.

use std::collections::VecDeque;

use serde_json::Value;

use crate::ssc::client::{Client, NodeKind, SscError};
use crate::ssc::wire::SscPath;

/// Bounds the walk, because the tree comes from a network device that can describe anything.
pub const MAX_SCHEMA_NODES: usize = 500;
pub const MAX_SCHEMA_DEPTH: usize = 10;

/// Holds the protocol's own methods (`osc/schema`, `osc/limits`, ...), not device values.
const PROTOCOL_CONTAINER: &str = "osc";
const PATH_SEPARATOR: char = '/';

#[derive(Clone, Debug, PartialEq)]
pub struct LeafReport {
    pub path: SscPath,
    /// The value, or the device's refusal as text.
    pub value: Result<Value, String>,
    /// `None` when the device has no limits for the leaf.
    pub limits: Option<Value>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WalkError {
    /// The connection is gone; the caller has to reconnect.
    Connection(SscError),
    /// The device or the shape of its tree ended the walk; the connection is still usable.
    Refused(String),
    /// `should_stop` asked to end the walk.
    Stopped,
}

impl From<SscError> for WalkError {
    fn from(error: SscError) -> WalkError {
        match error {
            SscError::Device(_) => WalkError::Refused(error.to_string()),
            lost => WalkError::Connection(lost),
        }
    }
}

/// Every leaf breadth-first from the root with its value and limits. A device error on one
/// leaf stays in that leaf's report. `should_stop` is asked before every request, so a caller
/// waiting to shut down waits for one request at most.
pub fn read_all(
    client: &mut Client,
    should_stop: &dyn Fn() -> bool,
) -> Result<Vec<LeafReport>, WalkError> {
    let leaves = leaf_paths(client, should_stop)?;
    let mut reports = Vec::with_capacity(leaves.len());
    for path in leaves {
        reports.push(report(client, path, should_stop)?);
    }
    Ok(reports)
}

fn leaf_paths(
    client: &mut Client,
    should_stop: &dyn Fn() -> bool,
) -> Result<Vec<SscPath>, WalkError> {
    let mut leaves = Vec::new();
    let mut containers: VecDeque<Option<SscPath>> = VecDeque::from([None]);
    let mut visited = 0usize;
    while let Some(container) = containers.pop_front() {
        if should_stop() {
            return Err(WalkError::Stopped);
        }
        for (name, kind) in client.schema(container.as_ref())? {
            if container.is_none() && name == PROTOCOL_CONTAINER {
                continue;
            }
            visited += 1;
            if visited > MAX_SCHEMA_NODES {
                return Err(WalkError::Refused(format!(
                    "schema has more than {MAX_SCHEMA_NODES} nodes"
                )));
            }
            let path = child_path(container.as_ref(), &name)?;
            let depth = path.segments().len();
            match kind {
                NodeKind::Leaf => leaves.push(path),
                NodeKind::Container if depth >= MAX_SCHEMA_DEPTH => {
                    return Err(WalkError::Refused(format!(
                        "schema is deeper than {MAX_SCHEMA_DEPTH} levels"
                    )));
                }
                NodeKind::Container => containers.push_back(Some(path)),
            }
        }
    }
    Ok(leaves)
}

fn report(
    client: &mut Client,
    path: SscPath,
    should_stop: &dyn Fn() -> bool,
) -> Result<LeafReport, WalkError> {
    if should_stop() {
        return Err(WalkError::Stopped);
    }
    let value = match client.get(&path) {
        Ok(value) => Ok(value),
        Err(refusal @ SscError::Device(_)) => Err(refusal.to_string()),
        Err(lost) => return Err(WalkError::Connection(lost)),
    };
    if should_stop() {
        return Err(WalkError::Stopped);
    }
    let limits = match client.limits(&path) {
        Ok(limits) => Some(limits),
        Err(SscError::Device(_)) => None,
        Err(lost) => return Err(WalkError::Connection(lost)),
    };
    Ok(LeafReport {
        path,
        value,
        limits,
    })
}

/// A node name holding the separator would silently become several segments.
fn child_path(parent: Option<&SscPath>, name: &str) -> Result<SscPath, WalkError> {
    if name.contains(PATH_SEPARATOR) {
        return Err(WalkError::Refused(format!(
            "schema names a node with a '{PATH_SEPARATOR}'"
        )));
    }
    let text = match parent {
        Some(parent) => format!("{parent}{PATH_SEPARATOR}{name}"),
        None => name.to_owned(),
    };
    SscPath::parse(&text).map_err(|error| WalkError::Refused(error.to_string()))
}
