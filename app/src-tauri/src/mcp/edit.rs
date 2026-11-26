use std::collections::{BTreeMap, BTreeSet};

use nf_graph::{COMMENT, DynamicPins, FUNCTION_ENTRY, GraphScope, MACRO_INPUTS, MACRO_OUTPUTS, NodeDefinition, PinDefinition};
use nf_project::{Id, NodeDocument, PinLink, PinValue};
use schemars::JsonSchema;
use serde::Deserialize;

const GRID: f64 = 16.0;
const COLUMN_GAP: f64 = 360.0;
const ROW_GAP: f64 = 180.0;
const PROTECTED_NODES: [&str; 3] = [FUNCTION_ENTRY, MACRO_INPUTS, MACRO_OUTPUTS];

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PinReference {
    #[schemars(description = "Node id, or the `ref` of a node added earlier in the same call.")]
    pub node: String,
    #[schemars(description = "Pin name as listed by get_graph or describe_node_type.")]
    pub pin: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Operation {
    #[schemars(description = "Add a node. Give it a `ref` to connect it later in the same call.")]
    Add {
        #[serde(rename = "ref")]
        reference: Option<String>,
        #[serde(rename = "type")]
        node_type: String,
        #[schemars(description = "Canvas position [x, y]. Left out, the node is placed next to the node it connects to.")]
        position: Option<[f64; 2]>,
        #[serde(default)]
        properties: BTreeMap<String, serde_json::Value>,
    },
    #[schemars(description = "Remove a node and every link to it.")]
    Remove { node: String },
    #[schemars(description = "Move a node to a canvas position [x, y].")]
    Move { node: String, position: [f64; 2] },
    #[schemars(description = "Connect an output pin to an input pin. Exec outputs link to exec inputs, data outputs to data inputs.")]
    Connect { from: PinReference, to: PinReference },
    #[schemars(description = "Remove every link of a pin.")]
    Disconnect { node: String, pin: String },
    #[schemars(description = "Set the literal value of an unconnected data input. Booleans, numbers, strings, enum keys as strings and structs like {\"X\": 0, \"Y\": 0, \"Z\": 0}.")]
    SetValue { node: String, pin: String, value: serde_json::Value },
    #[schemars(description = "Set a node property, for example `count` for nodes with a variable number of pins or `text` for comments.")]
    SetProperty { node: String, key: String, value: serde_json::Value },
}

#[derive(Debug, Default)]
pub struct EditOutcome {
    pub created: BTreeMap<String, Id>,
    pub upserts: Vec<NodeDocument>,
    pub deletions: Vec<Id>,
    pub touched: Vec<Id>,
}

struct Editor<'a> {
    scope: &'a GraphScope<'a>,
    nodes: BTreeMap<Id, NodeDocument>,
    references: BTreeMap<String, Id>,
    unplaced: Vec<Id>,
    touched: BTreeSet<Id>,
}

fn snap(value: f64) -> f64 {
    (value / GRID).round() * GRID
}

fn to_toml(value: serde_json::Value, what: &str) -> Result<toml::Value, String> {
    if value.is_null() {
        return Err(format!("{what} cannot be null."));
    }
    toml::Value::try_from(value).map_err(|error| format!("{what} is not a valid value: {error}"))
}

fn initial_properties(definition: &NodeDefinition) -> BTreeMap<String, toml::Value> {
    let mut properties = BTreeMap::new();
    if definition.id == COMMENT {
        properties.insert("text".to_owned(), toml::Value::String("Comment".to_owned()));
        properties.insert("width".to_owned(), toml::Value::Integer(420));
        properties.insert("height".to_owned(), toml::Value::Integer(260));
    }
    match &definition.dynamic {
        Some(DynamicPins::ExecOutputs { initial, .. } | DynamicPins::DataInputs { initial, .. }) => {
            properties.insert(nf_graph::COUNT_PROPERTY.to_owned(), toml::Value::Integer(*initial as i64));
        }
        Some(DynamicPins::Cases) => {
            properties.insert(nf_graph::CASES_PROPERTY.to_owned(), toml::Value::Array(Vec::new()));
        }
        Some(DynamicPins::Tracks) | None => {}
    }
    properties
}

fn find_pin<'d>(pins: &'d [PinDefinition], name: &str) -> Option<&'d PinDefinition> {
    pins.iter().find(|pin| pin.name == name)
}

fn pin_names(pins: &[PinDefinition]) -> String {
    if pins.is_empty() {
        return "none".to_owned();
    }
    pins.iter().map(|pin| pin.name.as_str()).collect::<Vec<_>>().join(", ")
}

impl<'a> Editor<'a> {
    fn resolve(&self, reference: &str) -> Result<Id, String> {
        if let Some(id) = self.references.get(reference) {
            return Ok(*id);
        }
        let id: Id = reference
            .parse()
            .map_err(|_| format!("'{reference}' is neither a node id of this graph nor the ref of a node added in this call."))?;
        if self.nodes.contains_key(&id) {
            Ok(id)
        } else {
            Err(format!("The graph has no node {reference}."))
        }
    }

    fn definition(&self, id: Id) -> Result<NodeDefinition, String> {
        let node = &self.nodes[&id];
        let definition = self
            .scope
            .get(&node.node_type)
            .ok_or_else(|| format!("Node {id} has the unknown type '{}'.", node.node_type))?;
        Ok(nf_graph::expand_definition(definition, &node.properties).unwrap_or_else(|| definition.clone()))
    }

    fn node_mut(&mut self, id: Id) -> &mut NodeDocument {
        self.touched.insert(id);
        self.nodes.get_mut(&id).expect("resolved nodes exist")
    }

    fn add(&mut self, reference: Option<String>, node_type: String, position: Option<[f64; 2]>, properties: BTreeMap<String, serde_json::Value>) -> Result<(), String> {
        let definition = self
            .scope
            .get(&node_type)
            .ok_or_else(|| format!("There is no node type '{node_type}' in this graph. Use search_nodes to find the right type id."))?;
        if let Some(reference) = &reference
            && self.references.contains_key(reference)
        {
            return Err(format!("The ref '{reference}' is used twice in this call."));
        }
        let mut node = NodeDocument::new(node_type, position.map(|[x, y]| [snap(x), snap(y)]).unwrap_or_default());
        node.properties = initial_properties(definition);
        for (key, value) in properties {
            node.properties.insert(key.clone(), to_toml(value, &format!("Property '{key}'"))?);
        }
        let id = node.id;
        if position.is_none() {
            self.unplaced.push(id);
        }
        self.nodes.insert(id, node);
        self.touched.insert(id);
        if let Some(reference) = reference {
            self.references.insert(reference, id);
        }
        Ok(())
    }

    fn remove(&mut self, id: Id) -> Result<(), String> {
        if PROTECTED_NODES.contains(&self.nodes[&id].node_type.as_str()) {
            return Err(format!("Node {id} is the entry of this graph and cannot be removed."));
        }
        self.nodes.remove(&id);
        self.touched.remove(&id);
        self.unplaced.retain(|unplaced| *unplaced != id);
        let linked: Vec<Id> = self
            .nodes
            .values()
            .filter(|node| {
                node.exec.values().any(|link| link.node == id) || node.inputs.values().any(|value| matches!(value, PinValue::Link(link) if link.node == id))
            })
            .map(|node| node.id)
            .collect();
        for other in linked {
            let node = self.node_mut(other);
            node.exec.retain(|_, link| link.node != id);
            node.inputs.retain(|_, value| !matches!(value, PinValue::Link(link) if link.node == id));
        }
        Ok(())
    }

    fn connect(&mut self, from: Id, from_pin: &str, to: Id, to_pin: &str) -> Result<(), String> {
        let source = self.definition(from)?;
        let target = self.definition(to)?;
        let output = find_pin(&source.outputs, from_pin)
            .ok_or_else(|| format!("'{}' has no output pin '{from_pin}'. Its outputs are: {}.", source.title, pin_names(&source.outputs)))?;
        let input = find_pin(&target.inputs, to_pin)
            .ok_or_else(|| format!("'{}' has no input pin '{to_pin}'. Its inputs are: {}.", target.title, pin_names(&target.inputs)))?;
        match (output.pin_type.is_exec(), input.pin_type.is_exec()) {
            (true, true) => {
                self.node_mut(from).exec.insert(from_pin.to_owned(), PinLink { node: to, pin: to_pin.to_owned() });
            }
            (false, false) => {
                self.node_mut(to)
                    .inputs
                    .insert(to_pin.to_owned(), PinValue::Link(PinLink { node: from, pin: from_pin.to_owned() }));
            }
            _ => return Err(format!("'{from_pin}' and '{to_pin}' cannot be connected: one is an exec pin and the other a data pin.")),
        }
        Ok(())
    }

    fn disconnect(&mut self, id: Id, pin: &str) {
        let node = self.node_mut(id);
        node.exec.remove(pin);
        if matches!(node.inputs.get(pin), Some(PinValue::Link(_))) {
            node.inputs.remove(pin);
        }
        let linked: Vec<Id> = self
            .nodes
            .values()
            .filter(|node| {
                node.exec.values().any(|link| link.node == id && link.pin == pin)
                    || node.inputs.values().any(|value| matches!(value, PinValue::Link(link) if link.node == id && link.pin == pin))
            })
            .map(|node| node.id)
            .collect();
        for other in linked {
            let node = self.node_mut(other);
            node.exec.retain(|_, link| !(link.node == id && link.pin == pin));
            node.inputs.retain(|_, value| !matches!(value, PinValue::Link(link) if link.node == id && link.pin == pin));
        }
    }

    fn set_value(&mut self, id: Id, pin: &str, value: serde_json::Value) -> Result<(), String> {
        let definition = self.definition(id)?;
        let input = find_pin(&definition.inputs, pin)
            .ok_or_else(|| format!("'{}' has no input pin '{pin}'. Its inputs are: {}.", definition.title, pin_names(&definition.inputs)))?;
        if input.pin_type.is_exec() {
            return Err(format!("'{pin}' is an exec pin and has no value."));
        }
        let literal = to_toml(value, &format!("The value of '{pin}'"))?;
        self.node_mut(id).inputs.insert(pin.to_owned(), PinValue::Literal { literal });
        Ok(())
    }

    fn apply(&mut self, operation: Operation) -> Result<(), String> {
        match operation {
            Operation::Add { reference, node_type, position, properties } => self.add(reference, node_type, position, properties),
            Operation::Remove { node } => {
                let id = self.resolve(&node)?;
                self.remove(id)
            }
            Operation::Move { node, position: [x, y] } => {
                let id = self.resolve(&node)?;
                self.unplaced.retain(|unplaced| *unplaced != id);
                self.node_mut(id).position = [snap(x), snap(y)];
                Ok(())
            }
            Operation::Connect { from, to } => {
                let source = self.resolve(&from.node)?;
                let target = self.resolve(&to.node)?;
                self.connect(source, &from.pin, target, &to.pin)
            }
            Operation::Disconnect { node, pin } => {
                let id = self.resolve(&node)?;
                self.disconnect(id, &pin);
                Ok(())
            }
            Operation::SetValue { node, pin, value } => {
                let id = self.resolve(&node)?;
                self.set_value(id, &pin, value)
            }
            Operation::SetProperty { node, key, value } => {
                let id = self.resolve(&node)?;
                let value = to_toml(value, &format!("Property '{key}'"))?;
                self.node_mut(id).properties.insert(key, value);
                Ok(())
            }
        }
    }

    fn neighbour_of(&self, id: Id) -> Option<(Id, bool)> {
        for node in self.nodes.values() {
            if node.id == id {
                continue;
            }
            let feeds = node.exec.values().any(|link| link.node == id);
            if feeds {
                return Some((node.id, true));
            }
        }
        let node = &self.nodes[&id];
        node.exec
            .values()
            .map(|link| (link.node, false))
            .chain(node.inputs.values().filter_map(|value| match value {
                PinValue::Link(link) => Some((link.node, true)),
                PinValue::Literal { .. } => None,
            }))
            .next()
            .or_else(|| {
                self.nodes
                    .values()
                    .find(|other| other.inputs.values().any(|value| matches!(value, PinValue::Link(link) if link.node == id)))
                    .map(|other| (other.id, false))
            })
    }

    fn place_unplaced(&mut self) {
        let unplaced = std::mem::take(&mut self.unplaced);
        let pending: BTreeSet<Id> = unplaced.iter().copied().collect();
        let mut placed: BTreeSet<Id> = BTreeSet::new();
        let mut occupied: Vec<[f64; 2]> = self
            .nodes
            .values()
            .filter(|node| !pending.contains(&node.id))
            .map(|node| node.position)
            .collect();
        let fallback_x = occupied.iter().map(|position| position[0]).fold(f64::NEG_INFINITY, f64::max);
        let fallback_y = occupied.iter().map(|position| position[1]).fold(f64::INFINITY, f64::min);
        let fallback = if occupied.is_empty() { [0.0, 0.0] } else { [fallback_x + COLUMN_GAP, fallback_y] };

        for _ in 0..unplaced.len() {
            for id in &unplaced {
                if placed.contains(id) {
                    continue;
                }
                let anchor = self
                    .neighbour_of(*id)
                    .filter(|(neighbour, _)| !pending.contains(neighbour) || placed.contains(neighbour));
                let Some((neighbour, upstream)) = anchor else {
                    continue;
                };
                let base = self.nodes[&neighbour].position;
                let x = if upstream { base[0] + COLUMN_GAP } else { base[0] - COLUMN_GAP };
                let position = free_position(&occupied, [x, base[1]]);
                occupied.push(position);
                self.nodes.get_mut(id).expect("unplaced nodes exist").position = position;
                placed.insert(*id);
            }
        }
        for id in unplaced {
            if placed.contains(&id) {
                continue;
            }
            let position = free_position(&occupied, fallback);
            occupied.push(position);
            self.nodes.get_mut(&id).expect("unplaced nodes exist").position = position;
        }
    }
}

fn free_position(occupied: &[[f64; 2]], wanted: [f64; 2]) -> [f64; 2] {
    let mut position = [snap(wanted[0]), snap(wanted[1])];
    while occupied
        .iter()
        .any(|other| (other[0] - position[0]).abs() < COLUMN_GAP / 2.0 && (other[1] - position[1]).abs() < ROW_GAP / 2.0)
    {
        position[1] = snap(position[1] + ROW_GAP);
    }
    position
}

pub fn apply_operations(scope: &GraphScope<'_>, nodes: Vec<NodeDocument>, operations: Vec<Operation>) -> Result<EditOutcome, String> {
    let original: BTreeMap<Id, NodeDocument> = nodes.into_iter().map(|node| (node.id, node)).collect();
    let mut editor = Editor {
        scope,
        nodes: original.clone(),
        references: BTreeMap::new(),
        unplaced: Vec::new(),
        touched: BTreeSet::new(),
    };
    for (index, operation) in operations.into_iter().enumerate() {
        editor.apply(operation).map_err(|message| format!("Operation {} failed, nothing was changed: {message}", index + 1))?;
    }
    editor.place_unplaced();

    let deletions = original.keys().filter(|id| !editor.nodes.contains_key(id)).copied().collect();
    let upserts = editor
        .nodes
        .values()
        .filter(|node| original.get(&node.id) != Some(node))
        .cloned()
        .collect();
    Ok(EditOutcome {
        created: editor.references,
        upserts,
        deletions,
        touched: editor.touched.into_iter().collect(),
    })
}
