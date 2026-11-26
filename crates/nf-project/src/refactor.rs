use std::path::Path;

use crate::graph::{GraphDocument, load_variables};
use crate::tree::scan_package_tree;
use crate::{GraphManifest, ProjectError};

pub fn rename_type_in_expression(expression: &str, old: &str, new: &str) -> String {
    let mut result = String::with_capacity(expression.len());
    let mut word = String::new();
    let flush = |word: &mut String, result: &mut String| {
        if word == old {
            result.push_str(new);
        } else {
            result.push_str(word);
        }
        word.clear();
    };
    for character in expression.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            word.push(character);
        } else {
            flush(&mut word, &mut result);
            result.push(character);
        }
    }
    flush(&mut word, &mut result);
    result
}

fn renamed_node_type(node_type: &str, old: &str, new: &str) -> Option<String> {
    ["cast/", "is-a/", "flow/switch-enum/"].iter().find_map(|prefix| {
        (node_type.strip_prefix(prefix) == Some(old)).then(|| format!("{prefix}{new}"))
    })
}

pub fn rename_type_references(package_directory: &Path, old: &str, new: &str) -> Result<usize, ProjectError> {
    if old == new {
        return Ok(0);
    }
    let mut changed_files = 0;
    for graph in scan_package_tree(package_directory)?.graphs {
        let mut manifest = GraphManifest::read(&graph.path)?;
        let mut manifest_changed = false;
        if manifest.parent.as_deref() == Some(old) {
            manifest.parent = Some(new.to_owned());
            manifest_changed = true;
        }
        for interface in manifest.interfaces.iter_mut().filter(|interface| *interface == old) {
            *interface = new.to_owned();
            manifest_changed = true;
        }
        for function in &mut manifest.functions {
            for pin in function.parameters.iter_mut().chain(function.returns.iter_mut()) {
                let renamed = rename_type_in_expression(&pin.type_name, old, new);
                if renamed != pin.type_name {
                    pin.type_name = renamed;
                    manifest_changed = true;
                }
            }
        }
        for pin in manifest.signature.parameters.iter_mut().chain(manifest.signature.returns.iter_mut()) {
            let renamed = rename_type_in_expression(&pin.type_name, old, new);
            if renamed != pin.type_name {
                pin.type_name = renamed;
                manifest_changed = true;
            }
        }
        if manifest_changed {
            manifest.write(&graph.path)?;
            changed_files += 1;
        }

        for mut variable in load_variables(&graph.path)? {
            let renamed = rename_type_in_expression(&variable.type_name, old, new);
            if renamed != variable.type_name {
                variable.type_name = renamed;
                variable.write(&graph.path)?;
                changed_files += 1;
            }
        }

        for mut node in GraphDocument::load(&graph.path)?.nodes {
            if let Some(renamed) = renamed_node_type(&node.node_type, old, new) {
                node.node_type = renamed;
                node.write(&graph.path)?;
                changed_files += 1;
            }
        }
    }
    Ok(changed_files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renames_whole_type_names_only() {
        assert_eq!(rename_type_in_expression("Zombie", "Zombie", "Walker"), "Walker");
        assert_eq!(rename_type_in_expression("Zombie[]", "Zombie", "Walker"), "Walker[]");
        assert_eq!(rename_type_in_expression("Zombie?|ZombieBoss", "Zombie", "Walker"), "Walker?|ZombieBoss");
        assert_eq!(rename_type_in_expression("MyZombie", "Zombie", "Walker"), "MyZombie");
    }
}

#[cfg(test)]
mod package_tests {
    use crate::{
        FunctionPin, FunctionSignature, GraphDocument, GraphKind, GraphManifest, Id, NodeDocument, PackageManifest, PackageType,
        ScriptContext, VariableDocument, apply_graph_changes, apply_variable_changes, context_root, create_class, create_graph,
        load_variables, rename_entry, update_function_signature,
    };

    #[test]
    fn renaming_a_class_updates_every_reference_in_the_package() {
        let temporary = tempfile::tempdir().unwrap();
        let package = temporary.path().join("game");
        PackageManifest {
            id: Id::generate(),
            package_type: PackageType::Script,
            title: "Game".to_owned(),
            author: "Tester".to_owned(),
            version: "1.0.0".to_owned(),
            compatibility_version: "1.144".to_owned(),
            auto_cleanup: false,
            load_level_entities: false,
            packages_requirements: Vec::new(),
            assets_requirements: Vec::new(),
            map_asset: None,
        }
        .write(&package)
        .unwrap();
        let shared = context_root(&package, ScriptContext::Shared);
        std::fs::create_dir_all(&shared).unwrap();
        let zombie = create_class(&shared, "Zombie", "Character").unwrap();
        let boss = create_class(&shared, "ZombieBoss", "Zombie").unwrap();
        let function = create_graph(&shared, "Bite All", GraphKind::Function).unwrap();
        update_function_signature(
            &function,
            FunctionSignature {
                parameters: vec![FunctionPin {
                    id: "victims".to_owned(),
                    name: "Victims".to_owned(),
                    type_name: "Zombie[]".to_owned(),
                }],
                ..FunctionSignature::default()
            },
        )
        .unwrap();
        let variable = VariableDocument {
            id: Id::generate(),
            name: "Target".to_owned(),
            type_name: "Zombie?".to_owned(),
            default: None,
            description: String::new(),
            sync: false,
        };
        apply_variable_changes(&function, std::slice::from_ref(&variable), &[]).unwrap();
        let cast = NodeDocument::new("cast/Zombie", [0.0, 0.0]);
        let unrelated = NodeDocument::new("cast/ZombieBoss", [0.0, 100.0]);
        apply_graph_changes(&function, &[cast.clone(), unrelated.clone()], &[]).unwrap();

        let renamed = rename_entry(&zombie, "Walker").unwrap();
        assert!(renamed.ends_with("Walker"));
        assert_eq!(GraphManifest::read(&boss).unwrap().parent.as_deref(), Some("Walker"));
        assert_eq!(GraphManifest::read(&function).unwrap().signature.parameters[0].type_name, "Walker[]");
        assert_eq!(load_variables(&function).unwrap()[0].type_name, "Walker?");
        let nodes = GraphDocument::load(&function).unwrap().nodes;
        let types: Vec<&str> = nodes.iter().map(|node| node.node_type.as_str()).collect();
        assert!(types.contains(&"cast/Walker") && types.contains(&"cast/ZombieBoss"), "{types:?}");
    }
}
