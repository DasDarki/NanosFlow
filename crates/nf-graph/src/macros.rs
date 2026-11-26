use std::collections::{HashMap, HashSet};

use nf_project::{Id, NodeDocument, PinLink, PinValue, VariableDocument};

use crate::scope::{MACRO_INPUTS, MACRO_OUTPUTS};

const MACRO_PREFIX: &str = "macro/";
const VARIABLE_PREFIXES: [&str; 2] = ["variable/get/", "variable/set/"];
const MAXIMUM_NODES: usize = 50_000;

#[derive(Debug, Clone)]
pub struct MacroBody {
    pub graph: Id,
    pub name: String,
    pub nodes: Vec<NodeDocument>,
    pub variables: Vec<VariableDocument>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacroOrigin {
    pub instance: Id,
    pub graph: Id,
    pub node: Id,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacroProblem {
    pub node: Id,
    pub message: String,
    pub hint: String,
}

#[derive(Debug, Clone, Default)]
pub struct MacroExpansion {
    pub nodes: Vec<NodeDocument>,
    pub origins: HashMap<Id, MacroOrigin>,
    pub problems: Vec<MacroProblem>,
    pub variables: Vec<VariableDocument>,
}

pub fn macro_of(node: &NodeDocument) -> Option<Id> {
    node.node_type.strip_prefix(MACRO_PREFIX)?.parse().ok()
}

struct Instance<'a> {
    node: NodeDocument,
    body: &'a MacroBody,
    inputs: Option<&'a NodeDocument>,
    outputs: Option<&'a NodeDocument>,
}

impl Instance<'_> {
    fn copy_of(&self, original: Id) -> Id {
        Id::derived(self.node.id, original)
    }

    fn is_inputs(&self, id: Id) -> bool {
        self.inputs.is_some_and(|inputs| inputs.id == id)
    }

    fn is_outputs(&self, id: Id) -> bool {
        self.outputs.is_some_and(|outputs| outputs.id == id)
    }

    fn exec_target(&self, link: &PinLink) -> Option<PinLink> {
        if self.is_outputs(link.node) {
            return self.node.exec.get(&link.pin).cloned();
        }
        if self.is_inputs(link.node) {
            return None;
        }
        Some(PinLink {
            node: self.copy_of(link.node),
            pin: link.pin.clone(),
        })
    }

    fn data_source(&self, link: &PinLink) -> Option<PinValue> {
        if self.is_inputs(link.node) {
            return self.node.inputs.get(&link.pin).cloned();
        }
        if self.is_outputs(link.node) {
            return None;
        }
        Some(PinValue::Link(PinLink {
            node: self.copy_of(link.node),
            pin: link.pin.clone(),
        }))
    }

    fn entry_target(&self, pin: &str) -> Option<PinLink> {
        let link = self.inputs?.exec.get(pin)?;
        self.exec_target(link).filter(|target| target.node != self.node.id)
    }

    fn output_value(&self, pin: &str) -> Option<PinValue> {
        let value = match self.outputs?.inputs.get(pin)? {
            PinValue::Link(link) => self.data_source(link)?,
            literal => literal.clone(),
        };
        match &value {
            PinValue::Link(link) if link.node == self.node.id => None,
            _ => Some(value),
        }
    }

    fn variables(&self) -> Vec<VariableDocument> {
        self.body
            .variables
            .iter()
            .map(|variable| VariableDocument {
                id: self.copy_of(variable.id),
                ..variable.clone()
            })
            .collect()
    }

    fn renamed_variable_node(&self, node_type: &str) -> Option<String> {
        VARIABLE_PREFIXES.iter().find_map(|prefix| {
            let original: Id = node_type.strip_prefix(prefix)?.parse().ok()?;
            self.body
                .variables
                .iter()
                .any(|variable| variable.id == original)
                .then(|| format!("{prefix}{}", self.copy_of(original)))
        })
    }

    fn copies(&self) -> Vec<(Id, NodeDocument)> {
        self.body
            .nodes
            .iter()
            .filter(|node| !self.is_inputs(node.id) && !self.is_outputs(node.id))
            .map(|original| {
                let mut copy = original.clone();
                copy.id = self.copy_of(original.id);
                if let Some(node_type) = self.renamed_variable_node(&original.node_type) {
                    copy.node_type = node_type;
                }
                copy.inputs = original
                    .inputs
                    .iter()
                    .filter_map(|(pin, value)| {
                        let value = match value {
                            PinValue::Link(link) => self.data_source(link)?,
                            literal => literal.clone(),
                        };
                        Some((pin.clone(), value))
                    })
                    .collect();
                copy.exec = original
                    .exec
                    .iter()
                    .filter_map(|(pin, link)| Some((pin.clone(), self.exec_target(link)?)))
                    .collect();
                (original.id, copy)
            })
            .collect()
    }

    fn rewire(&self, node: &mut NodeDocument) {
        let instance = self.node.id;
        node.exec = std::mem::take(&mut node.exec)
            .into_iter()
            .filter_map(|(pin, link)| {
                if link.node != instance {
                    return Some((pin, link));
                }
                Some((pin, self.entry_target(&link.pin)?))
            })
            .collect();
        node.inputs = std::mem::take(&mut node.inputs)
            .into_iter()
            .filter_map(|(pin, value)| match value {
                PinValue::Link(link) if link.node == instance => Some((pin, self.output_value(&link.pin)?)),
                value => Some((pin, value)),
            })
            .collect();
    }
}

pub fn expand_macros(nodes: &[NodeDocument], macros: &HashMap<Id, MacroBody>) -> MacroExpansion {
    let mut list = nodes.to_vec();
    let mut chains: HashMap<Id, Vec<Id>> = HashMap::new();
    let mut origins: HashMap<Id, MacroOrigin> = HashMap::new();
    let mut problems = Vec::new();
    let mut kept: HashSet<Id> = HashSet::new();
    let mut variables = Vec::new();

    while let Some(index) = list
        .iter()
        .position(|node| !kept.contains(&node.id) && macro_of(node).is_some_and(|id| macros.contains_key(&id)))
    {
        let node = list.remove(index);
        let macro_id = macro_of(&node).unwrap_or(node.id);
        let body = &macros[&macro_id];
        let top_level = origins.get(&node.id).map_or(node.id, |origin| origin.instance);
        let chain = chains.get(&node.id).cloned().unwrap_or_default();
        if chain.contains(&macro_id) {
            problems.push(MacroProblem {
                node: top_level,
                message: format!("The macro {} uses itself, so it would never stop growing.", body.name),
                hint: "Remove the macro node from inside its own macro, or from the macros it uses.".to_owned(),
            });
            kept.insert(node.id);
            list.insert(index, node);
            continue;
        }
        if list.len() + body.nodes.len() > MAXIMUM_NODES {
            problems.push(MacroProblem {
                node: top_level,
                message: format!("Inserting the macro {} would create more than {MAXIMUM_NODES} nodes.", body.name),
                hint: "Use functions for large or deeply nested logic. A function exists once, a macro is copied into every place that uses it.".to_owned(),
            });
            kept.insert(node.id);
            list.insert(index, node);
            continue;
        }

        let instance = Instance {
            inputs: body.nodes.iter().find(|candidate| candidate.node_type == MACRO_INPUTS),
            outputs: body.nodes.iter().find(|candidate| candidate.node_type == MACRO_OUTPUTS),
            node,
            body,
        };
        variables.extend(instance.variables());
        let mut copies = instance.copies();
        let mut chain = chain;
        chain.push(macro_id);
        for (original, copy) in &copies {
            chains.insert(copy.id, chain.clone());
            origins.insert(
                copy.id,
                MacroOrigin {
                    instance: top_level,
                    graph: body.graph,
                    node: *original,
                },
            );
        }
        for other in list.iter_mut().chain(copies.iter_mut().map(|(_, copy)| copy)) {
            instance.rewire(other);
        }
        list.extend(copies.into_iter().map(|(_, copy)| copy));
    }

    MacroExpansion {
        nodes: list,
        origins,
        problems,
        variables,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(node_type: &str) -> NodeDocument {
        NodeDocument::new(node_type, [0.0, 0.0])
    }

    fn link(node: &NodeDocument, pin: &str) -> PinLink {
        PinLink {
            node: node.id,
            pin: pin.to_owned(),
        }
    }

    fn twice_macro() -> (Id, MacroBody) {
        let mut inputs = node(MACRO_INPUTS);
        let mut outputs = node(MACRO_OUTPUTS);
        let mut first = node("call/Console/Log");
        let mut second = node("call/Console/Log");
        let graph = Id::generate();
        inputs.exec.insert("run".to_owned(), link(&first, "exec"));
        first.inputs.insert("message".to_owned(), PinValue::Link(link(&inputs, "text")));
        first.exec.insert("then".to_owned(), link(&second, "exec"));
        second.inputs.insert("message".to_owned(), PinValue::Link(link(&inputs, "text")));
        second.exec.insert("then".to_owned(), link(&outputs, "done"));
        outputs.inputs.insert("copy".to_owned(), PinValue::Link(link(&inputs, "text")));
        (
            graph,
            MacroBody {
                graph,
                name: "Twice".to_owned(),
                nodes: vec![inputs, outputs, first, second],
                variables: Vec::new(),
            },
        )
    }

    #[test]
    fn inlines_macro_bodies_and_rewires_both_sides() {
        let (graph, body) = twice_macro();
        let macros = HashMap::from([(graph, body)]);
        let mut start = node("event/Server/Start");
        let mut instance = node(&format!("macro/{graph}"));
        let mut after = node("call/Console/Log");
        start.exec.insert("then".to_owned(), link(&instance, "run"));
        instance.inputs.insert("text".to_owned(), PinValue::Literal { literal: "hi".into() });
        instance.exec.insert("done".to_owned(), link(&after, "exec"));
        after.inputs.insert("message".to_owned(), PinValue::Link(link(&instance, "copy")));

        let expansion = expand_macros(&[start.clone(), instance.clone(), after.clone()], &macros);
        assert!(expansion.problems.is_empty());
        assert_eq!(expansion.nodes.len(), 4);
        let by_id: HashMap<Id, &NodeDocument> = expansion.nodes.iter().map(|node| (node.id, node)).collect();
        let first = by_id[&by_id[&start.id].exec["then"].node];
        assert_eq!(first.inputs["message"], PinValue::Literal { literal: "hi".into() });
        let second = by_id[&first.exec["then"].node];
        assert_eq!(second.exec["then"].node, after.id);
        assert_eq!(by_id[&after.id].inputs["message"], PinValue::Literal { literal: "hi".into() });
        assert_eq!(expansion.origins[&first.id].instance, instance.id);
        assert_eq!(expansion.origins[&first.id].graph, graph);
        assert_eq!(expand_macros(&[start, instance, after], &macros).nodes, expansion.nodes);
    }

    #[test]
    fn nested_macros_expand_and_recursion_is_reported() {
        let (inner_graph, inner) = twice_macro();
        let outer_graph = Id::generate();
        let mut inputs = node(MACRO_INPUTS);
        let outputs = node(MACRO_OUTPUTS);
        let mut nested = node(&format!("macro/{inner_graph}"));
        let mut again = node(&format!("macro/{outer_graph}"));
        inputs.exec.insert("run".to_owned(), link(&nested, "run"));
        nested.inputs.insert("text".to_owned(), PinValue::Link(link(&inputs, "text")));
        nested.exec.insert("done".to_owned(), link(&outputs, "done"));
        again.exec.insert("done".to_owned(), link(&outputs, "done"));
        let outer = MacroBody {
            graph: outer_graph,
            name: "Outer".to_owned(),
            nodes: vec![inputs, outputs, nested, again],
            variables: Vec::new(),
        };
        let macros = HashMap::from([(inner_graph, inner), (outer_graph, outer)]);
        let mut start = node("event/Server/Start");
        let instance = node(&format!("macro/{outer_graph}"));
        start.exec.insert("then".to_owned(), link(&instance, "run"));

        let expansion = expand_macros(&[start.clone(), instance.clone()], &macros);
        let logs = expansion.nodes.iter().filter(|node| node.node_type == "call/Console/Log").count();
        assert_eq!(logs, 2);
        assert_eq!(expansion.problems.len(), 1);
        assert_eq!(expansion.problems[0].node, instance.id);
        let log = expansion.nodes.iter().find(|node| node.node_type == "call/Console/Log").unwrap();
        assert_eq!(expansion.origins[&log.id].graph, inner_graph);
        assert_eq!(expansion.origins[&log.id].instance, instance.id);
    }
}
