use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

use nf_project::{Id, NodeDocument};

use crate::catalog::{DynamicPins, NodeDefinition};
use crate::generics::type_parameter;
use crate::pin::PinDefinition;
use crate::scope::GraphScope;

pub const COUNT_PROPERTY: &str = "count";
pub const CASES_PROPERTY: &str = "cases";
pub const CASE_PREFIX: &str = "case_";
pub const TIMELINE: &str = "flow/timeline";
pub const TRACKS_PROPERTY: &str = "tracks";
pub const LENGTH_PROPERTY: &str = "length";
pub const LOOPING_PROPERTY: &str = "looping";
pub const TRACK_PREFIX: &str = "track_";
pub const DEFAULT_TIMELINE_LENGTH: f64 = 1.0;

#[derive(Debug, Clone, PartialEq)]
pub struct TimelineTrack {
    pub id: String,
    pub name: String,
    pub keys: Vec<(f64, f64)>,
}

fn number(value: &toml::Value) -> Option<f64> {
    match value {
        toml::Value::Integer(number) => Some(*number as f64),
        toml::Value::Float(number) => Some(*number),
        _ => None,
    }
}

pub fn timeline_length(properties: &BTreeMap<String, toml::Value>) -> f64 {
    properties
        .get(LENGTH_PROPERTY)
        .and_then(number)
        .filter(|length| *length > 0.0)
        .unwrap_or(DEFAULT_TIMELINE_LENGTH)
}

pub fn timeline_looping(properties: &BTreeMap<String, toml::Value>) -> bool {
    properties.get(LOOPING_PROPERTY).and_then(toml::Value::as_bool).unwrap_or(false)
}

pub fn timeline_tracks(properties: &BTreeMap<String, toml::Value>) -> Vec<TimelineTrack> {
    let Some(toml::Value::Array(entries)) = properties.get(TRACKS_PROPERTY) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let table = entry.as_table()?;
            let id = table.get("id")?.as_str()?.to_owned();
            if id.is_empty() || !id.chars().all(|character| character.is_ascii_alphanumeric()) {
                return None;
            }
            let name = table.get("name").and_then(toml::Value::as_str).unwrap_or("Track").to_owned();
            let mut keys: Vec<(f64, f64)> = table
                .get("keys")
                .and_then(toml::Value::as_array)
                .map(|keys| {
                    keys.iter()
                        .filter_map(|key| {
                            let pair = key.as_array()?;
                            Some((number(pair.first()?)?, number(pair.get(1)?)?))
                        })
                        .collect()
                })
                .unwrap_or_default();
            keys.sort_by(|left, right| left.0.total_cmp(&right.0));
            Some(TimelineTrack { id, name, keys })
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
pub struct SwitchCase {
    pub id: String,
    pub value: toml::Value,
}

pub fn switch_cases(properties: &BTreeMap<String, toml::Value>) -> Vec<SwitchCase> {
    let Some(toml::Value::Array(entries)) = properties.get(CASES_PROPERTY) else {
        return Vec::new();
    };
    entries
        .iter()
        .filter_map(|entry| {
            let table = entry.as_table()?;
            let id = table.get("id")?.as_str()?.to_owned();
            let value = table.get("value")?.clone();
            let valid_id = !id.is_empty() && id.chars().all(|character| character.is_ascii_alphanumeric());
            valid_id.then_some(SwitchCase { id, value })
        })
        .collect()
}

fn case_label(value: &toml::Value) -> String {
    match value {
        toml::Value::String(text) => format!("\"{text}\""),
        other => other.to_string(),
    }
}

fn pin_count(properties: &BTreeMap<String, toml::Value>, minimum: usize, fallback: usize) -> usize {
    let count = match properties.get(COUNT_PROPERTY) {
        Some(toml::Value::Integer(count)) => usize::try_from(*count).unwrap_or(fallback),
        _ => fallback,
    };
    count.clamp(minimum, 64)
}

pub fn expand_definition(definition: &NodeDefinition, properties: &BTreeMap<String, toml::Value>) -> Option<NodeDefinition> {
    let dynamic = definition.dynamic.as_ref()?;
    let mut expanded = definition.clone();
    match dynamic {
        DynamicPins::ExecOutputs {
            prefix,
            label,
            minimum,
            fallback,
            ..
        } => {
            let count = pin_count(properties, *minimum, *fallback);
            let mut outputs: Vec<PinDefinition> = (0..count)
                .map(|index| PinDefinition::exec(&format!("{prefix}{index}"), &format!("{label} {index}")))
                .collect();
            outputs.extend(definition.outputs.iter().cloned());
            expanded.outputs = outputs;
        }
        DynamicPins::DataInputs {
            prefix,
            label,
            minimum,
            fallback,
            ..
        } => {
            let count = pin_count(properties, *minimum, *fallback);
            expanded.inputs.extend((0..count).map(|index| {
                let mut pin = PinDefinition::data(&format!("{prefix}{index}"), &format!("{label} {index}"), type_parameter());
                pin.optional = true;
                pin
            }));
        }
        DynamicPins::Tracks => {
            expanded.outputs.extend(
                timeline_tracks(properties)
                    .iter()
                    .map(|track| PinDefinition::data(&format!("{TRACK_PREFIX}{}", track.id), &track.name, nf_api::TypeExpr::named("number"))),
            );
        }
        DynamicPins::Cases => {
            let mut outputs: Vec<PinDefinition> = switch_cases(properties)
                .iter()
                .map(|case| PinDefinition::exec(&format!("{CASE_PREFIX}{}", case.id), &case_label(&case.value)))
                .collect();
            outputs.extend(definition.outputs.iter().cloned());
            expanded.outputs = outputs;
        }
    }
    Some(expanded)
}

pub fn node_definitions<'s>(scope: &'s GraphScope<'s>, nodes: &[NodeDocument]) -> HashMap<Id, Cow<'s, NodeDefinition>> {
    nodes
        .iter()
        .filter_map(|node| {
            let definition = scope.get(&node.node_type)?;
            Some((
                node.id,
                match expand_definition(definition, &node.properties) {
                    Some(expanded) => Cow::Owned(expanded),
                    None => Cow::Borrowed(definition),
                },
            ))
        })
        .collect()
}
