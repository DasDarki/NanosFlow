use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nf_project::{GraphDocument, GraphKind, GraphManifest, GraphSummary, Id, NodeDocument, PinValue, ProjectError, VariableDocument, scan_package_tree};
use serde::Serialize;

use crate::builtins::COMMENT;
use crate::catalog::NodeCatalog;
use crate::text::humanize;

const MAXIMUM_HITS: usize = 300;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum HitKind {
    Graph,
    Node,
    Comment,
    Literal,
    Variable,
    Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub kind: HitKind,
    pub graph_id: Id,
    pub graph_name: String,
    pub graph_path: PathBuf,
    pub location: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<Id>,
    pub title: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Usages {
    pub subject: String,
    pub subject_kind: String,
    pub hits: Vec<SearchHit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub hits: Vec<SearchHit>,
    pub truncated: bool,
}

struct LoadedGraph {
    summary: GraphSummary,
    manifest: GraphManifest,
    nodes: Vec<NodeDocument>,
    variables: Vec<VariableDocument>,
}

struct Package {
    graphs: Vec<LoadedGraph>,
    names: HashMap<Id, String>,
    variables: HashMap<Id, String>,
}

impl Package {
    fn load(package_directory: &Path) -> Result<Package, ProjectError> {
        let mut graphs = Vec::new();
        for summary in scan_package_tree(package_directory)?.graphs {
            let document = GraphDocument::load(&summary.path)?;
            graphs.push(LoadedGraph {
                summary,
                manifest: document.manifest,
                nodes: document.nodes,
                variables: document.variables,
            });
        }
        graphs.sort_by(|left, right| left.summary.path.cmp(&right.summary.path));
        let mut names: HashMap<Id, String> = graphs.iter().map(|graph| (graph.summary.id, graph.summary.name.clone())).collect();
        if package_directory.join(nf_project::PackageManifest::FILE_NAME).is_file() {
            for library in crate::required_libraries(package_directory)? {
                names.extend(library.functions.into_iter().map(|function| (function.graph.id, function.graph.name)));
            }
        }
        let variables = graphs
            .iter()
            .flat_map(|graph| graph.variables.iter().map(|variable| (variable.id, variable.name.clone())))
            .collect();
        Ok(Package { graphs, names, variables })
    }

    fn location(&self, summary: &GraphSummary) -> String {
        let mut parts = vec![summary.context.display_name().to_owned()];
        parts.extend(summary.folders.iter().cloned());
        if let Some(owner) = summary.owner.and_then(|owner| self.names.get(&owner)) {
            parts.push(owner.clone());
        }
        parts.push(summary.name.clone());
        parts.join(" / ")
    }

    fn hit(&self, graph: &LoadedGraph, kind: HitKind, node: Option<Id>, title: String, detail: String) -> SearchHit {
        SearchHit {
            kind,
            graph_id: graph.summary.id,
            graph_name: graph.summary.name.clone(),
            graph_path: graph.summary.path.clone(),
            location: self.location(&graph.summary),
            node,
            title,
            detail,
        }
    }

    fn name_of(&self, text: &str) -> Option<&str> {
        let id: Id = text.parse().ok()?;
        self.names.get(&id).or_else(|| self.variables.get(&id)).map(String::as_str)
    }

    fn node_title(&self, catalog: &NodeCatalog, node_type: &str) -> String {
        let segments: Vec<&str> = node_type.split('/').collect();
        let project = match segments.as_slice() {
            ["function", id] | ["method", id] | ["macro", id] | ["library", id] => self.name_of(id).map(str::to_owned),
            ["class", "spawn", id] => self.name_of(id).map(|name| format!("Spawn {name}")),
            ["custom-event", id] => self.name_of(id).map(|name| format!("On {name}")),
            ["send-event", id, ..] => self.name_of(id).map(|name| format!("Send {name}")),
            ["struct", "make", id] => self.name_of(id).map(|name| format!("Make {name}")),
            ["struct", "break", id] => self.name_of(id).map(|name| format!("Break {name}")),
            ["struct", "set", id] => self.name_of(id).map(|name| format!("Set {name} Fields")),
            ["variable", "get", id] => self.name_of(id).map(|name| format!("Get {name}")),
            ["variable", "set", id] => self.name_of(id).map(|name| format!("Set {name}")),
            ["class-event", event] => Some(format!("On {}", humanize(event))),
            ["cast", class] => Some(format!("Cast to {class}")),
            ["is-a", class] => Some(format!("Is {class}")),
            _ => None,
        };
        project
            .or_else(|| catalog.get(node_type).map(|definition| definition.title.clone()))
            .unwrap_or_else(|| humanize(segments.last().copied().unwrap_or(node_type)))
    }
}

fn graph_kind_label(kind: GraphKind, is_method: bool) -> &'static str {
    match kind {
        GraphKind::EventGraph => "Event graph",
        GraphKind::Function if is_method => "Method",
        GraphKind::Function => "Function",
        GraphKind::Class => "Class",
        GraphKind::Event => "Network event",
        GraphKind::Struct => "Struct",
        GraphKind::Macro => "Macro",
        GraphKind::Enum => "Enum",
        GraphKind::Dispatcher => "Event dispatcher",
        GraphKind::Interface => "Interface",
    }
}

fn mentions_type(expression: &str, name: &str) -> bool {
    expression
        .split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .any(|word| word == name)
}

fn literal_text(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(text) => Some(text.clone()),
        toml::Value::Integer(number) => Some(number.to_string()),
        toml::Value::Float(number) => Some(number.to_string()),
        _ => None,
    }
}

pub fn search_package(catalog: &NodeCatalog, package_directory: &Path, query: &str) -> Result<SearchResult, ProjectError> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Ok(SearchResult { hits: Vec::new(), truncated: false });
    }
    let package = Package::load(package_directory)?;
    let matches = |text: &str| text.to_lowercase().contains(&needle);
    let mut hits = Vec::new();
    for graph in &package.graphs {
        if matches(&graph.summary.name) {
            let label = graph_kind_label(graph.summary.kind, graph.summary.owner.is_some());
            hits.push(package.hit(graph, HitKind::Graph, None, graph.summary.name.clone(), label.to_owned()));
        }
        for variable in &graph.variables {
            if matches(&variable.name) {
                hits.push(package.hit(graph, HitKind::Variable, None, variable.name.clone(), format!("Variable, {}", variable.type_name)));
            }
        }
        for node in &graph.nodes {
            if node.node_type == COMMENT {
                if let Some(text) = node.properties.get("text").and_then(toml::Value::as_str).filter(|text| matches(text)) {
                    hits.push(package.hit(graph, HitKind::Comment, Some(node.id), text.to_owned(), "Comment".to_owned()));
                }
                continue;
            }
            let title = package.node_title(catalog, &node.node_type);
            if matches(&title) {
                hits.push(package.hit(graph, HitKind::Node, Some(node.id), title.clone(), "Node".to_owned()));
                continue;
            }
            let literal = node.inputs.iter().find_map(|(pin, value)| match value {
                PinValue::Literal { literal } => literal_text(literal).filter(|text| matches(text)).map(|text| (pin, text)),
                PinValue::Link(_) => None,
            });
            if let Some((pin, text)) = literal {
                hits.push(package.hit(graph, HitKind::Literal, Some(node.id), format!("\"{text}\""), format!("{} of {title}", humanize(pin))));
            }
        }
    }
    let truncated = hits.len() > MAXIMUM_HITS;
    hits.truncate(MAXIMUM_HITS);
    Ok(SearchResult { hits, truncated })
}

fn usage_node_types(target: Id, kind: GraphKind, is_method: bool) -> Vec<String> {
    match kind {
        GraphKind::Function if is_method => vec![format!("method/{target}")],
        GraphKind::Function => vec![format!("function/{target}")],
        GraphKind::Class => vec![format!("class/spawn/{target}")],
        GraphKind::Event => vec![format!("custom-event/{target}"), format!("send-event/{target}"), format!("send-event/{target}/player")],
        GraphKind::Struct => ["make", "break", "set"].iter().map(|action| format!("struct/{action}/{target}")).collect(),
        GraphKind::Macro => vec![format!("macro/{target}")],
        GraphKind::Enum => Vec::new(),
        GraphKind::Dispatcher => ["call", "bind", "unbind", "unbind-all"].iter().map(|action| format!("dispatcher/{action}/{target}")).collect(),
        GraphKind::Interface => vec![format!("interface/implements/{target}")],
        GraphKind::EventGraph => Vec::new(),
    }
}

pub fn find_usages(catalog: &NodeCatalog, package_directory: &Path, target: Id) -> Result<Usages, ProjectError> {
    let package = Package::load(package_directory)?;
    let mut hits = Vec::new();

    if let Some(name) = package.variables.get(&target) {
        let types = [format!("variable/get/{target}"), format!("variable/set/{target}")];
        for graph in &package.graphs {
            for node in graph.nodes.iter().filter(|node| types.contains(&node.node_type)) {
                let title = package.node_title(catalog, &node.node_type);
                let detail = if node.node_type.starts_with("variable/set/") { "Writes the value" } else { "Reads the value" };
                hits.push(package.hit(graph, HitKind::Node, Some(node.id), title, detail.to_owned()));
            }
        }
        return Ok(Usages {
            subject: name.clone(),
            subject_kind: "Variable".to_owned(),
            hits,
        });
    }

    let Some(subject) = package.graphs.iter().find(|graph| graph.summary.id == target) else {
        let library_call = crate::library_node_id(target);
        for graph in &package.graphs {
            for node in graph.nodes.iter().filter(|node| node.node_type == library_call) {
                let title = package.node_title(catalog, &node.node_type);
                hits.push(package.hit(graph, HitKind::Node, Some(node.id), title, "Library call".to_owned()));
            }
        }
        return Ok(Usages {
            subject: String::new(),
            subject_kind: String::new(),
            hits,
        });
    };
    let kind = subject.summary.kind;
    let is_method = subject.summary.owner.is_some();
    let name = subject.summary.name.clone();
    let node_types = usage_node_types(target, kind, is_method);
    let is_type = matches!(kind, GraphKind::Class | GraphKind::Struct | GraphKind::Enum | GraphKind::Interface);
    let interface_call = format!("interface/call/{target}/");
    let type_node_types = [format!("cast/{name}"), format!("is-a/{name}"), format!("flow/switch-enum/{name}")];

    for graph in &package.graphs {
        for node in &graph.nodes {
            let direct = node_types.contains(&node.node_type) || (kind == GraphKind::Interface && node.node_type.starts_with(&interface_call));
            if direct || (matches!(kind, GraphKind::Class | GraphKind::Enum) && type_node_types.contains(&node.node_type)) {
                let title = package.node_title(catalog, &node.node_type);
                hits.push(package.hit(graph, HitKind::Node, Some(node.id), title, "Node".to_owned()));
            }
        }
        if !is_type || graph.summary.id == target {
            continue;
        }
        if graph.manifest.interfaces.contains(&name) {
            hits.push(package.hit(graph, HitKind::Type, None, graph.summary.name.clone(), format!("Implements {name}")));
        }
        if graph.manifest.parent.as_deref() == Some(name.as_str()) {
            hits.push(package.hit(graph, HitKind::Type, None, graph.summary.name.clone(), format!("Inherits from {name}")));
        }
        let field_word = if graph.summary.kind == GraphKind::Struct { "Field" } else { "Parameter" };
        for pin in &graph.manifest.signature.parameters {
            if mentions_type(&pin.type_name, &name) {
                hits.push(package.hit(graph, HitKind::Type, None, pin.name.clone(), format!("{field_word}, {}", pin.type_name)));
            }
        }
        for pin in &graph.manifest.signature.returns {
            if mentions_type(&pin.type_name, &name) {
                hits.push(package.hit(graph, HitKind::Type, None, pin.name.clone(), format!("Return value, {}", pin.type_name)));
            }
        }
        for variable in &graph.variables {
            if mentions_type(&variable.type_name, &name) {
                hits.push(package.hit(graph, HitKind::Type, None, variable.name.clone(), format!("Variable, {}", variable.type_name)));
            }
        }
    }
    Ok(Usages {
        subject: name,
        subject_kind: graph_kind_label(kind, is_method).to_owned(),
        hits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nf_api::{ApiChannel, ApiDatabase};
    use nf_project::{
        FunctionPin, FunctionSignature, PinLink, ScriptContext, apply_graph_changes, apply_variable_changes, context_root, create_class,
        create_graph, update_function_signature,
    };

    fn catalog() -> NodeCatalog {
        NodeCatalog::from_database(&ApiDatabase::bundled(ApiChannel::Stable).unwrap())
    }

    #[test]
    fn finds_nodes_comments_literals_and_every_usage() {
        let catalog = catalog();
        let temporary = tempfile::tempdir().unwrap();
        let package = temporary.path();
        let shared = context_root(package, ScriptContext::Shared);
        let zombie = create_class(&shared, "Zombie", "Character").unwrap();
        let zombie_id = GraphDocument::load(&zombie).unwrap().manifest.id;
        create_class(&shared, "ZombieBoss", "Zombie").unwrap();
        let heal = create_graph(&shared, "Heal Zombie", GraphKind::Function).unwrap();
        let heal_id = GraphDocument::load(&heal).unwrap().manifest.id;
        update_function_signature(
            &heal,
            FunctionSignature {
                parameters: vec![FunctionPin {
                    id: "target".to_owned(),
                    name: "Target".to_owned(),
                    type_name: "Zombie[]".to_owned(),
                }],
                ..FunctionSignature::default()
            },
        )
        .unwrap();

        let main = create_graph(&context_root(package, ScriptContext::Server), "Main", GraphKind::EventGraph).unwrap();
        let score = VariableDocument {
            id: Id::generate(),
            name: "Score".to_owned(),
            type_name: "integer".to_owned(),
            default: None,
            description: String::new(),
            sync: false,
        };
        apply_variable_changes(&main, std::slice::from_ref(&score), &[]).unwrap();
        let load = NodeDocument::new("event/Package/Load", [0.0, 0.0]);
        let mut call = NodeDocument::new(format!("function/{heal_id}"), [200.0, 0.0]);
        let spawn = NodeDocument::new(format!("class/spawn/{zombie_id}"), [400.0, 0.0]);
        let cast = NodeDocument::new("cast/Zombie", [600.0, 0.0]);
        let read = NodeDocument::new(format!("variable/get/{}", score.id), [800.0, 0.0]);
        let mut print = NodeDocument::new("debug/print", [1000.0, 0.0]);
        print.inputs.insert("value".to_owned(), PinValue::Literal { literal: toml::Value::String("Wave cleared".to_owned()) });
        let mut comment = NodeDocument::new(COMMENT, [0.0, 300.0]);
        comment.properties.insert("text".to_owned(), toml::Value::String("Healing logic".to_owned()));
        call.exec.insert("then".to_owned(), PinLink { node: spawn.id, pin: "exec".to_owned() });
        let nodes = [load, call.clone(), spawn.clone(), cast.clone(), read.clone(), print.clone(), comment.clone()];
        apply_graph_changes(&main, &nodes, &[]).unwrap();

        let titles = |query: &str| -> Vec<(HitKind, String)> {
            search_package(&catalog, package, query).unwrap().hits.into_iter().map(|hit| (hit.kind, hit.title)).collect()
        };
        let healing = titles("heal");
        assert!(healing.contains(&(HitKind::Graph, "Heal Zombie".to_owned())));
        assert!(healing.contains(&(HitKind::Node, "Heal Zombie".to_owned())));
        assert!(healing.contains(&(HitKind::Comment, "Healing logic".to_owned())));
        assert_eq!(titles("wave"), [(HitKind::Literal, "\"Wave cleared\"".to_owned())]);
        assert!(titles("score").contains(&(HitKind::Node, "Get Score".to_owned())));
        assert!(titles("score").contains(&(HitKind::Variable, "Score".to_owned())));
        assert!(titles("   ").is_empty());

        let function = find_usages(&catalog, package, heal_id).unwrap();
        assert_eq!(function.subject_kind, "Function");
        assert_eq!(function.hits.iter().map(|hit| hit.node).collect::<Vec<_>>(), [Some(call.id)]);
        assert_eq!(function.hits[0].location, "Server / Main");

        let variable = find_usages(&catalog, package, score.id).unwrap();
        assert_eq!(variable.hits.iter().map(|hit| hit.node).collect::<Vec<_>>(), [Some(read.id)]);

        let class = find_usages(&catalog, package, zombie_id).unwrap();
        let found: Vec<(Option<Id>, String)> = class.hits.iter().map(|hit| (hit.node, hit.detail.clone())).collect();
        assert!(found.contains(&(Some(spawn.id), "Node".to_owned())));
        assert!(found.contains(&(Some(cast.id), "Node".to_owned())));
        assert!(found.contains(&(None, "Parameter, Zombie[]".to_owned())));
        assert!(found.contains(&(None, "Inherits from Zombie".to_owned())));
        assert!(!class.hits.iter().any(|hit| hit.title == "ZombieBoss" && hit.detail.contains("Parameter")));
    }
}
