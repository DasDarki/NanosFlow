use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use nf_project::{
    FunctionPin, FunctionSignature, GraphDocument, GraphKind, GraphManifest, Id, METHODS_DIRECTORY, NodeDocument, PinLink, PinValue,
    ProjectError, VariableDocument, apply_graph_changes, create_graph, create_method, update_function_signature,
};
use serde::Serialize;

use crate::catalog::{NodeCatalog, NodeDefinition, NodeKind};
use crate::generics::{NodeMap, resolve_pin_type};
use crate::scope::{
    CLASS_SELF, FUNCTION_ENTRY, FUNCTION_RETURN, GraphScope, build_scope, call_node_id, method_node_id, variable_get_id, variable_set_id,
};

const SELF_PIN: &str = "self";
const ENTRY_GAP: f64 = 320.0;

pub struct CollapseRequest<'a> {
    pub nodes: &'a [NodeDocument],
    pub selection: &'a [Id],
    pub graph_variables: &'a [VariableDocument],
    pub owner_class: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct Collapse {
    pub signature: FunctionSignature,
    pub function_nodes: Vec<NodeDocument>,
    pub call: NodeDocument,
    pub changed: Vec<NodeDocument>,
    pub removed: Vec<Id>,
}

impl Collapse {
    pub fn graph_changes(mut self, call_type: String) -> (Vec<NodeDocument>, Vec<Id>) {
        self.call.node_type = call_type;
        let mut upserts = self.changed;
        upserts.push(self.call);
        (upserts, self.removed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollapseError {
    pub message: String,
    pub hint: String,
    pub nodes: Vec<Id>,
}

impl CollapseError {
    fn new(message: impl Into<String>, hint: impl Into<String>, nodes: Vec<Id>) -> CollapseError {
        CollapseError {
            message: message.into(),
            hint: hint.into(),
            nodes,
        }
    }
}

struct Boundary {
    key: (Id, String),
    pin: FunctionPin,
}

struct PinNames {
    used: HashSet<String>,
}

impl PinNames {
    fn new() -> PinNames {
        PinNames {
            used: ["exec", "then", SELF_PIN].into_iter().map(str::to_owned).collect(),
        }
    }

    fn claim(&mut self, label: &str) -> String {
        let mut base = String::new();
        for character in label.chars() {
            if character.is_ascii_alphanumeric() {
                base.push(character.to_ascii_lowercase());
            } else if !base.ends_with('_') && !base.is_empty() {
                base.push('_');
            }
        }
        let base = base.trim_end_matches('_').to_owned();
        let base = if base.is_empty() || base.starts_with(|character: char| character.is_ascii_digit()) {
            format!("value_{base}").trim_end_matches('_').to_owned()
        } else {
            base
        };
        let mut candidate = base.clone();
        let mut counter = 2;
        while !self.used.insert(candidate.clone()) {
            candidate = format!("{base}_{counter}");
            counter += 1;
        }
        candidate
    }
}

fn pin_label(definition: Option<&NodeDefinition>, pin: &str) -> String {
    let label = definition
        .and_then(|definition| definition.output(pin))
        .map(|output| if output.label.is_empty() { crate::text::humanize(&output.name) } else { output.label.clone() })
        .unwrap_or_else(|| crate::text::humanize(pin));
    let generic = ["result", "value", "return value", ""].contains(&label.to_lowercase().as_str());
    match definition {
        Some(definition) if generic => definition.title.split(" (").next().unwrap_or(&definition.title).to_owned(),
        _ => label,
    }
}

pub fn collapse_selection(scope: &GraphScope, request: &CollapseRequest) -> Result<Collapse, CollapseError> {
    let definitions = crate::node_definitions(scope, request.nodes);
    let definition_of = |id: Id| definitions.get(&id).map(|definition| definition.as_ref());
    let node_map: NodeMap = request.nodes.iter().map(|node| (node.id, (node, definition_of(node.id)))).collect();
    let by_id: HashMap<Id, &NodeDocument> = request.nodes.iter().map(|node| (node.id, node)).collect();

    let variable_getters: HashMap<String, &VariableDocument> =
        request.graph_variables.iter().map(|variable| (variable_get_id(variable.id), variable)).collect();
    let variable_setters: HashSet<String> = request.graph_variables.iter().map(|variable| variable_set_id(variable.id)).collect();

    let mut selected: HashSet<Id> = HashSet::new();
    let mut blocked = Vec::new();
    let mut setters = Vec::new();
    for id in request.selection {
        let Some(node) = by_id.get(id) else {
            continue;
        };
        let kind = definition_of(*id).map(|definition| definition.kind);
        if kind == Some(NodeKind::Comment) || variable_getters.contains_key(&node.node_type) {
            continue;
        }
        if matches!(kind, Some(NodeKind::Event | NodeKind::FunctionEntry | NodeKind::FunctionReturn))
            || node.node_type == FUNCTION_ENTRY
            || node.node_type == FUNCTION_RETURN
        {
            blocked.push(*id);
            continue;
        }
        if variable_setters.contains(&node.node_type) {
            setters.push(*id);
            continue;
        }
        selected.insert(*id);
    }
    if !blocked.is_empty() {
        return Err(CollapseError::new(
            "Events and function entry or return nodes cannot be moved into a new function.",
            "Deselect them and keep only the logic you want to collapse.",
            blocked,
        ));
    }
    if !setters.is_empty() {
        return Err(CollapseError::new(
            "The selection sets a variable of this graph. Graph variables only exist in this graph, so a new function could not change them.",
            "Keep the Set node outside of the selection, return the value from the function and set the variable after the call.",
            setters,
        ));
    }
    if selected.is_empty() {
        return Err(CollapseError::new(
            "There is nothing to collapse in the selection.",
            "Select the nodes that should become the new function.",
            Vec::new(),
        ));
    }

    let mut inside: Vec<NodeDocument> =
        request.nodes.iter().filter(|node| selected.contains(&node.id)).cloned().collect();
    inside.sort_by_key(|node| node.id);
    let outside: Vec<&NodeDocument> = request.nodes.iter().filter(|node| !selected.contains(&node.id)).collect();

    let has_exec = inside.iter().any(|node| {
        definition_of(node.id).is_some_and(|definition| {
            definition.inputs.iter().chain(definition.outputs.iter()).any(|pin| pin.pin_type.is_exec())
        })
    });

    let mut entry_targets: Vec<PinLink> = Vec::new();
    let mut entering: Vec<(Id, String)> = Vec::new();
    for node in &outside {
        for (pin, link) in &node.exec {
            if selected.contains(&link.node) {
                if !entry_targets.contains(link) {
                    entry_targets.push(link.clone());
                }
                entering.push((node.id, pin.clone()));
            }
        }
    }
    if entry_targets.len() > 1 {
        return Err(CollapseError::new(
            "Execution enters the selection at several places. A function can only start at one node.",
            "Select fewer nodes so that all incoming execution wires lead into the same node.",
            entry_targets.iter().map(|link| link.node).collect(),
        ));
    }
    let entry_target = match entry_targets.pop() {
        Some(target) => Some(target),
        None if has_exec => {
            let internally_entered: HashSet<Id> =
                inside.iter().flat_map(|node| node.exec.values()).map(|link| link.node).collect();
            let starts: Vec<(Id, String)> = inside
                .iter()
                .filter(|node| !internally_entered.contains(&node.id))
                .filter_map(|node| {
                    let definition = definition_of(node.id)?;
                    let exec = definition.inputs.iter().find(|pin| pin.pin_type.is_exec())?;
                    Some((node.id, exec.name.clone()))
                })
                .collect();
            if starts.len() > 1 {
                return Err(CollapseError::new(
                    "The selection has several separate starting points, so it is unclear where the function should begin.",
                    "Collapse each chain on its own, or connect them with a Sequence first.",
                    starts.iter().map(|(id, _)| *id).collect(),
                ));
            }
            starts.into_iter().next().map(|(node, pin)| PinLink { node, pin })
        }
        None => None,
    };

    let mut exits: Vec<PinLink> = Vec::new();
    for node in &inside {
        for link in node.exec.values() {
            if !selected.contains(&link.node) && !exits.contains(link) {
                exits.push(link.clone());
            }
        }
    }
    if exits.len() > 1 {
        return Err(CollapseError::new(
            "Execution leaves the selection towards several different nodes. A function call can only continue at one place.",
            "Include the following nodes in the selection, or collapse a smaller part.",
            exits.iter().map(|link| link.node).collect(),
        ));
    }
    let exit = exits.pop();

    let mut names = PinNames::new();
    let mut parameters: Vec<Boundary> = Vec::new();
    let mut self_sources: HashSet<(Id, String)> = HashSet::new();
    for node in &inside {
        for value in node.inputs.values() {
            let PinValue::Link(link) = value else {
                continue;
            };
            if selected.contains(&link.node) {
                continue;
            }
            let key = (link.node, link.pin.clone());
            if parameters.iter().any(|parameter| parameter.key == key) || self_sources.contains(&key) {
                continue;
            }
            let source = by_id.get(&link.node).copied();
            let source_definition = definition_of(link.node);
            let provides_self = request.owner_class.is_some()
                && link.pin == SELF_PIN
                && (source.is_some_and(|source| source.node_type == CLASS_SELF)
                    || source_definition.is_some_and(|definition| matches!(definition.kind, NodeKind::Event | NodeKind::FunctionEntry)));
            if provides_self {
                self_sources.insert(key);
                continue;
            }
            let label = match source.and_then(|source| variable_getters.get(&source.node_type)) {
                Some(variable) => variable.name.clone(),
                None => pin_label(source_definition, &link.pin),
            };
            let type_name = source_definition
                .and_then(|definition| definition.output(&link.pin))
                .and_then(|output| resolve_pin_type(&node_map, link.node, output))
                .map(|type_expr| type_expr.to_string())
                .unwrap_or_else(|| "any".to_owned());
            parameters.push(Boundary {
                key,
                pin: FunctionPin {
                    id: names.claim(&label),
                    name: label,
                    type_name,
                },
            });
        }
    }

    let mut returns: Vec<Boundary> = Vec::new();
    let mut consumers: Vec<(Id, String, (Id, String))> = Vec::new();
    let mut return_names = PinNames::new();
    for node in &outside {
        for (input, value) in &node.inputs {
            let PinValue::Link(link) = value else {
                continue;
            };
            if !selected.contains(&link.node) {
                continue;
            }
            let key = (link.node, link.pin.clone());
            consumers.push((node.id, input.clone(), key.clone()));
            if returns.iter().any(|value| value.key == key) {
                continue;
            }
            let source_definition = definition_of(link.node);
            let label = pin_label(source_definition, &link.pin);
            let type_name = source_definition
                .and_then(|definition| definition.output(&link.pin))
                .and_then(|output| resolve_pin_type(&node_map, link.node, output))
                .map(|type_expr| type_expr.to_string())
                .unwrap_or_else(|| "any".to_owned());
            returns.push(Boundary {
                key,
                pin: FunctionPin {
                    id: return_names.claim(&label),
                    name: label,
                    type_name,
                },
            });
        }
    }

    let produced_by_exec_nodes = returns.iter().any(|value| {
        definition_of(value.key.0).is_some_and(|definition| definition.has_exec_input())
    });
    if has_exec && exit.is_none() && produced_by_exec_nodes {
        let nodes = returns.iter().map(|value| value.key.0).collect();
        return Err(CollapseError::new(
            "Values of the selection are used outside of it, but execution never leaves the selection, so the function would not know when to return them.",
            "Include the nodes that use these values in the selection, or connect the selection's last node to the node that follows it.",
            nodes,
        ));
    }

    let min_x = inside.iter().map(|node| node.position[0]).fold(f64::INFINITY, f64::min);
    let max_x = inside.iter().map(|node| node.position[0]).fold(f64::NEG_INFINITY, f64::max);
    let min_y = inside.iter().map(|node| node.position[1]).fold(f64::INFINITY, f64::min);
    let center = [
        inside.iter().map(|node| node.position[0]).sum::<f64>() / inside.len() as f64,
        inside.iter().map(|node| node.position[1]).sum::<f64>() / inside.len() as f64,
    ];

    let mut entry = NodeDocument::new(FUNCTION_ENTRY, [min_x - ENTRY_GAP, min_y]);
    if let Some(target) = &entry_target {
        if let Some(node) = inside.iter().find(|node| node.id == target.node) {
            entry.position[1] = node.position[1];
        }
        entry.exec.insert("then".to_owned(), target.clone());
    }

    let return_inputs: BTreeMap<String, PinValue> = returns
        .iter()
        .map(|value| {
            (
                value.pin.id.clone(),
                PinValue::Link(PinLink {
                    node: value.key.0,
                    pin: value.key.1.clone(),
                }),
            )
        })
        .collect();

    let mut function_nodes: Vec<NodeDocument> = Vec::new();
    let mut return_nodes: Vec<NodeDocument> = Vec::new();
    for node in &mut inside {
        for value in node.inputs.values_mut() {
            let PinValue::Link(link) = value else {
                continue;
            };
            if selected.contains(&link.node) {
                continue;
            }
            let key = (link.node, link.pin.clone());
            let pin = if self_sources.contains(&key) {
                SELF_PIN.to_owned()
            } else {
                match parameters.iter().find(|parameter| parameter.key == key) {
                    Some(parameter) => parameter.pin.id.clone(),
                    None => continue,
                }
            };
            *link = PinLink { node: entry.id, pin };
        }
        let position = node.position;
        for link in node.exec.values_mut() {
            if selected.contains(&link.node) {
                continue;
            }
            let mut return_node = NodeDocument::new(FUNCTION_RETURN, [max_x + ENTRY_GAP, position[1]]);
            return_node.inputs = return_inputs.clone();
            *link = PinLink {
                node: return_node.id,
                pin: "exec".to_owned(),
            };
            return_nodes.push(return_node);
        }
    }
    if return_nodes.is_empty() && !returns.is_empty() {
        let mut return_node = NodeDocument::new(FUNCTION_RETURN, [max_x + ENTRY_GAP, center[1]]);
        return_node.inputs = return_inputs;
        return_nodes.push(return_node);
    }
    function_nodes.push(entry);
    function_nodes.extend(inside);
    function_nodes.extend(return_nodes);

    let mut call = NodeDocument::new(String::new(), center);
    for parameter in &parameters {
        call.inputs.insert(
            parameter.pin.id.clone(),
            PinValue::Link(PinLink {
                node: parameter.key.0,
                pin: parameter.key.1.clone(),
            }),
        );
    }
    if let Some(exit) = exit {
        call.exec.insert("then".to_owned(), exit);
    }

    let mut changed: BTreeMap<Id, NodeDocument> = BTreeMap::new();
    for (node, pin) in entering {
        let document = changed.entry(node).or_insert_with(|| by_id[&node].clone());
        document.exec.insert(
            pin,
            PinLink {
                node: call.id,
                pin: "exec".to_owned(),
            },
        );
    }
    for (node, input, key) in consumers {
        let Some(value) = returns.iter().find(|value| value.key == key) else {
            continue;
        };
        let document = changed.entry(node).or_insert_with(|| by_id[&node].clone());
        document.inputs.insert(
            input,
            PinValue::Link(PinLink {
                node: call.id,
                pin: value.pin.id.clone(),
            }),
        );
    }

    let mut removed: Vec<Id> = selected.into_iter().collect();
    removed.sort();
    Ok(Collapse {
        signature: FunctionSignature {
            exported: false,
            description: String::new(),
            pure: !has_exec,
            parameters: parameters.into_iter().map(|parameter| parameter.pin).collect(),
            returns: returns.into_iter().map(|value| value.pin).collect(),
        },
        function_nodes,
        call,
        changed: changed.into_values().collect(),
        removed,
    })
}

#[derive(Debug)]
pub enum CollapseFailure {
    Selection(CollapseError),
    Project(ProjectError),
}

impl From<ProjectError> for CollapseFailure {
    fn from(error: ProjectError) -> CollapseFailure {
        CollapseFailure::Project(error)
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollapsedFunction {
    pub path: PathBuf,
    pub id: Id,
    pub call: Id,
}

fn owning_class(graph_path: &Path, manifest: &GraphManifest) -> Option<PathBuf> {
    if manifest.kind == GraphKind::Class {
        return Some(graph_path.to_path_buf());
    }
    let methods = graph_path.parent()?;
    if methods.file_name()? != METHODS_DIRECTORY {
        return None;
    }
    let class = methods.parent()?;
    let is_class = GraphManifest::read(class).is_ok_and(|parent| parent.kind == GraphKind::Class);
    is_class.then(|| class.to_path_buf())
}

struct PlannedCollapse {
    class_directory: Option<PathBuf>,
    collapse: Result<Collapse, CollapseError>,
}

fn plan_from_disk(catalog: &NodeCatalog, package_directory: &Path, graph_path: &Path, selection: &[Id]) -> Result<PlannedCollapse, ProjectError> {
    let document = GraphDocument::load(graph_path)?;
    let scope = GraphScope::new(catalog, build_scope(catalog, package_directory, graph_path)?);
    let class_directory = owning_class(graph_path, &document.manifest);
    let class_name = class_directory
        .as_ref()
        .and_then(|directory| directory.file_name())
        .map(|name| name.to_string_lossy().into_owned());
    let graph_variables: &[VariableDocument] =
        if document.manifest.kind == GraphKind::Class { &[] } else { &document.variables };
    let collapse = collapse_selection(
        &scope,
        &CollapseRequest {
            nodes: &document.nodes,
            selection,
            graph_variables,
            owner_class: class_name.as_deref(),
        },
    );
    Ok(PlannedCollapse { class_directory, collapse })
}

pub fn check_collapse(catalog: &NodeCatalog, package_directory: &Path, graph_path: &Path, selection: &[Id]) -> Result<Option<CollapseError>, ProjectError> {
    Ok(plan_from_disk(catalog, package_directory, graph_path, selection)?.collapse.err())
}

pub fn collapse_into_function(
    catalog: &NodeCatalog,
    package_directory: &Path,
    graph_path: &Path,
    selection: &[Id],
    name: &str,
) -> Result<CollapsedFunction, CollapseFailure> {
    let planned = plan_from_disk(catalog, package_directory, graph_path, selection)?;
    let class_directory = planned.class_directory;
    let collapse = planned.collapse.map_err(CollapseFailure::Selection)?;

    let directory = match &class_directory {
        Some(class_directory) => create_method(class_directory, name)?,
        None => {
            let parent = graph_path.parent().unwrap_or(graph_path);
            create_graph(parent, name, GraphKind::Function)?
        }
    };
    let created = GraphDocument::load(&directory)?;
    let placeholders: Vec<Id> = created.nodes.iter().map(|node| node.id).collect();
    apply_graph_changes(&directory, &collapse.function_nodes, &placeholders)?;
    update_function_signature(&directory, collapse.signature.clone())?;

    let id = created.manifest.id;
    let call_type = if class_directory.is_some() { method_node_id(id) } else { call_node_id(id) };
    let call = collapse.call.id;
    let (upserts, deletions) = collapse.graph_changes(call_type);
    apply_graph_changes(graph_path, &upserts, &deletions)?;
    Ok(CollapsedFunction { path: directory, id, call })
}
