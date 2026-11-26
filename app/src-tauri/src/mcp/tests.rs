use nf_api::{ApiChannel, ApiDatabase};
use nf_graph::{GraphScope, NodeCatalog, ScopeData};
use nf_project::{NodeDocument, PinLink, PinValue};
use serde_json::json;

use super::describe;
use super::edit::{Operation, PinReference, apply_operations};

fn catalog() -> NodeCatalog {
    NodeCatalog::from_database(&ApiDatabase::bundled(ApiChannel::Stable).unwrap())
}

fn operations(value: serde_json::Value) -> Vec<Operation> {
    serde_json::from_value(value).unwrap()
}

fn pin(node: &str, pin: &str) -> PinReference {
    PinReference { node: node.to_owned(), pin: pin.to_owned() }
}

#[test]
fn adds_and_connects_nodes_by_reference() {
    let catalog = catalog();
    let scope = GraphScope::new(&catalog, ScopeData::default());
    let start = NodeDocument::new("flow/sequence", [0.0, 0.0]);
    let start_id = start.id;

    let outcome = apply_operations(
        &scope,
        vec![start],
        operations(json!([
            {"op": "add", "ref": "branch", "type": "flow/branch"},
            {"op": "add", "ref": "print", "type": "debug/print", "position": [700, 40]},
            {"op": "connect", "from": {"node": start_id.to_string(), "pin": "then_0"}, "to": {"node": "branch", "pin": "exec"}},
            {"op": "connect", "from": {"node": "branch", "pin": "true"}, "to": {"node": "print", "pin": "exec"}},
            {"op": "set_value", "node": "branch", "pin": "condition", "value": true},
            {"op": "set_value", "node": "print", "pin": "value", "value": "Hello from the agent"}
        ])),
    )
    .unwrap();

    let branch = outcome.created["branch"];
    let print = outcome.created["print"];
    let find = |id| outcome.upserts.iter().find(|node| node.id == id).unwrap();
    assert_eq!(find(start_id).exec["then_0"], PinLink { node: branch, pin: "exec".into() });
    assert_eq!(find(branch).exec["true"], PinLink { node: print, pin: "exec".into() });
    assert_eq!(find(branch).inputs["condition"], PinValue::Literal { literal: toml::Value::Boolean(true) });
    assert_eq!(find(print).position, [704.0, 48.0]);
    assert_eq!(find(branch).position, [368.0, 0.0]);
    assert!(outcome.deletions.is_empty());
}

#[test]
fn removing_a_node_drops_links_to_it() {
    let catalog = catalog();
    let scope = GraphScope::new(&catalog, ScopeData::default());
    let mut branch = NodeDocument::new("flow/branch", [0.0, 0.0]);
    let print = NodeDocument::new("debug/print", [400.0, 0.0]);
    branch.exec.insert("true".into(), PinLink { node: print.id, pin: "exec".into() });
    let branch_id = branch.id;
    let print_id = print.id;

    let outcome = apply_operations(&scope, vec![branch, print], vec![Operation::Remove { node: print_id.to_string() }]).unwrap();
    assert_eq!(outcome.deletions, vec![print_id]);
    let branch = outcome.upserts.iter().find(|node| node.id == branch_id).unwrap();
    assert!(branch.exec.is_empty());
}

#[test]
fn rejects_mismatched_pins_without_changing_anything() {
    let catalog = catalog();
    let scope = GraphScope::new(&catalog, ScopeData::default());
    let branch = NodeDocument::new("flow/branch", [0.0, 0.0]);
    let print = NodeDocument::new("debug/print", [400.0, 0.0]);
    let (branch_id, print_id) = (branch.id.to_string(), print.id.to_string());

    let exec_to_data = apply_operations(
        &scope,
        vec![branch.clone(), print.clone()],
        vec![Operation::Connect { from: pin(&branch_id, "true"), to: pin(&print_id, "value") }],
    )
    .unwrap_err();
    assert!(exec_to_data.contains("exec pin"), "{exec_to_data}");

    let unknown_pin = apply_operations(
        &scope,
        vec![branch, print],
        vec![Operation::Connect { from: pin(&branch_id, "maybe"), to: pin(&print_id, "exec") }],
    )
    .unwrap_err();
    assert!(unknown_pin.contains("true, false"), "{unknown_pin}");

    let unknown_type = apply_operations(&scope, Vec::new(), operations(json!([{"op": "add", "type": "does/not-exist"}]))).unwrap_err();
    assert!(unknown_type.contains("search_nodes"), "{unknown_type}");
}

#[test]
fn dynamic_nodes_start_with_their_initial_pins() {
    let catalog = catalog();
    let scope = GraphScope::new(&catalog, ScopeData::default());
    let outcome = apply_operations(&scope, Vec::new(), operations(json!([{"op": "add", "ref": "sequence", "type": "flow/sequence"}]))).unwrap();
    let sequence = &outcome.upserts[0];
    assert_eq!(sequence.properties["count"], toml::Value::Integer(2));
}

#[test]
fn searches_and_describes_nodes() {
    let catalog = catalog();
    let scope = GraphScope::new(&catalog, ScopeData::default());
    let found = describe::search(&scope, Some(nf_project::ScriptContext::Server), "print", 5);
    assert_eq!(found["results"][0]["type"], "debug/print");
    let described = describe::node_type_view(scope.get("flow/branch").unwrap());
    assert_eq!(described["outputs"][0]["pin"], "true");
}

#[test]
fn permissions_hide_their_tools() {
    let mut permissions = super::McpPermissions::default();
    assert!(permissions.blocked_tools().is_empty());
    permissions.test_server = false;
    permissions.edit_graphs = false;
    assert_eq!(permissions.blocked_tools(), ["edit_graph", "play_test_server", "stop_test_server", "read_server_log"]);
}
