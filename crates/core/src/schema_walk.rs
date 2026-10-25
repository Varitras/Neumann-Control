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
        if error.is_connection_lost() {
            return WalkError::Connection(error);
        }
        WalkError::Refused(error.to_string())
    }
}

/// Every leaf breadth-first from the root with its value and limits. A device error on one
/// leaf, or on the schema of one container below the root, stays in that node's report. `should_stop` is asked before every request, so a caller
/// waiting to shut down waits for one request at most.
pub fn read_all(
    client: &mut Client,
    should_stop: &dyn Fn() -> bool,
) -> Result<Vec<LeafReport>, WalkError> {
    let mut walk = Walk::new();
    loop {
        if let Progress::Done(reports) = walk.advance(client, should_stop)? {
            return Ok(reports);
        }
    }
}

pub enum Progress {
    Running,
    Done(Vec<LeafReport>),
}

/// The same walk taken one step at a time (one container, or one leaf with its limits), so
/// the caller can answer commands and poll meters in between.
pub struct Walk {
    containers: VecDeque<Option<SscPath>>,
    leaves: VecDeque<SscPath>,
    reports: Vec<LeafReport>,
    visited: usize,
}

impl Default for Walk {
    fn default() -> Walk {
        Walk::new()
    }
}

impl Walk {
    pub fn new() -> Walk {
        Walk {
            containers: VecDeque::from([None]),
            leaves: VecDeque::new(),
            reports: Vec::new(),
            visited: 0,
        }
    }

    pub fn advance(
        &mut self,
        client: &mut Client,
        should_stop: &dyn Fn() -> bool,
    ) -> Result<Progress, WalkError> {
        if let Some(container) = self.containers.pop_front() {
            self.expand(client, container, should_stop)?;
            return Ok(Progress::Running);
        }
        if let Some(path) = self.leaves.pop_front() {
            self.reports.push(report(client, path, should_stop)?);
            return Ok(Progress::Running);
        }
        Ok(Progress::Done(std::mem::take(&mut self.reports)))
    }

    fn expand(
        &mut self,
        client: &mut Client,
        container: Option<SscPath>,
        should_stop: &dyn Fn() -> bool,
    ) -> Result<(), WalkError> {
        if should_stop() {
            return Err(WalkError::Stopped);
        }
        let nodes = match client.schema(container.as_ref()) {
            Ok(nodes) => nodes,
            Err(error) => return self.refused_container(container, error),
        };
        for (name, kind) in nodes {
            if container.is_none() && name == PROTOCOL_CONTAINER {
                continue;
            }
            self.visited += 1;
            if self.visited > MAX_SCHEMA_NODES {
                return Err(WalkError::Refused(format!(
                    "schema has more than {MAX_SCHEMA_NODES} nodes"
                )));
            }
            let path = child_path(container.as_ref(), &name)?;
            let depth = path.segments().len();
            match kind {
                NodeKind::Leaf => self.leaves.push_back(path),
                NodeKind::Container if depth >= MAX_SCHEMA_DEPTH => {
                    return Err(WalkError::Refused(format!(
                        "schema is deeper than {MAX_SCHEMA_DEPTH} levels"
                    )));
                }
                NodeKind::Container => self.containers.push_back(Some(path)),
            }
        }
        Ok(())
    }

    /// A refusal or an unusable answer for a container below the root is that container's
    /// line in the report; a lost connection, or a failed root, ends the walk.
    fn refused_container(
        &mut self,
        container: Option<SscPath>,
        error: SscError,
    ) -> Result<(), WalkError> {
        let Some(path) = container.filter(|_| !error.is_connection_lost()) else {
            return Err(error.into());
        };
        self.reports.push(LeafReport {
            path,
            value: Err(error.to_string()),
            limits: None,
        });
        Ok(())
    }
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
        Err(lost) if lost.is_connection_lost() => return Err(WalkError::Connection(lost)),
        Err(refusal) => Err(refusal.to_string()),
    };
    if should_stop() {
        return Err(WalkError::Stopped);
    }
    let limits = match client.limits(&path) {
        Ok(limits) => Some(limits),
        Err(lost) if lost.is_connection_lost() => return Err(WalkError::Connection(lost)),
        Err(_) => None,
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
