//! The path table: which SSC paths exist, on which models, how they are shown, polled and
//! written. All of it is data in `paths.toml`; this module only loads and applies it.

use std::collections::HashSet;
use std::fmt;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::Value;

use crate::ssc::wire::SscPath;

const EMBEDDED_CATALOG: &str = include_str!("paths.toml");
const PRINTABLE_ASCII: std::ops::RangeInclusive<char> = ' '..='~';
/// Relative to the step size; absorbs binary rounding of decimal steps.
const STEP_TOLERANCE: f64 = 1e-9;
const KELVIN_OFFSET: f64 = 273.15;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Output,
    Output1,
    Output2,
    Input,
    Standby,
    Device,
    RearPanel,
    Metering,
}

impl Group {
    pub fn title(&self) -> &'static str {
        match self {
            Group::Output => "Ausgang",
            Group::Output1 => "Ausgang 1",
            Group::Output2 => "Ausgang 2",
            Group::Input => "Eingang",
            Group::Standby => "Standby",
            Group::Device => "Gerät",
            Group::RearPanel => "Rückseite",
            Group::Metering => "Messung",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Number,
    Switch,
    Select,
    Text,
    Display,
    Meter,
    Clip,
}

impl Kind {
    fn can_be_written(&self) -> bool {
        matches!(
            self,
            Kind::Number | Kind::Switch | Kind::Select | Kind::Text
        )
    }
}

/// `Once` marks identity values that are read when connecting.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Poll {
    Fast,
    Slow,
    Meter,
    Once,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelClass {
    All,
    Monitor,
    Subwoofer,
    Logo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conversion {
    KelvinToCelsius,
}

impl Conversion {
    pub fn apply(&self, value: f64) -> f64 {
        match self {
            Conversion::KelvinToCelsius => value - KELVIN_OFFSET,
        }
    }
}

/// Product lists behind the model classes; a monitor is any product that is not a subwoofer.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ModelClasses {
    pub subwoofer: Vec<String>,
    pub logo: Vec<String>,
}

impl ModelClasses {
    pub fn is_subwoofer(&self, product: &str) -> bool {
        self.subwoofer.iter().any(|known| known == product)
    }

    pub fn contains(&self, class: ModelClass, product: &str) -> bool {
        match class {
            ModelClass::All => true,
            ModelClass::Monitor => !self.is_subwoofer(product),
            ModelClass::Subwoofer => self.is_subwoofer(product),
            ModelClass::Logo => self.logo.iter().any(|known| known == product),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ValueError {
    OutOfRange { min: f64, max: f64 },
    NotAnOption,
    NotAStep,
    TooLong(usize),
    NotPrintableAscii,
    WrongType,
}

impl fmt::Display for ValueError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValueError::OutOfRange { min, max } => write!(formatter, "value outside {min}..{max}"),
            ValueError::NotAnOption => formatter.write_str("value is not one of the options"),
            ValueError::NotAStep => formatter.write_str("value is not a whole number of steps"),
            ValueError::TooLong(limit) => write!(formatter, "text longer than {limit} characters"),
            ValueError::NotPrintableAscii => formatter.write_str("text is not printable ASCII"),
            ValueError::WrongType => formatter.write_str("value has the wrong type"),
        }
    }
}

impl std::error::Error for ValueError {}

#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub path: SscPath,
    pub label: String,
    pub group: Group,
    pub kind: Kind,
    pub poll: Poll,
    pub class: ModelClass,
    #[serde(default)]
    pub unit: Option<String>,
    #[serde(default)]
    pub min: Option<f64>,
    #[serde(default)]
    pub max: Option<f64>,
    /// Upper bound on subwoofer models, where it differs from `max`.
    #[serde(default)]
    pub max_subwoofer: Option<f64>,
    #[serde(default)]
    pub step: Option<f64>,
    #[serde(default)]
    pub max_length: Option<usize>,
    #[serde(default)]
    pub options: Vec<String>,
    #[serde(default)]
    pub writable_on: Vec<String>,
    #[serde(default)]
    pub conversion: Option<Conversion>,
}

impl Entry {
    pub fn is_writable(&self, product: &str) -> bool {
        self.writable_on.iter().any(|known| known == product)
    }

    pub fn max_for(&self, product: &str, classes: &ModelClasses) -> Option<f64> {
        if classes.is_subwoofer(product) && self.max_subwoofer.is_some() {
            return self.max_subwoofer;
        }
        self.max
    }

    pub fn validate(
        &self,
        value: &Value,
        product: &str,
        classes: &ModelClasses,
    ) -> Result<(), ValueError> {
        match self.kind {
            Kind::Number => self.validate_number(value, self.max_for(product, classes)),
            Kind::Switch => value.as_bool().map(|_| ()).ok_or(ValueError::WrongType),
            Kind::Select => self.validate_option(value),
            Kind::Text => self.validate_text(value),
            Kind::Display | Kind::Meter | Kind::Clip => Err(ValueError::WrongType),
        }
    }

    fn validate_number(&self, value: &Value, max: Option<f64>) -> Result<(), ValueError> {
        let number = value
            .as_f64()
            .filter(|candidate| candidate.is_finite())
            .ok_or(ValueError::WrongType)?;
        let below = self.min.is_some_and(|min| number < min);
        let above = max.is_some_and(|max| number > max);
        if below || above {
            return Err(ValueError::OutOfRange {
                min: self.min.unwrap_or(f64::NEG_INFINITY),
                max: max.unwrap_or(f64::INFINITY),
            });
        }
        self.validate_step(number)
    }

    fn validate_step(&self, number: f64) -> Result<(), ValueError> {
        let Some(step) = self.step.filter(|step| *step > 0.0) else {
            return Ok(());
        };
        let steps = (number - self.min.unwrap_or(0.0)) / step;
        if (steps - steps.round()).abs() > STEP_TOLERANCE {
            return Err(ValueError::NotAStep);
        }
        Ok(())
    }

    fn validate_option(&self, value: &Value) -> Result<(), ValueError> {
        let text = value.as_str().ok_or(ValueError::WrongType)?;
        if self.options.iter().any(|option| option == text) {
            return Ok(());
        }
        Err(ValueError::NotAnOption)
    }

    fn validate_text(&self, value: &Value) -> Result<(), ValueError> {
        let text = value.as_str().ok_or(ValueError::WrongType)?;
        if !text
            .chars()
            .all(|character| PRINTABLE_ASCII.contains(&character))
        {
            return Err(ValueError::NotPrintableAscii);
        }
        match self.max_length {
            Some(limit) if text.len() > limit => Err(ValueError::TooLong(limit)),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, PartialEq)]
pub enum CatalogError {
    Syntax(String),
    DuplicatePath(SscPath),
    Invalid { path: SscPath, reason: &'static str },
}

impl fmt::Display for CatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogError::Syntax(message) => write!(formatter, "catalog syntax: {message}"),
            CatalogError::DuplicatePath(path) => write!(formatter, "catalog lists {path} twice"),
            CatalogError::Invalid { path, reason } => write!(formatter, "catalog {path}: {reason}"),
        }
    }
}

impl std::error::Error for CatalogError {}

#[derive(Debug, Deserialize)]
struct CatalogFile {
    models: ModelClasses,
    entry: Vec<Entry>,
}

#[derive(Debug)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    pub classes: ModelClasses,
}

impl Catalog {
    /// Panics only when the embedded file is invalid; a test pins that it is not.
    pub fn embedded() -> &'static Catalog {
        static EMBEDDED: OnceLock<Catalog> = OnceLock::new();
        EMBEDDED.get_or_init(|| Catalog::parse(EMBEDDED_CATALOG).expect("embedded paths.toml"))
    }

    pub fn parse(text: &str) -> Result<Catalog, CatalogError> {
        let file: CatalogFile =
            toml::from_str(text).map_err(|error| CatalogError::Syntax(error.to_string()))?;
        let mut seen = HashSet::new();
        for entry in &file.entry {
            if !seen.insert(entry.path.clone()) {
                return Err(CatalogError::DuplicatePath(entry.path.clone()));
            }
            check_entry(entry)?;
        }
        Ok(Catalog {
            entries: file.entry,
            classes: file.models,
        })
    }

    pub fn entries_for(&self, product: &str) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|entry| self.classes.contains(entry.class, product))
            .collect()
    }
}

fn check_entry(entry: &Entry) -> Result<(), CatalogError> {
    let invalid = |reason| CatalogError::Invalid {
        path: entry.path.clone(),
        reason,
    };
    if !entry.kind.can_be_written() && !entry.writable_on.is_empty() {
        return Err(invalid("read-only kind lists writable models"));
    }
    if entry.kind == Kind::Select && entry.options.is_empty() {
        return Err(invalid("select without options"));
    }
    if entry.kind == Kind::Text && entry.max_length.is_none() {
        return Err(invalid("text without max_length"));
    }
    let reversed = matches!((entry.min, entry.max), (Some(min), Some(max)) if min > max);
    if reversed {
        return Err(invalid("min above max"));
    }
    Ok(())
}
