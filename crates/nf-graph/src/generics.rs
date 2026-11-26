use std::collections::HashMap;

use nf_api::TypeExpr;
use nf_project::{Id, NodeDocument, PinValue};

use crate::catalog::NodeDefinition;
use crate::pin::PinDefinition;

pub type NodeMap<'a> = HashMap<Id, (&'a NodeDocument, Option<&'a NodeDefinition>)>;

pub const TYPE_PARAMETER: &str = "$T";

pub fn type_parameter() -> TypeExpr {
    TypeExpr::named(TYPE_PARAMETER)
}

pub fn contains_type_parameter(type_expr: &TypeExpr) -> bool {
    match type_expr {
        TypeExpr::Named(name) => name == TYPE_PARAMETER,
        TypeExpr::Array(inner) | TypeExpr::Optional(inner) | TypeExpr::Variadic(inner) => contains_type_parameter(inner),
        TypeExpr::Union(members) => members.iter().any(contains_type_parameter),
    }
}

pub fn substitute(type_expr: &TypeExpr, binding: Option<&TypeExpr>) -> TypeExpr {
    match type_expr {
        TypeExpr::Named(name) if name == TYPE_PARAMETER => binding.cloned().unwrap_or_else(|| TypeExpr::named("any")),
        TypeExpr::Named(_) => type_expr.clone(),
        TypeExpr::Array(inner) => TypeExpr::Array(Box::new(substitute(inner, binding))),
        TypeExpr::Optional(inner) => TypeExpr::Optional(Box::new(substitute(inner, binding))),
        TypeExpr::Variadic(inner) => TypeExpr::Variadic(Box::new(substitute(inner, binding))),
        TypeExpr::Union(members) => TypeExpr::Union(members.iter().map(|member| substitute(member, binding)).collect()),
    }
}

pub fn infer_binding(declared: &TypeExpr, actual: &TypeExpr) -> Option<TypeExpr> {
    let binding = match (declared, actual) {
        (TypeExpr::Named(name), _) if name == TYPE_PARAMETER => Some(actual.clone()),
        (TypeExpr::Array(declared), TypeExpr::Array(actual)) => infer_binding(declared, actual),
        (TypeExpr::Optional(declared), TypeExpr::Optional(actual)) => infer_binding(declared, actual),
        (TypeExpr::Optional(declared), _) => infer_binding(declared, actual),
        _ => None,
    }?;
    (binding != TypeExpr::named("any")).then_some(binding)
}

pub fn resolve_pin_type(nodes: &NodeMap, node: Id, pin: &PinDefinition) -> Option<TypeExpr> {
    resolve_at_depth(nodes, node, pin, 0)
}

fn resolve_at_depth(nodes: &NodeMap, node: Id, pin: &PinDefinition, depth: usize) -> Option<TypeExpr> {
    let declared = pin.pin_type.type_expr()?;
    if !contains_type_parameter(declared) {
        return Some(declared.clone());
    }
    Some(substitute(declared, binding(nodes, node, depth).as_ref()))
}

fn binding(nodes: &NodeMap, node_id: Id, depth: usize) -> Option<TypeExpr> {
    if depth > 32 {
        return None;
    }
    let (node, definition) = nodes.get(&node_id)?;
    let mut pins: Vec<&PinDefinition> = (*definition)?
        .inputs
        .iter()
        .filter(|pin| pin.pin_type.type_expr().is_some_and(contains_type_parameter))
        .collect();
    pins.sort_by_key(|pin| !matches!(pin.pin_type.type_expr(), Some(TypeExpr::Array(_))));
    for pin in pins {
        let Some(PinValue::Link(link)) = node.inputs.get(&pin.name) else {
            continue;
        };
        let Some((_, Some(source))) = nodes.get(&link.node) else {
            continue;
        };
        let Some(output) = source.output(&link.pin) else {
            continue;
        };
        let Some(actual) = resolve_at_depth(nodes, link.node, output, depth + 1) else {
            continue;
        };
        if let Some(binding) = infer_binding(pin.pin_type.type_expr()?, &actual) {
            return Some(binding);
        }
    }
    None
}
