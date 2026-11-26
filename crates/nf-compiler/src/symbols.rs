use std::collections::HashMap;
use std::path::PathBuf;

use nf_graph::DiagnosticSeverity;
use nf_project::{EventDirection, FunctionSignature, GraphSummary, Id, ScriptContext, VariableDocument};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct FunctionSymbol {
    pub lua_name: String,
    pub owner: Option<Id>,
    pub context: ScriptContext,
    pub signature: FunctionSignature,
}

#[derive(Debug, Clone)]
pub struct InterfaceSymbol {
    pub name: String,
    pub functions: Vec<(String, String, usize)>,
}

#[derive(Debug, Clone)]
pub struct EnumSymbol {
    pub name: String,
    pub values: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct LibrarySymbol {
    pub namespace: String,
    pub lua_name: String,
}

#[derive(Debug, Clone)]
pub struct EventSymbol {
    pub lua_name: String,
    pub direction: EventDirection,
}

#[derive(Debug, Clone)]
pub struct StructField {
    pub id: String,
    pub key: String,
    pub type_name: String,
}

#[derive(Debug, Clone)]
pub struct StructSymbol {
    pub name: String,
    pub fields: Vec<StructField>,
}

#[derive(Debug, Clone)]
pub struct ClassSymbol {
    pub id: Id,
    pub name: String,
    pub parent: String,
    pub parent_class: Option<Id>,
    pub context: ScriptContext,
    pub api_root: Option<String>,
    pub interfaces: Vec<String>,
}

impl ClassSymbol {
    pub fn is_available_in(&self, context: ScriptContext) -> bool {
        self.context == ScriptContext::Shared || self.context == context
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableOwner {
    Graph(Id),
    Class(Id),
}

#[derive(Debug, Clone)]
pub struct VariableSymbol {
    pub owner: VariableOwner,
    pub document: VariableDocument,
}

#[derive(Debug, Clone, Default)]
pub struct PackageSymbols {
    pub functions: HashMap<Id, FunctionSymbol>,
    pub classes: HashMap<Id, ClassSymbol>,
    pub variables: HashMap<Id, VariableSymbol>,
    pub events: HashMap<Id, EventSymbol>,
    pub structs: HashMap<Id, StructSymbol>,
    pub libraries: HashMap<Id, LibrarySymbol>,
    pub enums: HashMap<Id, EnumSymbol>,
    pub interfaces: HashMap<Id, InterfaceSymbol>,
}

impl PackageSymbols {
    pub fn struct_named(&self, name: &str) -> Option<&StructSymbol> {
        self.structs.values().find(|structure| structure.name == name)
    }

    pub fn enum_named(&self, name: &str) -> Option<&EnumSymbol> {
        self.enums.values().find(|enumeration| enumeration.name == name)
    }

    pub fn class_named(&self, name: &str) -> Option<&ClassSymbol> {
        self.classes.values().find(|class| class.name == name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphReference {
    pub id: Id,
    pub name: String,
    pub path: PathBuf,
    pub location: String,
}

impl GraphReference {
    pub fn of(graph: &GraphSummary, owner: Option<&str>) -> GraphReference {
        let mut parts = vec![graph.context.display_name().to_owned()];
        parts.extend(graph.folders.iter().cloned());
        if let Some(owner) = owner {
            parts.push(owner.to_owned());
        }
        parts.push(graph.name.clone());
        GraphReference {
            id: graph.id,
            name: graph.name.clone(),
            path: graph.path.clone(),
            location: parts.join(" / "),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildProblem {
    pub severity: DiagnosticSeverity,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<GraphReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<Id>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub severity: DiagnosticSeverity,
    pub code: &'static str,
    pub node: Option<Id>,
    pub message: String,
    pub hint: Option<String>,
}

impl Issue {
    pub fn error(code: &'static str, node: Option<Id>, message: impl Into<String>, hint: impl Into<String>) -> Issue {
        Issue {
            severity: DiagnosticSeverity::Error,
            code,
            node,
            message: message.into(),
            hint: Some(hint.into()),
        }
    }

    pub fn warning(code: &'static str, node: Option<Id>, message: impl Into<String>, hint: impl Into<String>) -> Issue {
        Issue {
            severity: DiagnosticSeverity::Warning,
            ..Issue::error(code, node, message, hint)
        }
    }

    pub fn into_problem(self, graph: Option<GraphReference>) -> BuildProblem {
        BuildProblem {
            severity: self.severity,
            code: self.code.to_owned(),
            graph,
            node: self.node,
            pin: None,
            message: self.message,
            hint: self.hint,
        }
    }
}
