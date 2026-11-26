use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nf_graph::{Diagnostic, GraphScope, NodeDefinition, PinDefinition, PinType};
use nf_project::{GraphDocument, GraphSummary, Id, NodeDocument, PackageSummary, PinValue, Project, ScriptContext};
use serde::Serialize;
use serde_json::{Value, json};

const DESCRIPTION_LIMIT: usize = 180;

pub struct GraphTarget {
    pub package: PackageSummary,
    pub graph: GraphSummary,
}

pub fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn shorten(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= DESCRIPTION_LIMIT {
        return text.to_owned();
    }
    let cut: String = text.chars().take(DESCRIPTION_LIMIT).collect();
    format!("{}...", cut.trim_end())
}

pub fn find_graph(project: &Project, query: Option<&str>, open_graph: Option<&Path>) -> Result<GraphTarget, String> {
    let wanted: Option<String> = query
        .map(str::trim)
        .filter(|query| !query.is_empty())
        .map(|query| query.replace('\\', "/").trim_end_matches('/').to_owned());
    let open_graph = open_graph.map(Path::to_path_buf);
    for package in project.packages() {
        for graph in &package.graphs {
            let matches = match &wanted {
                Some(wanted) => {
                    graph.id.to_string().eq_ignore_ascii_case(wanted)
                        || relative(project.root(), &graph.path) == *wanted
                        || Path::new(wanted) == graph.path
                }
                None => open_graph.as_ref() == Some(&graph.path),
            };
            if matches {
                return Ok(GraphTarget {
                    package: package.clone(),
                    graph: graph.clone(),
                });
            }
        }
    }
    Err(match wanted {
        Some(wanted) => format!("There is no graph '{wanted}' in this project. Use get_project to list the graphs and their paths."),
        None => "No graph is open in the editor. Pass the graph path from get_project.".to_owned(),
    })
}

pub fn project_overview(project: &Project, open_graph: Option<&Path>) -> Value {
    let root = project.root();
    let packages: Vec<Value> = project
        .packages()
        .iter()
        .map(|package| {
            let graphs: Vec<Value> = package
                .graphs
                .iter()
                .map(|graph| {
                    let mut entry = json!({
                        "path": relative(root, &graph.path),
                        "name": graph.name,
                        "kind": graph.kind,
                        "context": graph.context,
                        "nodes": graph.node_count,
                    });
                    if !graph.folders.is_empty() {
                        entry["folders"] = json!(graph.folders);
                    }
                    if let Some(parent) = &graph.parent {
                        entry["parentClass"] = json!(parent);
                    }
                    if graph.owner.is_some() {
                        entry["member"] = json!(true);
                    }
                    entry
                })
                .collect();
            json!({
                "folder": package.folder,
                "path": relative(root, &package.path),
                "title": package.title,
                "type": package.package_type,
                "graphs": graphs,
            })
        })
        .collect();
    json!({
        "name": project.manifest().name,
        "apiChannel": project.manifest().api_channel,
        "openGraph": open_graph.map(|path| relative(root, path)),
        "packages": packages,
    })
}

fn type_label(pin: &PinDefinition) -> String {
    match &pin.pin_type {
        PinType::Exec => "exec".to_owned(),
        PinType::Data { type_expr } => type_expr.to_string(),
    }
}

fn pin_summary(pins: &[PinDefinition]) -> String {
    pins.iter().map(|pin| format!("{}: {}", pin.name, type_label(pin))).collect::<Vec<_>>().join(", ")
}

fn literal_json(value: &toml::Value) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

fn link_text(node: Id, pin: &str) -> String {
    format!("{node}.{pin}")
}

pub fn graph_view(project: &Project, target: &GraphTarget, document: &GraphDocument, scope: &GraphScope<'_>, diagnostics: &[Diagnostic]) -> Value {
    let definitions = nf_graph::node_definitions(scope, &document.nodes);
    let mut outgoing: BTreeMap<(Id, String), Vec<String>> = BTreeMap::new();
    for node in &document.nodes {
        for (pin, value) in &node.inputs {
            if let PinValue::Link(link) = value {
                outgoing.entry((link.node, link.pin.clone())).or_default().push(link_text(node.id, pin));
            }
        }
    }

    let nodes: Vec<Value> = document
        .nodes
        .iter()
        .map(|node| {
            let definition = definitions.get(&node.id).map(|definition| definition.as_ref());
            node_view(node, definition, &outgoing)
        })
        .collect();

    let mut view = json!({
        "graph": {
            "id": target.graph.id,
            "path": relative(project.root(), &target.graph.path),
            "name": target.graph.name,
            "kind": target.graph.kind,
            "context": target.graph.context,
            "package": target.package.folder,
        },
        "nodes": nodes,
    });
    let manifest = &document.manifest;
    if !manifest.signature.parameters.is_empty() || !manifest.signature.returns.is_empty() {
        view["graph"]["signature"] = json!({
            "parameters": manifest.signature.parameters.iter().map(|pin| json!({"name": pin.name, "type": pin.type_name})).collect::<Vec<_>>(),
            "returns": manifest.signature.returns.iter().map(|pin| json!({"name": pin.name, "type": pin.type_name})).collect::<Vec<_>>(),
        });
    }
    if let Some(parent) = &manifest.parent {
        view["graph"]["parentClass"] = json!(parent);
    }
    if !document.variables.is_empty() {
        view["variables"] = json!(
            document
                .variables
                .iter()
                .map(|variable| {
                    let mut entry = json!({"name": variable.name, "type": variable.type_name});
                    if let Some(default) = &variable.default {
                        entry["default"] = literal_json(default);
                    }
                    entry
                })
                .collect::<Vec<_>>()
        );
    }
    if !diagnostics.is_empty() {
        view["diagnostics"] = json!(diagnostics);
    }
    if !document.load_errors.is_empty() {
        view["loadErrors"] = json!(document.load_errors.iter().map(|error| &error.message).collect::<Vec<_>>());
    }
    view
}

fn node_view(node: &NodeDocument, definition: Option<&NodeDefinition>, outgoing: &BTreeMap<(Id, String), Vec<String>>) -> Value {
    let mut view = json!({
        "id": node.id,
        "type": node.node_type,
        "position": node.position,
    });
    let Some(definition) = definition else {
        view["unknownType"] = json!(true);
        return view;
    };
    view["title"] = json!(definition.title);
    if !node.properties.is_empty() {
        view["properties"] = serde_json::to_value(&node.properties).unwrap_or(Value::Null);
    }

    let inputs: Vec<Value> = definition
        .inputs
        .iter()
        .map(|pin| {
            let mut entry = json!({"pin": pin.name, "type": type_label(pin)});
            match node.inputs.get(&pin.name) {
                Some(PinValue::Link(link)) => entry["from"] = json!(link_text(link.node, &link.pin)),
                Some(PinValue::Literal { literal }) => entry["value"] = literal_json(literal),
                None => {
                    if let Some(default) = &pin.default {
                        entry["default"] = json!(default);
                    }
                }
            }
            entry
        })
        .collect();
    let outputs: Vec<Value> = definition
        .outputs
        .iter()
        .map(|pin| {
            let mut entry = json!({"pin": pin.name, "type": type_label(pin)});
            if pin.pin_type.is_exec() {
                if let Some(link) = node.exec.get(&pin.name) {
                    entry["to"] = json!([link_text(link.node, &link.pin)]);
                }
            } else if let Some(targets) = outgoing.get(&(node.id, pin.name.clone())) {
                entry["to"] = json!(targets);
            }
            entry
        })
        .collect();
    if !inputs.is_empty() {
        view["inputs"] = json!(inputs);
    }
    if !outputs.is_empty() {
        view["outputs"] = json!(outputs);
    }
    view
}

#[derive(Serialize)]
struct SearchEntry {
    #[serde(rename = "type")]
    node_type: String,
    title: String,
    category: String,
    side: String,
    inputs: String,
    outputs: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    description: String,
}

fn score(definition: &NodeDefinition, terms: &[String]) -> usize {
    let title = definition.title.to_lowercase();
    let id = definition.id.to_lowercase();
    let category = definition.category.to_lowercase();
    let subtitle = definition.subtitle.as_deref().unwrap_or_default().to_lowercase();
    let mut total = 0;
    for term in terms {
        let term_score = if title == *term {
            12
        } else if title.starts_with(term.as_str()) {
            8
        } else if title.contains(term.as_str()) {
            6
        } else if id.contains(term.as_str()) || subtitle.contains(term.as_str()) {
            4
        } else if definition.keywords.iter().any(|keyword| keyword.to_lowercase().contains(term.as_str())) || category.contains(term.as_str()) {
            3
        } else if definition.description.to_lowercase().contains(term.as_str()) {
            1
        } else {
            return 0;
        };
        total += term_score;
    }
    total
}

pub fn search(scope: &GraphScope<'_>, context: Option<ScriptContext>, query: &str, limit: usize) -> Value {
    let terms: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut hits: Vec<(usize, &NodeDefinition)> = scope
        .definitions()
        .filter(|definition| context.is_none_or(|context| definition.side.runs_in(context)))
        .map(|definition| (score(definition, &terms), definition))
        .filter(|(score, _)| terms.is_empty() || *score > 0)
        .collect();
    hits.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.title.len().cmp(&right.1.title.len())));
    let total = hits.len();
    let entries: Vec<SearchEntry> = hits
        .into_iter()
        .take(limit)
        .map(|(_, definition)| SearchEntry {
            node_type: definition.id.clone(),
            title: match &definition.subtitle {
                Some(subtitle) => format!("{} ({subtitle})", definition.title),
                None => definition.title.clone(),
            },
            category: definition.category.clone(),
            side: definition.side.display_name().to_owned(),
            inputs: pin_summary(&definition.inputs),
            outputs: pin_summary(&definition.outputs),
            description: shorten(&definition.description),
        })
        .collect();
    json!({"total": total, "results": entries})
}

pub fn node_type_view(definition: &NodeDefinition) -> Value {
    let pins = |pins: &[PinDefinition]| -> Vec<Value> {
        pins.iter()
            .map(|pin| {
                let mut entry = json!({"pin": pin.name, "label": pin.label, "type": type_label(pin)});
                if !pin.description.is_empty() {
                    entry["description"] = json!(pin.description.trim());
                }
                if let Some(default) = &pin.default {
                    entry["default"] = json!(default);
                }
                if pin.optional {
                    entry["optional"] = json!(true);
                }
                entry
            })
            .collect()
    };
    let mut view = json!({
        "type": definition.id,
        "title": definition.title,
        "category": definition.category,
        "kind": definition.kind,
        "side": definition.side.display_name(),
        "description": definition.description.trim(),
        "inputs": pins(&definition.inputs),
        "outputs": pins(&definition.outputs),
    });
    if let Some(dynamic) = &definition.dynamic {
        view["dynamicPins"] = json!(dynamic);
        view["dynamicHint"] = json!(
            "Pins of this node depend on its properties. Set the `count` property to change how many numbered pins it has, then call get_graph to see the expanded pins."
        );
    }
    if !definition.context_warnings.is_empty() {
        view["contextWarnings"] = json!(definition.context_warnings.iter().map(|warning| &warning.message).collect::<Vec<_>>());
    }
    view
}

pub fn changed_paths(graph_path: &Path, upserts: &[NodeDocument], deletions: &[Id]) -> Vec<PathBuf> {
    upserts
        .iter()
        .map(|node| node.id)
        .chain(deletions.iter().copied())
        .map(|id| NodeDocument::path_for(graph_path, id))
        .collect()
}
