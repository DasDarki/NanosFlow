use std::collections::HashMap;
use std::path::Path;

use nf_api::TypeExpr;
use nf_project::{
    EventDirection, FieldIssue, FunctionPin, FunctionSignature, GraphKind, GraphManifest, GraphSummary, Id, InterfaceFunction, PackageManifest, ProjectError,
    ScriptContext, VariableDocument, load_variables, scan_package_tree,
};
use serde::Serialize;

use crate::catalog::{ApiBinding, BindingKind, CallbackDefinition, ContextWarning, NodeCatalog, NodeDefinition, NodeKind, Side, cast_definitions};
use crate::pin::{PinDefinition, PinType};
use crate::text::humanize;
use crate::types::TypeSystem;

pub const FUNCTION_ENTRY: &str = "function/entry";
pub const FUNCTION_RETURN: &str = "function/return";
pub const CLASS_SELF: &str = "class/self";
pub const MACRO_INPUTS: &str = "macro/inputs";
pub const MACRO_OUTPUTS: &str = "macro/outputs";
pub const EXEC_TYPE: &str = "exec";
const MAX_NAME_LENGTH: usize = 48;
const SELF_PIN: &str = "self";

pub fn call_node_id(graph_id: Id) -> String {
    format!("function/{graph_id}")
}

pub fn macro_node_id(graph_id: Id) -> String {
    format!("macro/{graph_id}")
}

pub fn library_node_id(graph_id: Id) -> String {
    format!("library/{graph_id}")
}

pub fn method_node_id(graph_id: Id) -> String {
    format!("method/{graph_id}")
}

pub fn spawn_node_id(class_id: Id) -> String {
    format!("class/spawn/{class_id}")
}

pub fn class_event_node_id(event: &str) -> String {
    format!("class-event/{event}")
}

pub fn event_handler_id(event: Id) -> String {
    format!("custom-event/{event}")
}

pub fn event_send_id(event: Id) -> String {
    format!("send-event/{event}")
}

pub fn event_send_to_player_id(event: Id) -> String {
    format!("send-event/{event}/player")
}

pub fn variable_get_id(variable: Id) -> String {
    format!("variable/get/{variable}")
}

pub fn variable_set_id(variable: Id) -> String {
    format!("variable/set/{variable}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectClass {
    pub id: Id,
    pub name: String,
    pub parent: String,
    pub parents: Vec<String>,
    pub api_root: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStruct {
    pub id: Id,
    pub name: String,
    pub fields: Vec<FunctionPin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEnum {
    pub id: Id,
    pub name: String,
    pub values: Vec<FunctionPin>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScopeData {
    pub definitions: Vec<NodeDefinition>,
    pub classes: Vec<ProjectClass>,
    pub structs: Vec<ProjectStruct>,
    pub enums: Vec<ProjectEnum>,
    pub interfaces: Vec<String>,
}

pub struct GraphScope<'a> {
    catalog: &'a NodeCatalog,
    extras: HashMap<String, NodeDefinition>,
    types: TypeSystem,
    enums: Vec<ProjectEnum>,
}

impl<'a> GraphScope<'a> {
    pub fn new(catalog: &'a NodeCatalog, scope: ScopeData) -> GraphScope<'a> {
        let classes: Vec<(String, Vec<String>)> = scope
            .classes
            .iter()
            .map(|class| (class.name.clone(), class.parents.clone()))
            .collect();
        GraphScope {
            catalog,
            types: catalog
                .types()
                .with_project_classes(&classes)
                .with_project_structs(scope.structs.iter().map(|structure| structure.name.clone()))
                .with_project_enums(scope.enums.iter().map(|enumeration| enumeration.name.clone())),
            enums: scope.enums,
            extras: scope
                .definitions
                .into_iter()
                .map(|definition| (definition.id.clone(), definition))
                .collect(),
        }
    }

    pub fn get(&self, id: &str) -> Option<&NodeDefinition> {
        self.extras.get(id).or_else(|| self.catalog.get(id))
    }

    pub fn definitions(&self) -> impl Iterator<Item = &NodeDefinition> {
        self.extras
            .values()
            .chain(self.catalog.definitions().iter().filter(|definition| !self.extras.contains_key(&definition.id)))
    }

    pub fn catalog(&self) -> &NodeCatalog {
        self.catalog
    }

    pub fn types(&self) -> &TypeSystem {
        &self.types
    }

    pub fn project_enum(&self, name: &str) -> Option<&ProjectEnum> {
        self.enums.iter().find(|enumeration| enumeration.name == name)
    }

    pub fn enum_has_key(&self, name: &str, key: &str) -> bool {
        match self.project_enum(name) {
            Some(enumeration) => enumeration.values.iter().any(|value| value.id == key),
            None => self.catalog.enum_has_key(name, key),
        }
    }
}

fn side_of(context: ScriptContext) -> Side {
    match context {
        ScriptContext::Server => Side::Server,
        ScriptContext::Client => Side::Client,
        ScriptContext::Shared => Side::Both,
    }
}

fn parse_type(type_name: &str) -> TypeExpr {
    TypeExpr::parse(type_name).unwrap_or_else(|_| TypeExpr::named("any"))
}

fn pin(function_pin: &FunctionPin) -> PinDefinition {
    PinDefinition::data(&function_pin.id, &function_pin.name, parse_type(&function_pin.type_name))
}

fn location_label(graph: &GraphSummary) -> String {
    let mut parts = vec![graph.context.display_name().to_owned()];
    parts.extend(graph.folders.iter().cloned());
    parts.join(" / ")
}

fn target_pin(class_name: &str, inside_class: bool) -> PinDefinition {
    let mut target = PinDefinition::data(SELF_PIN, "Target", TypeExpr::named(class_name)).with_description(if inside_class {
        format!("The {class_name} to use. Leave it empty to use self.")
    } else {
        format!("The {class_name} to use.")
    });
    target.optional = inside_class;
    target
}

pub fn call_definition(graph: &GraphSummary, signature: &FunctionSignature) -> NodeDefinition {
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    if !signature.pure {
        inputs.push(PinDefinition::exec("exec", ""));
        outputs.push(PinDefinition::exec("then", ""));
    }
    inputs.extend(signature.parameters.iter().map(pin));
    outputs.extend(signature.returns.iter().map(pin));
    NodeDefinition {
        id: call_node_id(graph.id),
        title: graph.name.clone(),
        subtitle: Some(format!("Function in {}", location_label(graph))),
        category: "Functions".to_owned(),
        kind: if signature.pure { NodeKind::PureFunction } else { NodeKind::Function },
        side: side_of(graph.context),
        description: signature.description.clone(),
        inputs,
        outputs,
        keywords: vec!["function".to_owned(), "call".to_owned()],
        api: None,
        context_warnings: Vec::new(),
        callbacks: Vec::new(),
        dynamic: None,
    }
}

fn method_definition(method: &GraphSummary, signature: &FunctionSignature, class_name: &str, inside_class: bool) -> NodeDefinition {
    let mut definition = call_definition(method, signature);
    definition.id = method_node_id(method.id);
    definition.subtitle = Some(format!("Target is {class_name}"));
    definition.category = class_name.to_owned();
    let position = usize::from(!signature.pure);
    definition.inputs.insert(position, target_pin(class_name, inside_class));
    definition.keywords.push(class_name.to_owned());
    definition.keywords.push("method".to_owned());
    definition
}

pub fn entry_definition(graph: &GraphSummary, signature: &FunctionSignature, owner: Option<&str>) -> NodeDefinition {
    let mut outputs = Vec::new();
    if !signature.pure {
        outputs.push(PinDefinition::exec("then", ""));
    }
    if let Some(class_name) = owner {
        outputs.push(PinDefinition::data(SELF_PIN, "Self", TypeExpr::named(class_name)));
    }
    outputs.extend(signature.parameters.iter().map(pin));
    NodeDefinition {
        id: FUNCTION_ENTRY.to_owned(),
        title: graph.name.clone(),
        subtitle: Some(match owner {
            Some(class_name) => format!("Method entry of {class_name}"),
            None => "Function entry".to_owned(),
        }),
        category: "Function".to_owned(),
        kind: NodeKind::FunctionEntry,
        side: Side::Both,
        description: "Where the function starts. Its outputs are the parameters the caller passes in.".to_owned(),
        inputs: Vec::new(),
        outputs,
        keywords: vec!["entry".to_owned(), "start".to_owned(), "parameters".to_owned()],
        api: None,
        context_warnings: Vec::new(),
        callbacks: Vec::new(),
        dynamic: None,
    }
}

fn macro_pin(function_pin: &FunctionPin) -> PinDefinition {
    if function_pin.type_name == EXEC_TYPE {
        PinDefinition::exec(&function_pin.id, &function_pin.name)
    } else {
        pin(function_pin)
    }
}

pub fn macro_definition(graph: &GraphSummary, signature: &FunctionSignature) -> NodeDefinition {
    let has_exec = signature
        .parameters
        .iter()
        .chain(&signature.returns)
        .any(|function_pin| function_pin.type_name == EXEC_TYPE);
    NodeDefinition {
        id: macro_node_id(graph.id),
        title: graph.name.clone(),
        subtitle: Some(format!("Macro in {}", location_label(graph))),
        category: "Macros".to_owned(),
        kind: if has_exec { NodeKind::Function } else { NodeKind::PureFunction },
        side: side_of(graph.context),
        description: signature.description.clone(),
        inputs: signature.parameters.iter().map(macro_pin).collect(),
        outputs: signature.returns.iter().map(macro_pin).collect(),
        keywords: vec!["macro".to_owned()],
        api: None,
        context_warnings: Vec::new(),
        callbacks: Vec::new(),
        dynamic: None,
    }
}

pub fn macro_inputs_definition(graph: &GraphSummary, signature: &FunctionSignature) -> NodeDefinition {
    NodeDefinition {
        id: MACRO_INPUTS.to_owned(),
        title: "Inputs".to_owned(),
        subtitle: Some(format!("Macro {}", graph.name)),
        category: "Macro".to_owned(),
        kind: NodeKind::FunctionEntry,
        side: Side::Both,
        description: "The pins of the macro node. Execution and values that reach the macro node come out here.".to_owned(),
        inputs: Vec::new(),
        outputs: signature.parameters.iter().map(macro_pin).collect(),
        keywords: vec!["inputs".to_owned(), "entry".to_owned(), "parameters".to_owned()],
        api: None,
        context_warnings: Vec::new(),
        callbacks: Vec::new(),
        dynamic: None,
    }
}

pub fn macro_outputs_definition(graph: &GraphSummary, signature: &FunctionSignature) -> NodeDefinition {
    NodeDefinition {
        id: MACRO_OUTPUTS.to_owned(),
        title: "Outputs".to_owned(),
        subtitle: Some(format!("Macro {}", graph.name)),
        category: "Macro".to_owned(),
        kind: NodeKind::FunctionReturn,
        side: Side::Both,
        description: "The output pins of the macro node. Execution that arrives here continues at the matching pin of the macro node.".to_owned(),
        inputs: signature
            .returns
            .iter()
            .map(|function_pin| {
                let mut definition = macro_pin(function_pin);
                definition.optional = true;
                definition
            })
            .collect(),
        outputs: Vec::new(),
        keywords: vec!["outputs".to_owned(), "return".to_owned(), "result".to_owned()],
        api: None,
        context_warnings: Vec::new(),
        callbacks: Vec::new(),
        dynamic: None,
    }
}

#[derive(Debug, Clone)]
pub struct LibraryFunction {
    pub graph: GraphSummary,
    pub signature: FunctionSignature,
}

#[derive(Debug, Clone)]
pub struct LibraryClass {
    pub graph: GraphSummary,
    pub parent: String,
    pub description: String,
    pub methods: Vec<LibraryFunction>,
    pub variables: Vec<VariableDocument>,
}

#[derive(Debug, Clone)]
pub struct LibraryPackage {
    pub folder: String,
    pub title: String,
    pub path: std::path::PathBuf,
    pub functions: Vec<LibraryFunction>,
    pub classes: Vec<LibraryClass>,
}

pub fn exported_classes(package_directory: &Path) -> Result<Vec<LibraryClass>, ProjectError> {
    let graphs = scan_package_tree(package_directory)?.graphs;
    let mut classes = Vec::new();
    for graph in graphs.iter().filter(|graph| graph.kind == GraphKind::Class) {
        let manifest = GraphManifest::read(&graph.path)?;
        if !manifest.signature.exported {
            continue;
        }
        let mut methods = Vec::new();
        for method in graphs.iter().filter(|method| method.owner == Some(graph.id) && method.kind == GraphKind::Function) {
            methods.push(LibraryFunction {
                graph: method.clone(),
                signature: GraphManifest::read(&method.path)?.signature,
            });
        }
        classes.push(LibraryClass {
            graph: graph.clone(),
            parent: manifest.parent.unwrap_or_default(),
            description: manifest.signature.description,
            methods,
            variables: load_variables(&graph.path)?,
        });
    }
    Ok(classes)
}

fn library_classes(package_directory: &Path) -> Result<Vec<LibraryClass>, ProjectError> {
    if !package_directory.join(PackageManifest::FILE_NAME).is_file() {
        return Ok(Vec::new());
    }
    Ok(required_libraries(package_directory)?.into_iter().flat_map(|library| library.classes).collect())
}

fn class_entries(tree_graphs: &[GraphSummary], libraries: &[LibraryClass]) -> Vec<(Id, String, String)> {
    tree_graphs
        .iter()
        .filter(|graph| graph.kind == GraphKind::Class)
        .map(|graph| (graph.id, graph.name.clone(), graph.parent.clone().unwrap_or_default()))
        .chain(libraries.iter().map(|class| (class.graph.id, class.graph.name.clone(), class.parent.clone())))
        .collect()
}

pub fn exported_functions(package_directory: &Path) -> Result<Vec<LibraryFunction>, ProjectError> {
    let mut functions = Vec::new();
    for graph in scan_package_tree(package_directory)?.graphs {
        if graph.kind != GraphKind::Function || graph.owner.is_some() {
            continue;
        }
        let signature = GraphManifest::read(&graph.path)?.signature;
        if signature.exported {
            functions.push(LibraryFunction { graph, signature });
        }
    }
    Ok(functions)
}

pub fn required_libraries(package_directory: &Path) -> Result<Vec<LibraryPackage>, ProjectError> {
    let manifest = PackageManifest::read(package_directory)?;
    let Some(packages_directory) = package_directory.parent() else {
        return Ok(Vec::new());
    };
    let mut libraries = Vec::new();
    for folder in &manifest.packages_requirements {
        let path = packages_directory.join(folder);
        if path == package_directory || !path.join(PackageManifest::FILE_NAME).is_file() {
            continue;
        }
        let library = PackageManifest::read(&path)?;
        libraries.push(LibraryPackage {
            folder: folder.clone(),
            title: library.title,
            functions: exported_functions(&path)?,
            classes: exported_classes(&path)?,
            path,
        });
    }
    Ok(libraries)
}

pub fn library_definition(library: &LibraryPackage, function: &LibraryFunction) -> NodeDefinition {
    let mut definition = call_definition(&function.graph, &function.signature);
    definition.id = library_node_id(function.graph.id);
    definition.subtitle = Some(format!("From the package {}", library.title));
    definition.category = format!("Libraries / {}", library.title);
    definition.keywords.push("library".to_owned());
    definition.keywords.push(library.folder.clone());
    definition
}

pub const DISPATCHER_EVENT: &str = "event";
pub const DISPATCHER_BINDING: &str = "binding";

pub fn dispatcher_member(action: &str, dispatcher_name: &str) -> String {
    let words: String = dispatcher_name
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut characters = word.chars();
            characters.next().map(|first| first.to_ascii_uppercase().to_string() + characters.as_str()).unwrap_or_default()
        })
        .collect();
    format!("{action}{words}")
}

pub fn dispatcher_definitions(dispatcher: &GraphSummary, signature: &FunctionSignature, class_name: &str, inside_class: bool) -> Vec<NodeDefinition> {
    let name = &dispatcher.name;
    let node = |action: &str, title: String, description: String, inputs: Vec<PinDefinition>, outputs: Vec<PinDefinition>, callbacks: Vec<CallbackDefinition>| {
        let member = match action {
            "unbind-all" => dispatcher_member("UnbindAll", name),
            other => dispatcher_member(&humanize(other), name),
        };
        let mut all_inputs = vec![PinDefinition::exec("exec", ""), target_pin(class_name, inside_class)];
        all_inputs.extend(inputs);
        let mut all_outputs = vec![PinDefinition::exec("then", "")];
        all_outputs.extend(outputs);
        NodeDefinition {
            id: format!("dispatcher/{action}/{}", dispatcher.id),
            title,
            subtitle: Some(format!("Event dispatcher of {class_name}")),
            category: class_name.to_owned(),
            kind: NodeKind::Function,
            side: Side::Both,
            description,
            inputs: all_inputs,
            outputs: all_outputs,
            keywords: vec!["dispatcher".to_owned(), "delegate".to_owned(), name.clone(), class_name.to_owned()],
            api: Some(ApiBinding {
                kind: BindingKind::Method,
                class: class_name.to_owned(),
                member,
            }),
            context_warnings: Vec::new(),
            callbacks,
            dynamic: None,
        }
    };
    let binding_pin = || PinDefinition::data(DISPATCHER_BINDING, "Binding", TypeExpr::named("function"));
    let event_arguments: Vec<String> = signature.parameters.iter().map(|parameter| format!("{DISPATCHER_EVENT}_{}", parameter.id)).collect();
    let mut bind_outputs = vec![
        PinDefinition::exec(DISPATCHER_EVENT, name).with_description(format!("Runs every time {name} is called on this {class_name}.")),
    ];
    bind_outputs.extend(signature.parameters.iter().map(|parameter| {
        PinDefinition::data(&format!("{DISPATCHER_EVENT}_{}", parameter.id), &parameter.name, parse_type(&parameter.type_name))
    }));
    bind_outputs.push(binding_pin().with_description("Pass it to Unbind to stop listening."));
    let mut unbind_binding = binding_pin();
    unbind_binding.description = "The Binding output of the Bind node to remove.".to_owned();
    vec![
        node(
            "call",
            format!("Call {name}"),
            format!("Runs everything that is bound to {name} on the target, with these values."),
            signature.parameters.iter().map(pin).collect(),
            Vec::new(),
            Vec::new(),
        ),
        node(
            "bind",
            format!("Bind to {name}"),
            format!("From now on the {name} output runs whenever {name} is called on the target."),
            Vec::new(),
            bind_outputs,
            vec![CallbackDefinition {
                parameter: DISPATCHER_EVENT.to_owned(),
                position: 0,
                exec: DISPATCHER_EVENT.to_owned(),
                arguments: event_arguments,
                variadic: false,
            }],
        ),
        node(
            "unbind",
            format!("Unbind from {name}"),
            "Stops one binding from running again.".to_owned(),
            vec![unbind_binding],
            Vec::new(),
            Vec::new(),
        ),
        node(
            "unbind-all",
            format!("Unbind All from {name}"),
            format!("Removes every binding of {name} on the target."),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
    ]
}

pub fn interface_flag(interface_name: &str) -> String {
    format!("Implements{interface_name}")
}

pub fn interface_definitions(interface: &GraphSummary, functions: &[InterfaceFunction]) -> Vec<NodeDefinition> {
    let name = &interface.name;
    let mut definitions: Vec<NodeDefinition> = functions
        .iter()
        .map(|function| {
            let mut inputs = vec![
                PinDefinition::exec("exec", ""),
                PinDefinition::data(SELF_PIN, "Target", TypeExpr::named("any"))
                    .with_description(format!("Anything. If it does not implement {name}, nothing happens and the outputs are nil.")),
            ];
            inputs.extend(function.parameters.iter().map(pin));
            let mut outputs = vec![PinDefinition::exec("then", "")];
            outputs.extend(function.returns.iter().map(pin));
            NodeDefinition {
                id: format!("interface/call/{}/{}", interface.id, function.id),
                title: function.name.clone(),
                subtitle: Some(format!("Interface {name}")),
                category: format!("Interfaces / {name}"),
                kind: NodeKind::Function,
                side: side_of(interface.context),
                description: function.description.clone(),
                inputs,
                outputs,
                keywords: vec!["interface".to_owned(), "message".to_owned(), name.clone()],
                api: None,
                context_warnings: Vec::new(),
                callbacks: Vec::new(),
                dynamic: None,
            }
        })
        .collect();
    definitions.push(NodeDefinition {
        id: format!("interface/implements/{}", interface.id),
        title: format!("Does Implement {name}"),
        subtitle: Some(format!("Interface {name}")),
        category: format!("Interfaces / {name}"),
        kind: NodeKind::PureFunction,
        side: Side::Both,
        description: format!("True when the value is an instance of a class that implements {name}."),
        inputs: vec![PinDefinition::data(SELF_PIN, "Target", TypeExpr::named("any"))],
        outputs: vec![PinDefinition::data("result", "Result", TypeExpr::named("boolean"))],
        keywords: vec!["interface".to_owned(), "implements".to_owned(), "is".to_owned(), name.clone()],
        api: None,
        context_warnings: Vec::new(),
        callbacks: Vec::new(),
        dynamic: None,
    });
    definitions
}

pub fn validate_interface(functions: &[InterfaceFunction], types: &TypeSystem) -> Vec<FieldIssue> {
    let mut issues = Vec::new();
    for (index, function) in functions.iter().enumerate() {
        let previous: Vec<&str> = functions[..index].iter().map(|other| other.name.as_str()).collect();
        check_name(&function.name, "function", &format!("functions.{index}"), &previous, &mut issues);
        validate_pins(&function.parameters, &format!("functions.{index}.parameters"), "parameter", false, types, &mut issues);
        validate_pins(&function.returns, &format!("functions.{index}.returns"), "return value", false, types, &mut issues);
    }
    issues
}

pub fn enum_switch_definition(name: &str, values: &[FunctionPin]) -> NodeDefinition {
    let mut outputs: Vec<PinDefinition> = values
        .iter()
        .map(|value| PinDefinition::exec(&format!("{}{}", crate::CASE_PREFIX, value.id), &value.name))
        .collect();
    outputs.push(PinDefinition::exec("default", "Default"));
    NodeDefinition {
        id: format!("flow/switch-enum/{name}"),
        title: format!("Switch on {name}"),
        subtitle: Some("Your enum".to_owned()),
        category: "Switch".to_owned(),
        kind: NodeKind::FlowControl,
        side: Side::Both,
        description: format!("Continues with the output that matches the {name} value, or with Default."),
        inputs: vec![PinDefinition::exec("exec", ""), PinDefinition::data("value", "Value", TypeExpr::named(name))],
        outputs,
        keywords: vec!["switch".to_owned(), "case".to_owned(), "enum".to_owned(), name.to_owned()],
        api: None,
        context_warnings: Vec::new(),
        callbacks: Vec::new(),
        dynamic: None,
    }
}

pub fn return_definition(signature: &FunctionSignature) -> NodeDefinition {
    let mut inputs = Vec::new();
    if !signature.pure {
        inputs.push(PinDefinition::exec("exec", ""));
    }
    inputs.extend(signature.returns.iter().map(|value| {
        let mut definition = pin(value);
        definition.optional = true;
        definition
    }));
    NodeDefinition {
        id: FUNCTION_RETURN.to_owned(),
        title: "Return".to_owned(),
        subtitle: Some("Ends the function".to_owned()),
        category: "Function".to_owned(),
        kind: NodeKind::FunctionReturn,
        side: Side::Both,
        description: "Ends the function and hands the connected values back to the caller. Unconnected return values are nil.".to_owned(),
        inputs,
        outputs: Vec::new(),
        keywords: vec!["return".to_owned(), "result".to_owned(), "output".to_owned()],
        api: None,
        context_warnings: Vec::new(),
        callbacks: Vec::new(),
        dynamic: None,
    }
}

fn event_node(id: String, title: String, subtitle: &str, side: Side, kind: NodeKind, description: &str) -> NodeDefinition {
    NodeDefinition {
        id,
        title,
        subtitle: Some(subtitle.to_owned()),
        category: "Network Events".to_owned(),
        kind,
        side,
        description: description.to_owned(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        keywords: vec!["event".to_owned(), "remote".to_owned(), "network".to_owned(), "send".to_owned()],
        api: None,
        context_warnings: Vec::new(),
        callbacks: Vec::new(),
        dynamic: None,
    }
}

fn player_pin(description: &str) -> PinDefinition {
    PinDefinition::data("player", "Player", TypeExpr::named("Player")).with_description(description)
}

pub fn struct_make_id(structure: Id) -> String {
    format!("struct/make/{structure}")
}

pub fn struct_break_id(structure: Id) -> String {
    format!("struct/break/{structure}")
}

pub fn struct_set_id(structure: Id) -> String {
    format!("struct/set/{structure}")
}

pub fn struct_definitions(graph: &GraphSummary, description: &str, fields: &[FunctionPin]) -> Vec<NodeDefinition> {
    let name = &graph.name;
    let value = || PinDefinition::data("value", name, TypeExpr::named(name.as_str()));
    let optional_fields = || {
        fields.iter().map(|field| {
            let mut definition = pin(field);
            definition.optional = true;
            definition
        })
    };
    let node = |id: String, title: String, kind: NodeKind, text: String| NodeDefinition {
        id,
        title,
        subtitle: Some(format!("Struct {name}")),
        category: name.clone(),
        kind,
        side: Side::Both,
        description: text,
        inputs: Vec::new(),
        outputs: Vec::new(),
        keywords: vec![name.clone(), "struct".to_owned()],
        api: None,
        context_warnings: Vec::new(),
        callbacks: Vec::new(),
        dynamic: None,
    };
    let about = |action: &str| {
        if description.is_empty() {
            action.to_owned()
        } else {
            format!("{action} {description}")
        }
    };

    let mut make = node(struct_make_id(graph.id), format!("Make {name}"), NodeKind::PureFunction, about("Creates a new value from its fields. Unset fields start empty."));
    make.inputs.extend(optional_fields());
    make.outputs.push(value());
    make.keywords.extend(["make".to_owned(), "create".to_owned(), "new".to_owned()]);

    let mut split = node(struct_break_id(graph.id), format!("Break {name}"), NodeKind::PureFunction, about("Splits a value into its fields."));
    split.inputs.push(value());
    split.outputs.extend(fields.iter().map(pin));
    split.keywords.extend(["break".to_owned(), "split".to_owned(), "get".to_owned()]);

    let mut set = node(struct_set_id(graph.id), format!("Set Fields of {name}"), NodeKind::Function, about("Changes the connected fields of a value. Other fields keep their value."));
    set.inputs.push(PinDefinition::exec("exec", ""));
    set.inputs.push(value());
    set.inputs.extend(optional_fields());
    set.outputs.push(PinDefinition::exec("then", ""));
    set.outputs.push(value());
    set.keywords.extend(["set".to_owned(), "change".to_owned(), "members".to_owned()]);
    vec![make, split, set]
}

pub fn event_definitions(graph: &GraphSummary, direction: EventDirection, signature: &FunctionSignature) -> Vec<NodeDefinition> {
    let parameters: Vec<PinDefinition> = signature.parameters.iter().map(pin).collect();
    let name = &graph.name;
    let description = signature.description.as_str();
    let (handler_side, handler_subtitle) = match direction {
        EventDirection::ToServer => (Side::Server, "Sent by a client"),
        EventDirection::ToClients => (Side::Client, "Sent by the server"),
        EventDirection::Local => (side_of(graph.context), "Local event"),
    };
    let mut handler = event_node(
        event_handler_id(graph.id),
        format!("On {name}"),
        handler_subtitle,
        handler_side,
        NodeKind::Event,
        description,
    );
    handler.outputs.push(PinDefinition::exec("then", ""));
    if direction == EventDirection::ToServer {
        handler.outputs.push(player_pin("The player whose client sent the event."));
    }
    handler.outputs.extend(parameters.iter().cloned());
    handler.keywords.push(name.clone());

    let send = |id: String, title: String, subtitle: &str, side: Side, player: Option<&str>| {
        let mut definition = event_node(id, title, subtitle, side, NodeKind::Function, description);
        definition.inputs.push(PinDefinition::exec("exec", ""));
        if let Some(player) = player {
            definition.inputs.push(player_pin(player));
        }
        definition.inputs.extend(parameters.iter().cloned());
        definition.outputs.push(PinDefinition::exec("then", ""));
        definition.keywords.push(name.clone());
        definition
    };
    let mut definitions = vec![handler];
    match direction {
        EventDirection::ToServer => definitions.push(send(
            event_send_id(graph.id),
            format!("Send {name} to Server"),
            "Network event",
            Side::Client,
            None,
        )),
        EventDirection::ToClients => {
            definitions.push(send(
                event_send_to_player_id(graph.id),
                format!("Send {name} to Player"),
                "Network event",
                Side::Server,
                Some("The player whose client receives the event."),
            ));
            definitions.push(send(
                event_send_id(graph.id),
                format!("Send {name} to All Players"),
                "Network event",
                Side::Server,
                None,
            ));
        }
        EventDirection::Local => definitions.push(send(
            event_send_id(graph.id),
            format!("Call {name}"),
            "Local event",
            side_of(graph.context),
            None,
        )),
    }
    definitions
}

pub fn variable_definitions(variable: &VariableDocument, member_of: Option<(&str, bool)>) -> [NodeDefinition; 2] {
    let type_expr = parse_type(&variable.type_name);
    let (subtitle, category) = match member_of {
        Some((class_name, _)) if variable.sync => (format!("Member of {class_name}, synced"), class_name.to_owned()),
        Some((class_name, _)) => (format!("Member of {class_name}"), class_name.to_owned()),
        None => ("Variable".to_owned(), "Variables".to_owned()),
    };
    let target = member_of.map(|(class_name, inside)| target_pin(class_name, inside));
    let mut keywords = vec![variable.name.clone(), "variable".to_owned()];
    if let Some((class_name, _)) = member_of {
        keywords.push(class_name.to_owned());
    }

    let get = NodeDefinition {
        id: variable_get_id(variable.id),
        title: format!("Get {}", variable.name),
        subtitle: Some(subtitle.clone()),
        category: category.clone(),
        kind: NodeKind::Variable,
        side: Side::Both,
        description: variable.description.clone(),
        inputs: target.iter().cloned().collect(),
        outputs: vec![PinDefinition::data("value", &variable.name, type_expr.clone())],
        keywords: keywords.iter().cloned().chain(["get".to_owned()]).collect(),
        api: None,
        context_warnings: Vec::new(),
        callbacks: Vec::new(),
        dynamic: None,
    };

    let mut inputs = vec![PinDefinition::exec("exec", "")];
    inputs.extend(target);
    inputs.push(PinDefinition::data("value", &variable.name, type_expr.clone()));
    let context_warnings = if variable.sync && member_of.is_some() {
        vec![ContextWarning {
            contexts: vec![ScriptContext::Client, ScriptContext::Shared],
            message: format!(
                "'{}' is synced from the server. Setting it on a client only changes that client's copy until the server sends the next value.",
                variable.name
            ),
            hint: "Set synced variables in a server graph, or send a remote event to the server and set it there.".to_owned(),
        }]
    } else {
        Vec::new()
    };
    let set = NodeDefinition {
        id: variable_set_id(variable.id),
        title: format!("Set {}", variable.name),
        subtitle: Some(subtitle),
        category,
        kind: NodeKind::Variable,
        side: Side::Both,
        description: variable.description.clone(),
        inputs,
        outputs: vec![PinDefinition::exec("then", ""), PinDefinition::data("value", &variable.name, type_expr)],
        keywords: keywords.into_iter().chain(["set".to_owned(), "assign".to_owned()]).collect(),
        api: None,
        context_warnings,
        callbacks: Vec::new(),
        dynamic: None,
    };
    [get, set]
}

fn self_definition(class_name: &str) -> NodeDefinition {
    NodeDefinition {
        id: CLASS_SELF.to_owned(),
        title: "Self".to_owned(),
        subtitle: Some(class_name.to_owned()),
        category: class_name.to_owned(),
        kind: NodeKind::Variable,
        side: Side::Both,
        description: format!("The {class_name} instance this code runs for."),
        inputs: Vec::new(),
        outputs: vec![PinDefinition::data(SELF_PIN, "Self", TypeExpr::named(class_name))],
        keywords: vec!["self".to_owned(), "this".to_owned()],
        api: None,
        context_warnings: Vec::new(),
        callbacks: Vec::new(),
        dynamic: None,
    }
}

fn spawn_definition(catalog: &NodeCatalog, class: &ProjectClass, graph: &GraphSummary, description: &str) -> Option<NodeDefinition> {
    let root = class.api_root.as_deref()?;
    let base = catalog
        .get(&format!("construct/{root}"))
        .or_else(|| catalog.get(&format!("construct/{root}/1")))?;
    let mut definition = base.clone();
    definition.id = spawn_node_id(class.id);
    definition.title = format!("Spawn {}", class.name);
    definition.subtitle = Some(format!("Class in {}", location_label(graph)));
    definition.category = class.name.clone();
    definition.side = if side_of(graph.context) == Side::Both { base.side } else { side_of(graph.context) };
    definition.api = None;
    if !description.is_empty() {
        definition.description = description.to_owned();
    }
    definition.keywords = vec![class.name.clone(), "spawn".to_owned(), "new".to_owned(), "create".to_owned()];
    if let Some(output) = definition.outputs.iter_mut().find(|pin| pin.name == "return_value") {
        output.label = class.name.clone();
        output.pin_type = PinType::named(&class.name);
    }
    Some(definition)
}

fn class_event_definitions(catalog: &NodeCatalog, class: &ProjectClass) -> Vec<NodeDefinition> {
    let Some(root) = class.api_root.as_deref() else {
        return Vec::new();
    };
    let prefix = format!("event/{root}/");
    catalog
        .definitions()
        .iter()
        .filter_map(|definition| {
            let event = definition.id.strip_prefix(&prefix)?;
            let mut copy = definition.clone();
            copy.id = class_event_node_id(event);
            copy.title = format!("On {} {}", class.name, humanize(event));
            copy.subtitle = Some(format!("Event of {}", class.name));
            copy.category = class.name.clone();
            copy.api = None;
            for output in copy.outputs.iter_mut().filter(|pin| pin.name == SELF_PIN) {
                output.label = class.name.clone();
                output.pin_type = PinType::named(&class.name);
            }
            Some(copy)
        })
        .collect()
}

fn resolve_classes(types: &TypeSystem, classes: &[(Id, String, String)]) -> Vec<ProjectClass> {
    let by_name: HashMap<&str, &str> = classes.iter().map(|(_, name, parent)| (name.as_str(), parent.as_str())).collect();
    classes
        .iter()
        .map(|(id, name, parent)| {
            let mut parents = Vec::new();
            let mut api_root = None;
            let mut current = parent.as_str();
            while parents.len() < 32 && !parents.iter().any(|seen: &String| seen == current) && current != name {
                parents.push(current.to_owned());
                match by_name.get(current) {
                    Some(next) => current = next,
                    None => {
                        api_root = Some(current.to_owned());
                        parents.extend(types.parents_of(current));
                        break;
                    }
                }
            }
            ProjectClass {
                id: *id,
                name: name.clone(),
                parent: parent.clone(),
                parents,
                api_root,
            }
        })
        .collect()
}

pub fn project_classes(catalog: &NodeCatalog, package_directory: &Path) -> Result<Vec<ProjectClass>, ProjectError> {
    let tree = scan_package_tree(package_directory)?;
    let libraries = library_classes(package_directory)?;
    Ok(resolve_classes(catalog.types(), &class_entries(&tree.graphs, &libraries)))
}

pub fn build_scope(catalog: &NodeCatalog, package_directory: &Path, current_graph: &Path) -> Result<ScopeData, ProjectError> {
    let tree = scan_package_tree(package_directory)?;
    let current = tree.graphs.iter().find(|graph| graph.path == current_graph);
    let current_class = current.and_then(|graph| if graph.kind == GraphKind::Class { Some(graph.id) } else { graph.owner });

    let library_classes = library_classes(package_directory)?;
    let classes = resolve_classes(catalog.types(), &class_entries(&tree.graphs, &library_classes));
    let class_names: HashMap<Id, &str> = classes.iter().map(|class| (class.id, class.name.as_str())).collect();

    let mut definitions = Vec::new();
    for library_class in &library_classes {
        let Some(class) = classes.iter().find(|class| class.id == library_class.graph.id) else {
            continue;
        };
        definitions.extend(spawn_definition(catalog, class, &library_class.graph, &library_class.description));
        if class.api_root.is_some() {
            for mut cast in cast_definitions(&class.name) {
                cast.category = class.name.clone();
                definitions.push(cast);
            }
        }
        for variable in &library_class.variables {
            definitions.extend(variable_definitions(variable, Some((&class.name, false))));
        }
        for method in &library_class.methods {
            definitions.push(method_definition(&method.graph, &method.signature, &class.name, false));
        }
    }
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut interfaces = Vec::new();
    for graph in &tree.graphs {
        match (graph.kind, graph.owner) {
            (GraphKind::Function, None) => {
                definitions.push(call_definition(graph, &GraphManifest::read(&graph.path)?.signature));
            }
            (GraphKind::Function, Some(owner)) => {
                if let Some(class_name) = class_names.get(&owner) {
                    let signature = GraphManifest::read(&graph.path)?.signature;
                    definitions.push(method_definition(graph, &signature, class_name, current_class == Some(owner)));
                }
            }
            (GraphKind::Class, _) => {
                let Some(class) = classes.iter().find(|class| class.id == graph.id) else {
                    continue;
                };
                let inside = current_class == Some(class.id);
                let description = GraphManifest::read(&graph.path)?.signature.description;
                definitions.extend(spawn_definition(catalog, class, graph, &description));
                if class.api_root.is_some() {
                    for mut cast in cast_definitions(&class.name) {
                        cast.category = class.name.clone();
                        definitions.push(cast);
                    }
                }
                for variable in load_variables(&graph.path)? {
                    definitions.extend(variable_definitions(&variable, Some((&class.name, inside))));
                }
                if inside {
                    definitions.push(self_definition(&class.name));
                }
                if current.is_some_and(|current| current.id == class.id) {
                    definitions.extend(class_event_definitions(catalog, class));
                }
            }
            (GraphKind::Event, _) => {
                let manifest = GraphManifest::read(&graph.path)?;
                definitions.extend(event_definitions(graph, manifest.direction.unwrap_or_default(), &manifest.signature));
            }
            (GraphKind::Struct, _) => {
                let manifest = GraphManifest::read(&graph.path)?;
                definitions.extend(struct_definitions(graph, &manifest.signature.description, &manifest.signature.parameters));
                structs.push(ProjectStruct {
                    id: graph.id,
                    name: graph.name.clone(),
                    fields: manifest.signature.parameters,
                });
            }
            (GraphKind::Dispatcher, Some(owner)) => {
                if let Some(class_name) = class_names.get(&owner) {
                    let signature = GraphManifest::read(&graph.path)?.signature;
                    definitions.extend(dispatcher_definitions(graph, &signature, class_name, current_class == Some(owner)));
                }
            }
            (GraphKind::Dispatcher, None) => {}
            (GraphKind::Interface, _) => {
                let manifest = GraphManifest::read(&graph.path)?;
                definitions.extend(interface_definitions(graph, &manifest.functions));
                interfaces.push(graph.name.clone());
            }
            (GraphKind::Enum, _) => {
                let manifest = GraphManifest::read(&graph.path)?;
                definitions.push(enum_switch_definition(&graph.name, &manifest.signature.parameters));
                enums.push(ProjectEnum {
                    id: graph.id,
                    name: graph.name.clone(),
                    values: manifest.signature.parameters,
                });
            }
            (GraphKind::Macro, _) => {
                if current.is_some_and(|current| current.id == graph.id) {
                    continue;
                }
                definitions.push(macro_definition(graph, &GraphManifest::read(&graph.path)?.signature));
            }
            (GraphKind::EventGraph, _) => {}
        }
    }
    if package_directory.join(PackageManifest::FILE_NAME).is_file() {
        for library in required_libraries(package_directory)? {
            definitions.extend(library.functions.iter().map(|function| library_definition(&library, function)));
        }
    }

    if let Some(current) = current {
        if current.kind != GraphKind::Class {
            for variable in load_variables(&current.path)? {
                definitions.extend(variable_definitions(&variable, None));
            }
        }
        if current.kind == GraphKind::Function {
            let signature = GraphManifest::read(&current.path)?.signature;
            let owner = current.owner.and_then(|owner| class_names.get(&owner).copied());
            definitions.push(entry_definition(current, &signature, owner));
            definitions.push(return_definition(&signature));
        }
        if current.kind == GraphKind::Macro {
            let signature = GraphManifest::read(&current.path)?.signature;
            definitions.push(macro_inputs_definition(current, &signature));
            definitions.push(macro_outputs_definition(current, &signature));
        }
    }

    Ok(ScopeData {
        definitions,
        classes,
        structs,
        enums,
        interfaces,
    })
}

fn is_known_type(types: &TypeSystem, type_expr: &TypeExpr) -> bool {
    match type_expr {
        TypeExpr::Named(name) => types.is_known(name),
        TypeExpr::Array(inner) | TypeExpr::Optional(inner) | TypeExpr::Variadic(inner) => is_known_type(types, inner),
        TypeExpr::Union(members) => members.iter().all(|member| is_known_type(types, member)),
    }
}

fn check_name(name: &str, singular: &str, field: &str, previous: &[&str], issues: &mut Vec<FieldIssue>) {
    let name = name.trim();
    if name.is_empty() {
        issues.push(FieldIssue::error(field, format!("Give every {singular} a name.")));
    } else if name.chars().count() > MAX_NAME_LENGTH {
        issues.push(FieldIssue::error(field, format!("'{name}' is longer than {MAX_NAME_LENGTH} characters.")));
    } else if !name.starts_with(|character: char| character.is_ascii_alphabetic())
        || !name.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, ' ' | '_'))
    {
        issues.push(FieldIssue::error(
            field,
            format!("'{name}' must start with a letter and may only contain letters, digits, spaces and '_', because it becomes a Lua name."),
        ));
    } else if previous.iter().any(|other| other.trim().eq_ignore_ascii_case(name)) {
        issues.push(FieldIssue::error(field, format!("There is already a {singular} called '{name}'.")));
    }
}

fn check_type(type_name: &str, name: &str, singular: &str, field: &str, types: &TypeSystem, issues: &mut Vec<FieldIssue>) {
    match TypeExpr::parse(type_name) {
        Ok(type_expr) if is_known_type(types, &type_expr) => {}
        _ => issues.push(FieldIssue::error(field, format!("'{type_name}' is not a known type for the {singular} '{name}'."))),
    }
}

fn validate_pins(pins: &[FunctionPin], list: &str, singular: &str, allow_exec: bool, types: &TypeSystem, issues: &mut Vec<FieldIssue>) {
    for (index, function_pin) in pins.iter().enumerate() {
        let field = format!("{list}.{index}");
        let previous: Vec<&str> = pins[..index].iter().map(|other| other.name.as_str()).collect();
        check_name(&function_pin.name, singular, &field, &previous, issues);
        if pins[..index].iter().any(|other| other.id == function_pin.id) || function_pin.id.trim().is_empty() {
            issues.push(FieldIssue::error(
                &field,
                format!("The {singular} '{}' has a broken internal id. Remove it and add it again.", function_pin.name),
            ));
        }
        if !(allow_exec && function_pin.type_name == EXEC_TYPE) {
            check_type(&function_pin.type_name, &function_pin.name, singular, &field, types, issues);
        }
    }
}

pub fn validate_signature(signature: &FunctionSignature, types: &TypeSystem) -> Vec<FieldIssue> {
    let mut issues = Vec::new();
    validate_pins(&signature.parameters, "parameters", "parameter", false, types, &mut issues);
    validate_pins(&signature.returns, "returns", "return value", false, types, &mut issues);
    issues
}

pub fn validate_macro_signature(signature: &FunctionSignature, types: &TypeSystem) -> Vec<FieldIssue> {
    let mut issues = Vec::new();
    validate_pins(&signature.parameters, "parameters", "input", true, types, &mut issues);
    validate_pins(&signature.returns, "returns", "output", true, types, &mut issues);
    issues
}

pub fn validate_variables(variables: &[VariableDocument], types: &TypeSystem) -> Vec<FieldIssue> {
    let mut issues = Vec::new();
    for (index, variable) in variables.iter().enumerate() {
        let field = format!("variables.{}", variable.id);
        let previous: Vec<&str> = variables[..index].iter().map(|other| other.name.as_str()).collect();
        check_name(&variable.name, "variable", &field, &previous, &mut issues);
        check_type(&variable.type_name, &variable.name, "variable", &field, types, &mut issues);
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DiagnosticSeverity, analyze_graph};
    use nf_api::{ApiChannel, ApiDatabase};
    use nf_project::{
        GraphDocument, NodeDocument, PinLink, PinValue, apply_variable_changes, context_root, create_class, create_graph,
        create_method, update_function_signature,
    };

    fn function_pin(id: &str, name: &str, type_name: &str) -> FunctionPin {
        FunctionPin {
            id: id.to_owned(),
            name: name.to_owned(),
            type_name: type_name.to_owned(),
        }
    }

    fn catalog() -> NodeCatalog {
        NodeCatalog::from_database(&ApiDatabase::bundled(ApiChannel::Stable).unwrap())
    }

    fn variable(name: &str, type_name: &str) -> VariableDocument {
        VariableDocument {
            id: Id::generate(),
            name: name.to_owned(),
            type_name: type_name.to_owned(),
            default: None,
            description: String::new(),
            sync: false,
        }
    }

    #[test]
    fn functions_become_callable_nodes_with_their_signature() {
        let catalog = catalog();
        let temporary = tempfile::tempdir().unwrap();
        let package = temporary.path();
        let function = create_graph(&context_root(package, ScriptContext::Shared), "Heal Player", GraphKind::Function).unwrap();
        update_function_signature(
            &function,
            FunctionSignature {
                description: "Heals a character".to_owned(),
                pure: false,
                exported: false,
                parameters: vec![function_pin("p1", "Target", "Character"), function_pin("p2", "Amount", "integer")],
                returns: vec![function_pin("r1", "New Health", "integer")],
            },
        )
        .unwrap();
        let caller = create_graph(&context_root(package, ScriptContext::Server), "Main", GraphKind::EventGraph).unwrap();

        let scope = build_scope(&catalog, package, &caller).unwrap();
        assert_eq!(scope.definitions.len(), 1);
        let call = &scope.definitions[0];
        assert_eq!(call.side, Side::Both);
        assert_eq!(call.input("p2").unwrap().label, "Amount");

        let function_scope = build_scope(&catalog, package, &function).unwrap();
        assert_eq!(function_scope.definitions.len(), 3);

        let scope = GraphScope::new(&catalog, function_scope);
        let document = GraphDocument::load(&function).unwrap();
        let diagnostics = analyze_graph(&scope, ScriptContext::Shared, GraphKind::Function, &document.nodes);
        assert!(diagnostics.is_empty(), "{diagnostics:#?}");

        let mut entry = document.nodes[0].clone();
        let mut set_health = NodeDocument::new("call/Damageable/SetHealth", [300.0, 0.0]);
        set_health.inputs.insert("self".to_owned(), PinValue::Link(PinLink { node: entry.id, pin: "p1".to_owned() }));
        set_health.inputs.insert("new_health".to_owned(), PinValue::Link(PinLink { node: entry.id, pin: "p2".to_owned() }));
        entry.exec.insert("then".to_owned(), PinLink { node: set_health.id, pin: "exec".to_owned() });
        let diagnostics = analyze_graph(&scope, ScriptContext::Shared, GraphKind::Function, &[entry, set_health]);
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "wrong-side");

        let empty = analyze_graph(&scope, ScriptContext::Shared, GraphKind::Function, &[]);
        assert_eq!(empty[0].code, "missing-entry");
        assert_eq!(empty[0].severity, DiagnosticSeverity::Error);
    }

    #[test]
    fn classes_provide_spawn_events_members_and_methods() {
        let catalog = catalog();
        let temporary = tempfile::tempdir().unwrap();
        let package = temporary.path();
        let shared = context_root(package, ScriptContext::Shared);
        let zombie = create_class(&shared, "Zombie", "Character").unwrap();
        let boss = create_class(&shared, "ZombieBoss", "Zombie").unwrap();
        let bite = create_method(&zombie, "Bite").unwrap();
        update_function_signature(
            &bite,
            FunctionSignature {
                parameters: vec![function_pin("v1", "Victim", "Character")],
                ..FunctionSignature::default()
            },
        )
        .unwrap();
        let rage = variable("Rage", "number");
        apply_variable_changes(&zombie, std::slice::from_ref(&rage), &[]).unwrap();
        let server_main = create_graph(&context_root(package, ScriptContext::Server), "Main", GraphKind::EventGraph).unwrap();

        let outside = build_scope(&catalog, package, &server_main).unwrap();
        let boss_class = outside.classes.iter().find(|class| class.name == "ZombieBoss").unwrap();
        assert_eq!(boss_class.api_root.as_deref(), Some("Character"));
        assert!(boss_class.parents.iter().any(|parent| parent == "Zombie"));
        assert!(boss_class.parents.iter().any(|parent| parent == "Pawn"));

        let types = catalog.types().with_project_classes(
            &outside.classes.iter().map(|class| (class.name.clone(), class.parents.clone())).collect::<Vec<_>>(),
        );
        assert!(types.is_assignable(&TypeExpr::named("ZombieBoss"), &TypeExpr::named("Character")));
        assert!(types.is_assignable(&TypeExpr::named("ZombieBoss"), &TypeExpr::named("Zombie")));
        assert!(!types.is_assignable(&TypeExpr::named("Character"), &TypeExpr::named("Zombie")));

        let ids: Vec<&str> = outside.definitions.iter().map(|definition| definition.id.as_str()).collect();
        assert!(ids.iter().any(|id| id.starts_with("class/spawn/")));
        let get_rage = outside.definitions.iter().find(|definition| definition.title == "Get Rage").unwrap();
        assert!(!get_rage.input("self").unwrap().optional);
        assert!(!ids.contains(&CLASS_SELF));
        assert!(!ids.iter().any(|id| id.starts_with("class-event/")));

        let inside = build_scope(&catalog, package, &zombie).unwrap();
        let get_rage = inside.definitions.iter().find(|definition| definition.title == "Get Rage").unwrap();
        assert!(get_rage.input("self").unwrap().optional);
        let death = inside.definitions.iter().find(|definition| definition.id == "class-event/Death").unwrap();
        assert_eq!(death.output("self").unwrap().pin_type, PinType::named("Zombie"));

        let method = build_scope(&catalog, package, &bite).unwrap();
        let entry = method.definitions.iter().find(|definition| definition.id == FUNCTION_ENTRY).unwrap();
        assert_eq!(entry.output("self").unwrap().pin_type, PinType::named("Zombie"));
        assert!(method.definitions.iter().any(|definition| definition.id == CLASS_SELF));
        assert!(outside.definitions.iter().any(|definition| definition.id == "cast/ZombieBoss"));
        let _ = boss;
    }

    #[test]
    fn synced_members_warn_when_set_on_clients_and_casts_check_types() {
        let catalog = catalog();
        let temporary = tempfile::tempdir().unwrap();
        let package = temporary.path();
        let zombie = create_class(&context_root(package, ScriptContext::Shared), "Zombie", "Character").unwrap();
        let mut hunger = variable("Hunger", "number");
        hunger.sync = true;
        apply_variable_changes(&zombie, std::slice::from_ref(&hunger), &[]).unwrap();
        let client = create_graph(&context_root(package, ScriptContext::Client), "Hud", GraphKind::EventGraph).unwrap();
        let scope = GraphScope::new(&catalog, build_scope(&catalog, package, &client).unwrap());

        let mut load = NodeDocument::new("event/Package/Load", [0.0, 0.0]);
        let mut cast = NodeDocument::new("cast/Zombie", [200.0, 0.0]);
        let mut set = NodeDocument::new(variable_set_id(hunger.id), [400.0, 0.0]);
        set.inputs.insert("self".to_owned(), PinValue::Link(PinLink { node: cast.id, pin: "result".to_owned() }));
        cast.exec.insert("then".to_owned(), PinLink { node: set.id, pin: "exec".to_owned() });
        load.exec.insert("then".to_owned(), PinLink { node: cast.id, pin: "exec".to_owned() });
        let diagnostics = analyze_graph(&scope, ScriptContext::Client, GraphKind::EventGraph, &[load, cast, set]);
        let codes: Vec<&str> = diagnostics.iter().map(|diagnostic| diagnostic.code).collect();
        assert_eq!(codes, ["context-warning"], "{diagnostics:#?}");
    }

    #[test]
    fn validates_signatures_and_variables() {
        let catalog = catalog();
        let signature = FunctionSignature {
            parameters: vec![
                function_pin("a", "Speed", "number"),
                function_pin("b", "speed", "Vector[]"),
                function_pin("c", "", "NotAType"),
            ],
            ..FunctionSignature::default()
        };
        let issues = validate_signature(&signature, catalog.types());
        let fields: Vec<&str> = issues.iter().map(|issue| issue.field.as_str()).collect();
        assert_eq!(fields, ["parameters.1", "parameters.2", "parameters.2"]);

        let variables = [variable("Score", "integer"), variable("score", "integer"), variable("2fast", "Vector")];
        assert_eq!(validate_variables(&variables, catalog.types()).len(), 2);
    }
}
