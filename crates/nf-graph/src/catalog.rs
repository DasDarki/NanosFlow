use std::collections::HashMap;

use nf_api::{
    ApiChannel, ApiDatabase, Authority, ClassDefinition, ClassKind, Event, Function, Operator, Parameter, Property,
    ReturnValue, TypeExpr,
};
use nf_project::ScriptContext;
use serde::Serialize;

use crate::builtins::builtin_definitions;
use crate::pin::PinDefinition;
use crate::text::{humanize, strip_markup};
use crate::types::TypeSystem;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    Both,
    Server,
    Client,
}

impl Side {
    fn from_authority(authority: Option<Authority>) -> Side {
        match authority {
            Some(Authority::Server) => Side::Server,
            Some(Authority::Client) => Side::Client,
            _ => Side::Both,
        }
    }

    pub fn runs_in(self, context: ScriptContext) -> bool {
        matches!(
            (self, context),
            (Side::Both, _) | (Side::Server, ScriptContext::Server) | (Side::Client, ScriptContext::Client)
        )
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Side::Both => "server and client",
            Side::Server => "server",
            Side::Client => "client",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NodeKind {
    Event,
    Function,
    PureFunction,
    Constructor,
    FlowControl,
    Operator,
    Literal,
    FunctionEntry,
    FunctionReturn,
    Variable,
    Comment,
    Reroute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum BindingKind {
    Event,
    Method,
    StaticFunction,
    Constructor,
    Operator,
    StaticProperty,
    BreakStruct,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiBinding {
    pub kind: BindingKind,
    pub class: String,
    pub member: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextWarning {
    pub contexts: Vec<ScriptContext>,
    pub message: String,
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CallbackDefinition {
    pub parameter: String,
    pub position: usize,
    pub exec: String,
    pub arguments: Vec<String>,
    pub variadic: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case", rename_all_fields = "camelCase")]
pub enum DynamicPins {
    ExecOutputs {
        prefix: String,
        label: String,
        minimum: usize,
        initial: usize,
        fallback: usize,
    },
    DataInputs {
        prefix: String,
        label: String,
        minimum: usize,
        initial: usize,
        fallback: usize,
    },
    Cases,
    Tracks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDefinition {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    pub category: String,
    pub kind: NodeKind,
    pub side: Side,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    pub inputs: Vec<PinDefinition>,
    pub outputs: Vec<PinDefinition>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub keywords: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<ApiBinding>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub context_warnings: Vec<ContextWarning>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub callbacks: Vec<CallbackDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dynamic: Option<DynamicPins>,
}

impl NodeDefinition {
    pub fn input(&self, name: &str) -> Option<&PinDefinition> {
        self.inputs.iter().find(|pin| pin.name == name)
    }

    pub fn output(&self, name: &str) -> Option<&PinDefinition> {
        self.outputs.iter().find(|pin| pin.name == name)
    }

    pub fn has_exec_input(&self) -> bool {
        self.inputs.iter().any(|pin| pin.pin_type.is_exec())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassInfo {
    pub name: String,
    pub kind: &'static str,
    pub parents: Vec<String>,
    pub is_base: bool,
    pub inheritable: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumValueInfo {
    pub key: String,
    pub value: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnumInfo {
    pub name: String,
    pub values: Vec<EnumValueInfo>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogPayload<'a> {
    pub channel: ApiChannel,
    pub revision: &'a str,
    pub nodes: &'a [NodeDefinition],
    pub classes: &'a [ClassInfo],
    pub enums: &'a [EnumInfo],
}

#[derive(Debug, Clone)]
pub struct NodeCatalog {
    channel: ApiChannel,
    revision: String,
    definitions: Vec<NodeDefinition>,
    index: HashMap<String, usize>,
    classes: Vec<ClassInfo>,
    enums: Vec<EnumInfo>,
    types: TypeSystem,
}

const SELF_PIN: &str = "self";
const EVENT_SYSTEM_FUNCTIONS: [&str; 5] = ["Subscribe", "SubscribeRemote", "Unsubscribe", "Inherit", "GetInheritedClasses"];
const HIDDEN_FUNCTIONS: [&str; 2] = ["Subscribe", "SubscribeRemote"];
const EXEC_INPUT: &str = "exec";
const EXEC_OUTPUT: &str = "then";

impl NodeCatalog {
    pub fn from_database(database: &ApiDatabase) -> NodeCatalog {
        let mut builder = CatalogBuilder {
            definitions: Vec::new(),
            index: HashMap::new(),
        };
        for definition in builtin_definitions() {
            builder.push(definition);
        }
        for (name, definition) in database.enums() {
            builder.push(switch_enum_definition(name, definition));
        }
        for (name, kind, class) in database.classes() {
            let is_entity = name == "Entity" || class.inheritance.iter().any(|parent| parent == "Entity");
            if kind == ClassKind::Class && is_entity {
                for definition in cast_definitions(name) {
                    builder.push(definition);
                }
            }
        }
        for (name, kind, class) in database.classes() {
            builder.add_class(database, name, kind, class);
        }

        let classes = database
            .classes()
            .map(|(name, kind, definition)| ClassInfo {
                name: name.to_owned(),
                kind: class_kind_name(kind),
                parents: definition.inheritance.clone(),
                is_base: definition.is_base,
                inheritable: kind == ClassKind::Class
                    && !definition.is_base
                    && definition.inheritance.iter().any(|parent| parent == "Entity"),
            })
            .collect();
        let enums = database
            .enums()
            .map(|(name, definition)| EnumInfo {
                name: name.to_owned(),
                values: definition
                    .values
                    .iter()
                    .map(|value| EnumValueInfo {
                        key: value.key.clone(),
                        value: value.value.clone(),
                        description: value.description.as_deref().map(strip_markup).unwrap_or_default(),
                    })
                    .collect(),
            })
            .collect();

        NodeCatalog {
            channel: database.channel(),
            revision: database.revision().to_owned(),
            definitions: builder.definitions,
            index: builder.index,
            classes,
            enums,
            types: TypeSystem::from_database(database),
        }
    }

    pub fn get(&self, id: &str) -> Option<&NodeDefinition> {
        self.index.get(id).map(|position| &self.definitions[*position])
    }

    pub fn definitions(&self) -> &[NodeDefinition] {
        &self.definitions
    }

    pub fn enum_has_key(&self, enum_name: &str, key: &str) -> bool {
        self.enums
            .iter()
            .find(|definition| definition.name == enum_name)
            .is_some_and(|definition| definition.values.iter().any(|value| value.key == key))
    }

    pub fn types(&self) -> &TypeSystem {
        &self.types
    }

    pub fn class_info(&self, name: &str) -> Option<&ClassInfo> {
        self.classes.iter().find(|class| class.name == name)
    }

    pub fn is_inheritable(&self, name: &str) -> bool {
        self.class_info(name).is_some_and(|class| class.inheritable)
    }

    pub fn concrete_classes_of(&self, name: &str) -> Vec<&str> {
        self.classes
            .iter()
            .filter(|class| {
                class.kind == "class" && !class.is_base && (class.name == name || class.parents.iter().any(|parent| parent == name))
            })
            .map(|class| class.name.as_str())
            .collect()
    }

    pub fn payload(&self) -> CatalogPayload<'_> {
        CatalogPayload {
            channel: self.channel,
            revision: &self.revision,
            nodes: &self.definitions,
            classes: &self.classes,
            enums: &self.enums,
        }
    }
}

fn class_kind_name(kind: ClassKind) -> &'static str {
    match kind {
        ClassKind::Class => "class",
        ClassKind::StaticClass => "static-class",
        ClassKind::StandardLibrary => "standard-library",
        ClassKind::Struct => "struct",
        ClassKind::UtilityClass => "utility-class",
    }
}

struct CatalogBuilder {
    definitions: Vec<NodeDefinition>,
    index: HashMap<String, usize>,
}

fn description(text: &str) -> String {
    strip_markup(text)
}

fn parse_type(type_name: &str) -> TypeExpr {
    TypeExpr::parse(type_name).unwrap_or_else(|_| TypeExpr::named("any"))
}

fn parameter_pin(parameter: &Parameter) -> PinDefinition {
    let pin = PinDefinition::data(&parameter.name, &humanize(&parameter.name), parse_type(&parameter.type_name))
        .with_description(parameter.description.as_deref().map(description).unwrap_or_default());
    match &parameter.default {
        Some(default) => pin.with_default(default),
        None => pin,
    }
}

fn return_pins(returns: &[ReturnValue]) -> Vec<PinDefinition> {
    let mut pins: Vec<PinDefinition> = Vec::with_capacity(returns.len());
    for (position, value) in returns.iter().enumerate() {
        let base = value
            .name
            .as_deref()
            .map(|name| name.trim_end_matches("...").to_owned())
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| {
                if returns.len() == 1 {
                    "return_value".to_owned()
                } else {
                    format!("return_value_{}", position + 1)
                }
            });
        let mut name = base.clone();
        let mut suffix = 2;
        while pins.iter().any(|pin| pin.name == name) || name == SELF_PIN {
            name = format!("{base}_{suffix}");
            suffix += 1;
        }
        let label = if name.starts_with("return_value") {
            "Return Value".to_owned()
        } else {
            humanize(&name)
        };
        pins.push(
            PinDefinition::data(&name, &label, parse_type(&value.type_name))
                .with_description(value.description.as_deref().map(description).unwrap_or_default()),
        );
    }
    pins
}

fn is_pure_function(kind: ClassKind, class_name: &str, function: &Function) -> bool {
    if function.returns.is_empty() {
        return false;
    }
    match kind {
        ClassKind::Struct => true,
        ClassKind::StandardLibrary => class_name != "table",
        ClassKind::UtilityClass => true,
        ClassKind::Class | ClassKind::StaticClass => {
            let name = function.name.as_str();
            ["Get", "Is", "Has", "Was", "Can"].iter().any(|prefix| {
                name.strip_prefix(prefix)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with(|character: char| character.is_ascii_uppercase()))
            })
        }
    }
}

fn static_property_type(class_name: &str, property: &Property) -> TypeExpr {
    if let Some(type_name) = &property.type_name {
        return parse_type(type_name);
    }
    let value = property.value.as_deref().unwrap_or_default();
    if value.starts_with(&format!("{class_name}(")) {
        TypeExpr::named(class_name)
    } else if value.parse::<f64>().is_ok() || class_name == "math" {
        TypeExpr::named("number")
    } else {
        TypeExpr::named("any")
    }
}

fn operator_title(class_name: &str, operator: &Operator) -> String {
    let side = |type_name: &Option<String>| type_name.clone().unwrap_or_else(|| class_name.to_owned());
    match operator.operator.as_str() {
        "tostring" => "To String".to_owned(),
        "__unm" => format!("Negate {class_name}"),
        _ => format!("{} {} {}", side(&operator.lhs), operator.name, side(&operator.rhs)),
    }
}

impl CatalogBuilder {
    fn push(&mut self, mut definition: NodeDefinition) {
        if self.index.contains_key(&definition.id) {
            let side_suffix = format!("{}@{}", definition.id, side_identifier(definition.side));
            let mut candidate = side_suffix.clone();
            let mut counter = 2;
            while self.index.contains_key(&candidate) {
                candidate = format!("{side_suffix}-{counter}");
                counter += 1;
            }
            definition.id = candidate;
        }
        self.index.insert(definition.id.clone(), self.definitions.len());
        self.definitions.push(definition);
    }

    fn add_class(&mut self, database: &ApiDatabase, name: &str, kind: ClassKind, class: &ClassDefinition) {
        let is_instance_type = matches!(kind, ClassKind::Class | ClassKind::Struct);

        if kind == ClassKind::Class && !class.is_base {
            if let Some(resolved) = database.resolve(name) {
                for (owner, event) in resolved.events() {
                    self.push(event_definition(name, &owner.name, event));
                }
            }
        } else if kind != ClassKind::Class {
            for event in &class.events {
                self.push(event_definition(name, name, event));
            }
        }

        if is_instance_type {
            for function in class.functions.iter().filter(|function| !HIDDEN_FUNCTIONS.contains(&function.name.as_str())) {
                self.push(function_definition(kind, name, function, true));
            }
        }
        if !class.is_base {
            for function in class.static_functions.iter().filter(|function| !HIDDEN_FUNCTIONS.contains(&function.name.as_str())) {
                self.push(function_definition(kind, name, function, false));
            }
        }
        if kind == ClassKind::Class && !class.is_base {
            let inherited = class
                .inheritance
                .iter()
                .filter_map(|parent| database.class(parent))
                .filter(|parent| parent.is_base)
                .flat_map(|parent| parent.static_functions.iter())
                .filter(|function| !EVENT_SYSTEM_FUNCTIONS.contains(&function.name.as_str()))
                .filter(|function| !class.static_functions.iter().any(|own| own.name == function.name));
            for function in inherited {
                self.push(function_definition(kind, name, function, false));
            }
        }

        if !class.is_base {
            let constructor_count = class.constructors.len();
            for (position, constructor) in class.constructors.iter().enumerate() {
                let id = if constructor_count == 1 {
                    format!("construct/{name}")
                } else {
                    format!("construct/{name}/{}", position + 1)
                };
                let is_struct = kind == ClassKind::Struct;
                let variant = constructor
                    .name
                    .trim_end_matches("Constructor")
                    .trim()
                    .trim_start_matches("Default")
                    .trim()
                    .to_owned();
                let title = match (is_struct, variant.is_empty()) {
                    (true, _) => format!("Make {name}"),
                    (false, true) => format!("Spawn {name}"),
                    (false, false) => format!("Spawn {name} ({variant})"),
                };
                let mut inputs = Vec::new();
                if !is_struct {
                    inputs.push(PinDefinition::exec(EXEC_INPUT, ""));
                }
                inputs.extend(constructor.parameters.iter().map(parameter_pin));
                let mut outputs = Vec::new();
                if !is_struct {
                    outputs.push(PinDefinition::exec(EXEC_OUTPUT, ""));
                }
                outputs.push(PinDefinition::data("return_value", name, TypeExpr::named(name)));
                self.push(NodeDefinition {
                    id,
                    title,
                    subtitle: None,
                    category: name.to_owned(),
                    kind: if is_struct { NodeKind::PureFunction } else { NodeKind::Constructor },
                    side: if is_struct { Side::Both } else { Side::from_authority(class.authority) },
                    description: constructor
                        .description
                        .as_deref()
                        .map(description)
                        .filter(|text| !text.is_empty())
                        .unwrap_or_else(|| description(&class.description)),
                    inputs,
                    outputs,
                    keywords: vec![name.to_owned(), "new".to_owned(), "create".to_owned()],
                    api: Some(ApiBinding {
                        kind: BindingKind::Constructor,
                        class: name.to_owned(),
                        member: position.to_string(),
                    }),
                    context_warnings: Vec::new(),
                    callbacks: Vec::new(),
                    dynamic: None,
                });
            }
        }

        if kind == ClassKind::Struct && !class.properties.is_empty() {
            self.push(NodeDefinition {
                id: format!("break/{name}"),
                title: format!("Break {name}"),
                subtitle: None,
                category: name.to_owned(),
                kind: NodeKind::PureFunction,
                side: Side::Both,
                description: format!("Splits a {name} into its components."),
                inputs: vec![PinDefinition::data(SELF_PIN, name, TypeExpr::named(name))],
                outputs: class
                    .properties
                    .iter()
                    .map(|property| {
                        PinDefinition::data(
                            &property.name,
                            &humanize(&property.name),
                            property.type_name.as_deref().map(parse_type).unwrap_or_else(|| TypeExpr::named("any")),
                        )
                        .with_description(property.description.as_deref().map(description).unwrap_or_default())
                    })
                    .collect(),
                keywords: vec!["split".to_owned(), name.to_owned()],
                api: Some(ApiBinding {
                    kind: BindingKind::BreakStruct,
                    class: name.to_owned(),
                    member: String::new(),
                }),
                context_warnings: Vec::new(),
                callbacks: Vec::new(),
                dynamic: None,
            });
        }

        for property in &class.static_properties {
            self.push(NodeDefinition {
                id: format!("property/{name}/{}", property.name),
                title: format!("{name}.{}", property.name),
                subtitle: None,
                category: name.to_owned(),
                kind: NodeKind::PureFunction,
                side: Side::Both,
                description: property.description.as_deref().map(description).unwrap_or_default(),
                inputs: Vec::new(),
                outputs: vec![PinDefinition::data(
                    "value",
                    &humanize(&property.name),
                    static_property_type(name, property),
                )],
                keywords: vec![property.name.clone(), name.to_owned()],
                api: Some(ApiBinding {
                    kind: BindingKind::StaticProperty,
                    class: name.to_owned(),
                    member: property.name.clone(),
                }),
                context_warnings: Vec::new(),
                callbacks: Vec::new(),
                dynamic: None,
            });
        }

        for operator in &class.operators {
            let left = operator.lhs.as_deref().map(parse_type).unwrap_or_else(|| TypeExpr::named(name));
            let mut inputs = vec![PinDefinition::data("a", "", left)];
            if let Some(right) = &operator.rhs {
                inputs.push(PinDefinition::data("b", "", parse_type(right)));
            }
            self.push(NodeDefinition {
                id: format!("operator/{name}/{}", operator.operator),
                title: operator_title(name, operator),
                subtitle: None,
                category: name.to_owned(),
                kind: NodeKind::Operator,
                side: Side::Both,
                description: String::new(),
                inputs,
                outputs: vec![PinDefinition::data("result", "", parse_type(&operator.returns))],
                keywords: vec![operator.name.clone(), operator.operator.clone(), name.to_owned()],
                api: Some(ApiBinding {
                    kind: BindingKind::Operator,
                    class: name.to_owned(),
                    member: operator.operator.clone(),
                }),
                context_warnings: Vec::new(),
                callbacks: Vec::new(),
                dynamic: None,
            });
        }
    }
}

fn switch_enum_definition(name: &str, definition: &nf_api::EnumDefinition) -> NodeDefinition {
    let mut outputs: Vec<PinDefinition> = definition
        .values
        .iter()
        .map(|value| {
            PinDefinition::exec(&format!("{}{}", crate::CASE_PREFIX, value.key), &value.key)
                .with_description(value.description.as_deref().map(description).unwrap_or_default())
        })
        .collect();
    outputs.push(PinDefinition::exec("default", "Default"));
    NodeDefinition {
        id: format!("flow/switch-enum/{name}"),
        title: format!("Switch on {name}"),
        subtitle: None,
        category: "Switch".to_owned(),
        kind: NodeKind::FlowControl,
        side: Side::Both,
        description: format!("Continues with the output that matches the {name} value, or with Default."),
        inputs: vec![PinDefinition::exec(EXEC_INPUT, ""), PinDefinition::data("value", "Value", TypeExpr::named(name))],
        outputs,
        keywords: vec!["switch".to_owned(), "case".to_owned(), "enum".to_owned(), name.to_owned()],
        api: None,
        context_warnings: Vec::new(),
        callbacks: Vec::new(),
        dynamic: None,
    }
}

pub fn cast_definitions(class_name: &str) -> [NodeDefinition; 2] {
    let mut object = PinDefinition::data("object", "Object", TypeExpr::named("Entity"))
        .with_description("The entity to check. Nothing connected or nil counts as a failed check.");
    object.optional = true;
    [
        NodeDefinition {
            id: format!("cast/{class_name}"),
            title: format!("Cast to {class_name}"),
            subtitle: None,
            category: "Casting".to_owned(),
            kind: NodeKind::FlowControl,
            side: Side::Both,
            description: format!(
                "Checks whether the object is a {class_name} (or inherits from it). On success it continues with the object typed as {class_name}, otherwise with Cast Failed."
            ),
            inputs: vec![PinDefinition::exec("exec", ""), object.clone()],
            outputs: vec![
                PinDefinition::exec("then", ""),
                PinDefinition::exec("failed", "Cast Failed"),
                PinDefinition::data("result", &format!("As {class_name}"), TypeExpr::named(class_name)),
            ],
            keywords: vec!["cast".to_owned(), "is a".to_owned(), "isa".to_owned(), "as".to_owned(), class_name.to_owned()],
            api: None,
            context_warnings: Vec::new(),
            callbacks: Vec::new(),
            dynamic: None,
        },
        NodeDefinition {
            id: format!("is-a/{class_name}"),
            title: format!("Is {class_name}"),
            subtitle: None,
            category: "Casting".to_owned(),
            kind: NodeKind::PureFunction,
            side: Side::Both,
            description: format!("True when the object is a {class_name} or inherits from it. Uses IsA."),
            inputs: vec![object],
            outputs: vec![PinDefinition::data("result", "", TypeExpr::named("boolean"))],
            keywords: vec!["isa".to_owned(), "is a".to_owned(), "check".to_owned(), "type".to_owned(), class_name.to_owned()],
            api: None,
            context_warnings: Vec::new(),
            callbacks: Vec::new(),
            dynamic: None,
        },
    ]
}

fn side_identifier(side: Side) -> &'static str {
    match side {
        Side::Both => "both",
        Side::Server => "server",
        Side::Client => "client",
    }
}

fn event_definition(class_name: &str, owner_name: &str, event: &Event) -> NodeDefinition {
    let mut outputs = vec![PinDefinition::exec(EXEC_OUTPUT, "")];
    for argument in &event.arguments {
        let mut pin = parameter_pin(argument);
        pin.default = None;
        pin.optional = false;
        if argument.name == SELF_PIN {
            pin.label = class_name.to_owned();
            if argument.type_name == owner_name {
                pin.pin_type = crate::PinType::named(class_name);
            }
        }
        outputs.push(pin);
    }
    let subtitle = (owner_name != class_name).then(|| format!("Inherited from {owner_name}"));
    NodeDefinition {
        id: format!("event/{class_name}/{}", event.name),
        title: format!("On {class_name} {}", humanize(&event.name)),
        subtitle,
        category: class_name.to_owned(),
        kind: NodeKind::Event,
        side: Side::from_authority(event.authority),
        description: description(&event.description),
        inputs: Vec::new(),
        outputs,
        keywords: vec![event.name.clone(), class_name.to_owned(), "event".to_owned()],
        api: Some(ApiBinding {
            kind: BindingKind::Event,
            class: class_name.to_owned(),
            member: event.name.clone(),
        }),
        context_warnings: Vec::new(),
        callbacks: Vec::new(),
        dynamic: None,
    }
}

fn is_function_type(parameter: &Parameter) -> bool {
    parameter.type_name == "function"
}

fn callback_pins(parameter: &Parameter, position: usize) -> (CallbackDefinition, Vec<PinDefinition>) {
    let mut outputs = vec![
        PinDefinition::exec(&parameter.name, &humanize(&parameter.name))
            .with_description(parameter.description.as_deref().map(description).unwrap_or_default()),
    ];
    let mut arguments = Vec::new();
    let mut variadic = false;
    for argument in &parameter.function_parameters {
        let raw = argument.name.trim_end_matches('?').trim_end_matches("...");
        let is_variadic = argument.type_name.ends_with("...") || argument.name.ends_with("...");
        let base = parse_type(argument.type_name.trim_end_matches("..."));
        let type_expr = if is_variadic {
            TypeExpr::array(base)
        } else if argument.name.ends_with('?') {
            TypeExpr::Optional(Box::new(base))
        } else {
            base
        };
        variadic |= is_variadic;
        let name = format!("{}_{raw}", parameter.name);
        outputs.push(
            PinDefinition::data(&name, &humanize(raw), type_expr)
                .with_description(argument.description.as_deref().map(description).unwrap_or_default()),
        );
        arguments.push(name);
        if is_variadic {
            break;
        }
    }
    (
        CallbackDefinition {
            parameter: parameter.name.clone(),
            position,
            exec: parameter.name.clone(),
            arguments,
            variadic,
        },
        outputs,
    )
}

fn function_definition(kind: ClassKind, class_name: &str, function: &Function, is_method: bool) -> NodeDefinition {
    let has_callback = function.parameters.iter().any(|parameter| is_function_type(parameter) && parameter.default.is_none());
    let is_pure = is_pure_function(kind, class_name, function) && !has_callback;
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    if !is_pure {
        inputs.push(PinDefinition::exec(EXEC_INPUT, ""));
        outputs.push(PinDefinition::exec(EXEC_OUTPUT, ""));
    }
    if is_method {
        inputs.push(
            PinDefinition::data(SELF_PIN, "Target", TypeExpr::named(class_name))
                .with_description(format!("The {class_name} to call {} on", function.name)),
        );
    }
    let mut callbacks = Vec::new();
    let mut callback_outputs = Vec::new();
    for (position, parameter) in function.parameters.iter().enumerate() {
        if !is_function_type(parameter) {
            inputs.push(parameter_pin(parameter));
        } else if parameter.default.is_none() {
            let (callback, pins) = callback_pins(parameter, position);
            callbacks.push(callback);
            callback_outputs.extend(pins);
        }
    }
    outputs.extend(callback_outputs);
    let returns = return_pins(&function.returns);
    outputs.extend(returns.into_iter().filter(|pin| outputs.iter().all(|existing| existing.name != pin.name)).collect::<Vec<_>>());

    let (id_prefix, binding) = if is_method {
        ("call", BindingKind::Method)
    } else {
        ("static", BindingKind::StaticFunction)
    };
    let subtitle = if is_method {
        format!("Target is {class_name}")
    } else {
        class_name.to_owned()
    };
    NodeDefinition {
        id: format!("{id_prefix}/{class_name}/{}", function.name),
        title: humanize(&function.name),
        subtitle: Some(subtitle),
        category: class_name.to_owned(),
        kind: if is_pure { NodeKind::PureFunction } else { NodeKind::Function },
        side: Side::from_authority(function.authority),
        description: description(&function.description),
        inputs,
        outputs,
        keywords: vec![function.name.clone(), class_name.to_owned()],
        api: Some(ApiBinding {
            kind: binding,
            class: class_name.to_owned(),
            member: function.name.clone(),
        }),
        context_warnings: Vec::new(),
        callbacks,
        dynamic: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog() -> NodeCatalog {
        NodeCatalog::from_database(&ApiDatabase::bundled(ApiChannel::Stable).unwrap())
    }

    #[test]
    fn contains_the_starter_template_nodes() {
        let catalog = catalog();
        for id in ["event/Package/Load", "event/Package/Unload", "event/Player/Ready"] {
            assert!(catalog.get(id).is_some(), "missing {id}");
        }
    }

    #[test]
    fn inherited_events_are_typed_for_the_concrete_class() {
        let catalog = catalog();
        let death = catalog.get("event/Character/Death").unwrap();
        let self_pin = death.output("self").unwrap();
        assert_eq!(self_pin.pin_type, crate::PinType::named("Character"));
    }

    #[test]
    fn methods_are_declared_once_on_their_owner() {
        let catalog = catalog();
        assert!(catalog.get("call/Actor/SetLocation").is_some());
        assert!(catalog.get("call/Character/SetLocation").is_none());
        let set_health = catalog.get("call/Damageable/SetHealth").unwrap();
        assert_eq!(set_health.side, Side::Server);
        assert_eq!(set_health.kind, NodeKind::Function);
        assert_eq!(catalog.get("call/Damageable/GetHealth").unwrap().kind, NodeKind::PureFunction);
    }

    #[test]
    fn function_parameters_become_callback_outputs() {
        let catalog = catalog();
        let interval = catalog.get("static/Timer/SetInterval").unwrap();
        assert_eq!(interval.kind, NodeKind::Function);
        assert!(interval.input("callback").is_none());
        assert!(interval.output("callback").unwrap().pin_type.is_exec());
        assert_eq!(interval.callbacks[0].position, 0);
        let bind = catalog.get("static/Input/Bind").unwrap();
        assert_eq!(bind.callbacks[0].arguments, ["callback_delta"]);
        let command = catalog.get("static/Console/RegisterCommand").unwrap();
        assert!(command.callbacks[0].variadic);
        let unbind = catalog.get("static/Input/Unbind").unwrap();
        assert!(unbind.callbacks.is_empty() && unbind.input("callback").is_none());
        assert!(catalog.get("static/Events/Subscribe").is_none());
    }

    #[test]
    fn duplicate_names_get_unique_ids() {
        let catalog = catalog();
        let remote_calls: Vec<_> = catalog
            .definitions()
            .iter()
            .filter(|definition| {
                definition.id == "static/Events/CallRemote" || definition.id.starts_with("static/Events/CallRemote@")
            })
            .collect();
        assert_eq!(remote_calls.len(), 2);
        assert_ne!(remote_calls[0].id, remote_calls[1].id);
    }

    #[test]
    fn pin_names_are_unique_per_direction() {
        for definition in catalog().definitions() {
            for pins in [&definition.inputs, &definition.outputs] {
                let mut names: Vec<_> = pins.iter().map(|pin| pin.name.as_str()).collect();
                names.sort_unstable();
                let count = names.len();
                names.dedup();
                assert_eq!(names.len(), count, "duplicate pin on {}", definition.id);
            }
        }
    }
}
