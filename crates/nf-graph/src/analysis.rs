use std::collections::{HashMap, HashSet, VecDeque};

use nf_api::TypeExpr;
use nf_project::{GraphKind, Id, NodeDocument, PinValue, ScriptContext};
use serde::Serialize;

use crate::catalog::{NodeDefinition, NodeKind};
use crate::generics::{NodeMap, resolve_pin_type};
use crate::scope::{FUNCTION_ENTRY, GraphScope};
use crate::pin::PinDefinition;
use crate::types::{TypeCategory, TypeSystem};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    pub severity: DiagnosticSeverity,
    pub code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

struct Analyzer<'a> {
    scope: &'a GraphScope<'a>,
    context: ScriptContext,
    nodes: NodeMap<'a>,
    diagnostics: Vec<Diagnostic>,
}

fn pin_label(pin: &PinDefinition) -> String {
    if pin.label.is_empty() {
        pin.name.to_uppercase()
    } else {
        pin.label.clone()
    }
}

fn with_article(type_expr: &TypeExpr) -> String {
    let text = type_expr.to_string();
    let article = match text.chars().next() {
        Some(first) if "aeiouAEIOU".contains(first) => "an",
        _ => "a",
    };
    format!("{article} {text}")
}

fn base_type(type_expr: &TypeExpr) -> &TypeExpr {
    match type_expr {
        TypeExpr::Optional(inner) | TypeExpr::Variadic(inner) => base_type(inner),
        other => other,
    }
}

fn needs_connection(types: &TypeSystem, type_expr: &TypeExpr) -> bool {
    match base_type(type_expr) {
        TypeExpr::Named(name) => matches!(
            types.category(name),
            TypeCategory::Object | TypeCategory::Table | TypeCategory::Function
        ),
        TypeExpr::Array(_) => true,
        TypeExpr::Union(members) => members.iter().all(|member| needs_connection(types, member)),
        _ => false,
    }
}

impl<'a> Analyzer<'a> {
    fn report(
        &mut self,
        severity: DiagnosticSeverity,
        code: &'static str,
        node: Id,
        pin: Option<&str>,
        message: String,
        hint: Option<String>,
    ) {
        self.diagnostics.push(Diagnostic {
            severity,
            code,
            node: Some(node),
            pin: pin.map(str::to_owned),
            message,
            hint,
        });
    }

    fn title_of(&self, id: Id) -> String {
        match self.nodes.get(&id) {
            Some((_, Some(definition))) => format!("'{}'", definition.title),
            Some((node, None)) => format!("'{}'", node.node_type),
            None => "a node".to_owned(),
        }
    }

    fn check_node(&mut self, node: &'a NodeDocument, definition: &'a NodeDefinition) {
        let context = self.context;
        for warning in definition.context_warnings.iter().filter(|warning| warning.contexts.contains(&context)) {
            self.report(
                DiagnosticSeverity::Warning,
                "context-warning",
                node.id,
                None,
                warning.message.clone(),
                Some(warning.hint.clone()),
            );
        }
        if !definition.side.runs_in(self.context) {
            let message = match self.context {
                ScriptContext::Shared => format!(
                    "{} only works on the {}, but Shared graphs run on both server and client.",
                    self.title_of(node.id),
                    definition.side.display_name()
                ),
                _ => format!(
                    "{} only works on the {}, but this graph runs on the {}.",
                    self.title_of(node.id),
                    definition.side.display_name(),
                    self.context.display_name().to_lowercase()
                ),
            };
            self.report(
                DiagnosticSeverity::Error,
                "wrong-side",
                node.id,
                None,
                message,
                Some(format!(
                    "Move this logic into a {} graph, or send a remote event to the {} and handle it there.",
                    definition.side.display_name(),
                    definition.side.display_name()
                )),
            );
        }

        for (pin_name, value) in &node.inputs {
            let Some(pin) = definition.input(pin_name) else {
                self.report(
                    DiagnosticSeverity::Warning,
                    "stale-input",
                    node.id,
                    Some(pin_name),
                    format!(
                        "{} has no input called '{pin_name}' anymore. The stored value is ignored.",
                        self.title_of(node.id)
                    ),
                    Some("This usually happens after an API update. Remove the node and add it again to clean it up.".to_owned()),
                );
                continue;
            };
            match value {
                PinValue::Link(link) => self.check_data_link(node, pin, link.node, &link.pin),
                PinValue::Literal { literal } => self.check_literal(node, pin, literal),
            }
        }

        for pin in &definition.inputs {
            let Some(type_expr) = pin.pin_type.type_expr() else {
                continue;
            };
            if pin.optional || node.inputs.contains_key(&pin.name) || !needs_connection(self.scope.types(), type_expr) {
                continue;
            }
            self.report(
                DiagnosticSeverity::Error,
                "missing-input",
                node.id,
                Some(&pin.name),
                format!(
                    "The input '{}' of {} needs a connection.",
                    pin_label(pin),
                    self.title_of(node.id)
                ),
                Some(format!("Connect {} to it.", with_article(type_expr))),
            );
        }

        for (output_name, link) in &node.exec {
            let output_is_exec = definition.output(output_name).is_some_and(|pin| pin.pin_type.is_exec());
            if !output_is_exec {
                self.report(
                    DiagnosticSeverity::Warning,
                    "stale-output",
                    node.id,
                    Some(output_name),
                    format!(
                        "{} has no execution output called '{output_name}' anymore. The connection is ignored.",
                        self.title_of(node.id)
                    ),
                    None,
                );
                continue;
            }
            match self.nodes.get(&link.node) {
                None => self.report(
                    DiagnosticSeverity::Error,
                    "dangling-link",
                    node.id,
                    Some(output_name),
                    format!(
                        "The execution wire from {} leads to a node that no longer exists.",
                        self.title_of(node.id)
                    ),
                    Some("Reconnect the output or remove the wire.".to_owned()),
                ),
                Some((_, Some(target))) if !target.input(&link.pin).is_some_and(|pin| pin.pin_type.is_exec()) => {
                    self.report(
                        DiagnosticSeverity::Error,
                        "dangling-link",
                        node.id,
                        Some(output_name),
                        format!(
                            "The execution wire from {} points to '{}', which is not an execution input of {}.",
                            self.title_of(node.id),
                            link.pin,
                            self.title_of(link.node)
                        ),
                        None,
                    )
                }
                _ => {}
            }
        }
    }

    fn check_data_link(&mut self, node: &NodeDocument, pin: &PinDefinition, source_id: Id, source_pin: &str) {
        let Some(input_type) = self.resolved_type(node.id, pin) else {
            self.report(
                DiagnosticSeverity::Error,
                "invalid-link",
                node.id,
                Some(&pin.name),
                format!("'{}' is an execution pin and cannot hold a data connection.", pin_label(pin)),
                None,
            );
            return;
        };
        let Some((_, source_definition)) = self.nodes.get(&source_id).copied() else {
            self.report(
                DiagnosticSeverity::Error,
                "dangling-link",
                node.id,
                Some(&pin.name),
                format!(
                    "The input '{}' of {} is wired to a node that no longer exists.",
                    pin_label(pin),
                    self.title_of(node.id)
                ),
                Some("Reconnect the input or remove the wire.".to_owned()),
            );
            return;
        };
        let Some(source_definition) = source_definition else {
            return;
        };
        let Some(output) = source_definition.output(source_pin) else {
            self.report(
                DiagnosticSeverity::Error,
                "dangling-link",
                node.id,
                Some(&pin.name),
                format!(
                    "The input '{}' is wired to the output '{source_pin}' of {}, which does not exist anymore.",
                    pin_label(pin),
                    self.title_of(source_id)
                ),
                None,
            );
            return;
        };
        let Some(output_type) = self.resolved_type(source_id, output) else {
            self.report(
                DiagnosticSeverity::Error,
                "invalid-link",
                node.id,
                Some(&pin.name),
                format!("The input '{}' is wired to an execution output.", pin_label(pin)),
                None,
            );
            return;
        };
        let (output_type, input_type) = (&output_type, &input_type);
        let types = self.scope.types();
        if !types.is_assignable(output_type, input_type) {
            self.report(
                DiagnosticSeverity::Error,
                "type-mismatch",
                node.id,
                Some(&pin.name),
                format!(
                    "'{}' of {} is {}, but '{}' of {} needs {}.",
                    pin_label(output),
                    self.title_of(source_id),
                    with_article(output_type),
                    pin_label(pin),
                    self.title_of(node.id),
                    with_article(input_type)
                ),
                types.describe_mismatch(output_type, input_type),
            );
        }
    }

    fn resolved_type(&self, node: Id, pin: &PinDefinition) -> Option<TypeExpr> {
        resolve_pin_type(&self.nodes, node, pin)
    }

    fn literal_fits(&self, type_expr: &TypeExpr, literal: &toml::Value) -> bool {
        match base_type(type_expr) {
            TypeExpr::Union(members) => members.iter().any(|member| self.literal_fits(member, literal)),
            TypeExpr::Array(_) => false,
            TypeExpr::Named(name) => {
                let category = self.scope.types().category(name);
                match (category, literal) {
                    (TypeCategory::Any, _) => true,
                    (TypeCategory::Boolean, toml::Value::Boolean(_)) => true,
                    (TypeCategory::Number, toml::Value::Integer(_) | toml::Value::Float(_)) => true,
                    (TypeCategory::Integer, toml::Value::Integer(_)) => true,
                    (TypeCategory::Integer, toml::Value::Float(value)) => value.fract() == 0.0,
                    (TypeCategory::String | TypeCategory::AssetPath, toml::Value::String(_)) => true,
                    (TypeCategory::Enum, toml::Value::String(key)) => self.scope.enum_has_key(name, key),
                    (TypeCategory::Struct, toml::Value::Table(fields)) => fields
                        .values()
                        .all(|field| matches!(field, toml::Value::Integer(_) | toml::Value::Float(_))),
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn check_literal(&mut self, node: &NodeDocument, pin: &PinDefinition, literal: &toml::Value) {
        let Some(resolved) = self.resolved_type(node.id, pin) else {
            return;
        };
        let type_expr = &resolved;
        if self.literal_fits(type_expr, literal) {
            return;
        }
        let message = if needs_connection(self.scope.types(), type_expr) {
            format!(
                "The input '{}' of {} cannot be typed in, it needs a connection to {}.",
                pin_label(pin),
                self.title_of(node.id),
                with_article(type_expr)
            )
        } else {
            format!(
                "The value {literal} does not fit the input '{}' of {}, which needs {}.",
                pin_label(pin),
                self.title_of(node.id),
                with_article(type_expr)
            )
        };
        self.report(DiagnosticSeverity::Error, "invalid-literal", node.id, Some(&pin.name), message, None);
    }

    fn check_reachability(&mut self) {
        let mut reached: HashSet<Id> = HashSet::new();
        let mut queue: VecDeque<Id> = self
            .nodes
            .values()
            .filter(|(_, definition)| {
                definition.is_some_and(|definition| matches!(definition.kind, NodeKind::Event | NodeKind::FunctionEntry))
            })
            .map(|(node, _)| node.id)
            .collect();
        while let Some(id) = queue.pop_front() {
            if !reached.insert(id) {
                continue;
            }
            if let Some((node, _)) = self.nodes.get(&id) {
                queue.extend(node.exec.values().map(|link| link.node));
            }
        }

        let mut unreachable: Vec<Id> = self
            .nodes
            .values()
            .filter(|(node, definition)| {
                definition.is_some_and(NodeDefinition::has_exec_input) && !reached.contains(&node.id)
            })
            .map(|(node, _)| node.id)
            .collect();
        unreachable.sort();
        for id in unreachable {
            self.report(
                DiagnosticSeverity::Warning,
                "unreachable",
                id,
                None,
                format!("{} never runs, because no event leads to it.", self.title_of(id)),
                Some("Connect its execution input to an event or to another node that runs.".to_owned()),
            );
        }
    }

    fn check_data_cycles(&mut self) {
        #[derive(Clone, Copy, PartialEq)]
        enum Visit {
            Active,
            Done,
        }

        fn visit(
            id: Id,
            nodes: &HashMap<Id, (&NodeDocument, Option<&NodeDefinition>)>,
            state: &mut HashMap<Id, Visit>,
            cyclic: &mut HashSet<Id>,
        ) {
            state.insert(id, Visit::Active);
            if let Some((node, _)) = nodes.get(&id) {
                for value in node.inputs.values() {
                    let PinValue::Link(link) = value else {
                        continue;
                    };
                    match state.get(&link.node) {
                        Some(Visit::Active) => {
                            cyclic.insert(link.node);
                            cyclic.insert(id);
                        }
                        Some(Visit::Done) => {}
                        None if nodes.contains_key(&link.node) => visit(link.node, nodes, state, cyclic),
                        None => {}
                    }
                }
            }
            state.insert(id, Visit::Done);
        }

        let mut state = HashMap::new();
        let mut cyclic = HashSet::new();
        let mut ids: Vec<Id> = self.nodes.keys().copied().collect();
        ids.sort();
        for id in &ids {
            if !state.contains_key(id) {
                visit(*id, &self.nodes, &mut state, &mut cyclic);
            }
        }
        let mut cyclic: Vec<Id> = cyclic.into_iter().collect();
        cyclic.sort();
        for id in cyclic {
            self.report(
                DiagnosticSeverity::Error,
                "data-cycle",
                id,
                None,
                format!("{} depends on its own result through a loop of data wires.", self.title_of(id)),
                Some("Break the loop, for example by storing the value in a variable first.".to_owned()),
            );
        }
    }
}

fn missing_project_member(node_type: &str) -> Option<(&'static str, &'static str, &'static str)> {
    let lookup: [(&str, &str, &str, &str); 11] = [
        (
            "struct/",
            "missing-struct",
            "The struct this node uses no longer exists in this package.",
            "It was deleted. Delete this node or use another struct.",
        ),
        (
            "custom-event/",
            "missing-event",
            "The network event this node handles no longer exists in this package.",
            "It was deleted. Delete this node, or create the event again in the project tree.",
        ),
        (
            "send-event/",
            "missing-event",
            "The network event this node sends no longer exists in this package, or its direction changed.",
            "Delete this node and add the matching send node of the event again.",
        ),
        (
            "function/",
            "missing-function",
            "The function this node calls no longer exists in this package.",
            "It was deleted or lives in another package. Packages cannot call each other's functions directly, use events for that.",
        ),
        (
            "method/",
            "missing-method",
            "The method this node calls no longer exists.",
            "It was deleted, or its class was removed. Delete this node or point it to another method.",
        ),
        (
            "variable/",
            "missing-variable",
            "The variable this node uses no longer exists.",
            "It was deleted or belongs to another graph. Variables of a graph are only visible inside that graph, class members everywhere in the package.",
        ),
        (
            "class/spawn/",
            "missing-class",
            "The class this node spawns no longer exists in this package.",
            "It was deleted or renamed to another package. Delete this node or spawn another class.",
        ),
        (
            "macro/",
            "missing-macro",
            "The macro this node uses no longer exists.",
            "It was deleted. Delete this node or use another macro.",
        ),
        (
            "library/",
            "missing-library-function",
            "The library function this node calls is not available.",
            "Its package must be listed in the package requirements of this package (Package settings), and the function must still be exported there.",
        ),
        (
            "class-event/",
            "misplaced-class-event",
            "Class events only work inside the class graph they belong to.",
            "Move this node into the class graph, or use the normal event of the parent class.",
        ),
        (
            "class/self",
            "misplaced-self",
            "'Self' only exists inside a class graph or one of its methods.",
            "Delete this node or move the logic into the class.",
        ),
    ];
    lookup
        .into_iter()
        .find(|(prefix, ..)| node_type.starts_with(prefix))
        .map(|(_, code, message, hint)| (code, message, hint))
}

pub fn analyze_graph<'a>(
    scope: &'a GraphScope<'a>,
    context: ScriptContext,
    kind: GraphKind,
    nodes: &'a [NodeDocument],
) -> Vec<Diagnostic> {
    let definitions = crate::node_definitions(scope, nodes);
    let definition_of = |node: &NodeDocument| definitions.get(&node.id).map(|definition| definition.as_ref());
    let mut analyzer = Analyzer {
        scope,
        context,
        nodes: nodes.iter().map(|node| (node.id, (node, definition_of(node)))).collect(),
        diagnostics: Vec::new(),
    };

    for node in nodes {
        match definition_of(node) {
            Some(definition) => analyzer.check_node(node, definition),
            None if node.node_type == FUNCTION_ENTRY || node.node_type == crate::FUNCTION_RETURN => analyzer.report(
                DiagnosticSeverity::Error,
                "unknown-node",
                node.id,
                None,
                "Function entry and return nodes only work inside function graphs.".to_owned(),
                Some("Delete this node, or move the logic into a function graph.".to_owned()),
            ),
            None if node.node_type == crate::MACRO_INPUTS || node.node_type == crate::MACRO_OUTPUTS => analyzer.report(
                DiagnosticSeverity::Error,
                "unknown-node",
                node.id,
                None,
                "Macro inputs and outputs nodes only work inside macro graphs.".to_owned(),
                Some("Delete this node, or move the logic into a macro graph.".to_owned()),
            ),
            None if let Some((code, message, hint)) = missing_project_member(&node.node_type) => analyzer.report(
                DiagnosticSeverity::Error,
                code,
                node.id,
                None,
                message.to_owned(),
                Some(hint.to_owned()),
            ),
            None => analyzer.report(
                DiagnosticSeverity::Error,
                "unknown-node",
                node.id,
                None,
                format!("The node type '{}' is not known.", node.node_type),
                Some("It may come from a newer API version or was removed from nanos world. Switch the API channel in the project settings or delete the node.".to_owned()),
            ),
        }
    }

    if kind == GraphKind::Function {
        let entries: Vec<&NodeDocument> = nodes.iter().filter(|node| node.node_type == FUNCTION_ENTRY).collect();
        if entries.is_empty() {
            analyzer.diagnostics.push(Diagnostic {
                severity: DiagnosticSeverity::Error,
                code: "missing-entry",
                node: None,
                pin: None,
                message: "This function has no entry node, so nothing in it can run.".to_owned(),
                hint: Some("Add the entry node again from the node menu (search for the function name).".to_owned()),
            });
        }
        for extra in entries.iter().skip(1) {
            analyzer.report(
                DiagnosticSeverity::Error,
                "duplicate-entry",
                extra.id,
                None,
                "A function can only have one entry node.".to_owned(),
                Some("Delete the extra entry node.".to_owned()),
            );
        }
    }
    if kind == GraphKind::Macro {
        for node_type in [crate::MACRO_INPUTS, crate::MACRO_OUTPUTS] {
            let mut copies = nodes.iter().filter(|node| node.node_type == node_type).skip(1);
            for extra in copies.by_ref() {
                analyzer.report(
                    DiagnosticSeverity::Error,
                    "duplicate-entry",
                    extra.id,
                    None,
                    format!("A macro can only have one {} node.", if node_type == crate::MACRO_INPUTS { "Inputs" } else { "Outputs" }),
                    Some("Delete the extra node and connect everything to the first one.".to_owned()),
                );
            }
        }
    }
    analyzer.check_reachability();
    analyzer.check_data_cycles();
    analyzer.diagnostics
}

#[cfg(test)]
mod tests {
    use super::*;
    use nf_api::{ApiChannel, ApiDatabase};
    use crate::NodeCatalog;
    use nf_project::PinLink;

    fn catalog() -> NodeCatalog {
        NodeCatalog::from_database(&ApiDatabase::bundled(ApiChannel::Stable).unwrap())
    }

    fn analyze(catalog: &NodeCatalog, context: ScriptContext, nodes: &[NodeDocument]) -> Vec<Diagnostic> {
        let scope = GraphScope::new(catalog, crate::ScopeData::default());
        analyze_graph(&scope, context, GraphKind::EventGraph, nodes)
    }

    fn link(node: &NodeDocument, pin: &str) -> PinLink {
        PinLink {
            node: node.id,
            pin: pin.to_owned(),
        }
    }

    fn codes(diagnostics: &[Diagnostic]) -> Vec<&'static str> {
        diagnostics.iter().map(|diagnostic| diagnostic.code).collect()
    }

    #[test]
    fn a_valid_graph_has_no_diagnostics() {
        let catalog = catalog();
        let mut ready = NodeDocument::new("event/Player/Ready", [0.0, 0.0]);
        let mut message = NodeDocument::new("static/Chat/BroadcastMessage", [300.0, 0.0]);
        message.inputs.insert(
            "message".to_owned(),
            PinValue::Literal {
                literal: toml::Value::String("Welcome!".to_owned()),
            },
        );
        ready.exec.insert("then".to_owned(), link(&message, "exec"));

        let diagnostics = analyze(&catalog, ScriptContext::Server, &[ready, message]);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");
    }

    #[test]
    fn reports_server_only_nodes_in_client_graphs() {
        let catalog = catalog();
        let ready = NodeDocument::new("event/Package/Load", [0.0, 0.0]);
        let mut broadcast = NodeDocument::new("static/Chat/BroadcastMessage", [300.0, 0.0]);
        broadcast.inputs.insert(
            "message".to_owned(),
            PinValue::Literal {
                literal: toml::Value::String("Hi".to_owned()),
            },
        );
        let mut ready = ready;
        ready.exec.insert("then".to_owned(), link(&broadcast, "exec"));

        let diagnostics = analyze(&catalog, ScriptContext::Client, &[ready, broadcast]);
        assert_eq!(codes(&diagnostics), ["wrong-side"]);
        assert!(diagnostics[0].message.contains("only works on the server"));
    }

    #[test]
    fn reports_type_mismatches_with_a_hint() {
        let catalog = catalog();
        let mut death = NodeDocument::new("event/Prop/Destroy", [0.0, 0.0]);
        let mut set_health = NodeDocument::new("call/Damageable/SetHealth", [300.0, 0.0]);
        set_health.inputs.insert("self".to_owned(), PinValue::Link(link(&death, "self")));
        set_health
            .inputs
            .insert("new_health".to_owned(), PinValue::Literal { literal: toml::Value::Integer(10) });
        death.exec.insert("then".to_owned(), link(&set_health, "exec"));

        let diagnostics = analyze(&catalog, ScriptContext::Server, &[death, set_health]);
        assert!(codes(&diagnostics).contains(&"type-mismatch"), "{diagnostics:#?}");
    }

    #[test]
    fn reports_missing_targets_and_unreachable_nodes() {
        let catalog = catalog();
        let set_health = NodeDocument::new("call/Damageable/SetHealth", [300.0, 0.0]);
        let diagnostics = analyze(&catalog, ScriptContext::Server, &[set_health]);
        let codes = codes(&diagnostics);
        assert!(codes.contains(&"missing-input"));
        assert!(codes.contains(&"unreachable"));
    }

    #[test]
    fn reports_unknown_nodes_and_bad_literals() {
        let catalog = catalog();
        let unknown = NodeDocument::new("call/Nope/Missing", [0.0, 0.0]);
        let mut literal = NodeDocument::new("literal/integer", [0.0, 200.0]);
        literal.inputs.insert(
            "value".to_owned(),
            PinValue::Literal {
                literal: toml::Value::String("ten".to_owned()),
            },
        );
        let diagnostics = analyze(&catalog, ScriptContext::Server, &[unknown, literal]);
        assert_eq!(codes(&diagnostics), ["unknown-node", "invalid-literal"]);
    }

    #[test]
    fn detects_data_cycles() {
        let catalog = catalog();
        let mut first = NodeDocument::new("math/add", [0.0, 0.0]);
        let mut second = NodeDocument::new("math/add", [200.0, 0.0]);
        second.inputs.insert("a".to_owned(), PinValue::Link(link(&first, "result")));
        first.inputs.insert("a".to_owned(), PinValue::Link(link(&second, "result")));
        let diagnostics = analyze(&catalog, ScriptContext::Server, &[first, second]);
        assert_eq!(codes(&diagnostics), ["data-cycle", "data-cycle"]);
    }
}
