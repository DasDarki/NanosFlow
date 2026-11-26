use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

use nf_graph::{DiagnosticSeverity, GraphScope, MacroBody, MacroOrigin, MacroProblem, NodeCatalog, ProjectClass, analyze_graph};
use serde::{Deserialize, Serialize};
use nf_project::{
    GraphDocument, GraphKind, GraphSummary, Id, NodeDocument, PackageManifest, VariableDocument, ProjectError, ScriptContext, scan_package_tree,
};

use crate::emit::{Emitter, GraphSource};
use crate::helpers;
use crate::lua::{Expr, KEYWORDS, STANDARD_GLOBALS, Statement, extract_markers, render_block};
use crate::naming::{NameScope, pascal_case};
use crate::symbols::{
    BuildProblem, ClassSymbol, EnumSymbol, EventSymbol, InterfaceSymbol, FunctionSymbol, LibrarySymbol, StructField, StructSymbol, GraphReference, Issue, PackageSymbols, VariableOwner, VariableSymbol,
};

pub const INDEX_FILE: &str = "Index.lua";
pub const PACKAGE_FILE: &str = "Package.toml";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub line: usize,
    pub graph: Id,
    pub node: Id,
}

pub type SourceMap = BTreeMap<String, Vec<SourceSpan>>;

#[derive(Debug, Clone, Default)]
pub struct CompiledPackage {
    pub files: BTreeMap<String, String>,
    pub graph_files: HashMap<Id, String>,
    pub problems: Vec<BuildProblem>,
    pub source_map: SourceMap,
}

pub fn locate(spans: &[SourceSpan], line: usize) -> Option<SourceSpan> {
    let index = spans.partition_point(|span| span.line <= line);
    index.checked_sub(1).map(|index| spans[index])
}

impl CompiledPackage {
    pub fn has_errors(&self) -> bool {
        self.problems.iter().any(|problem| problem.severity == DiagnosticSeverity::Error)
    }
}

struct Unit<'a> {
    summary: GraphSummary,
    document: GraphDocument,
    scope: GraphScope<'a>,
    nodes: Vec<NodeDocument>,
    variables: Vec<VariableDocument>,
    macro_variables: Vec<VariableDocument>,
    origins: HashMap<Id, MacroOrigin>,
    macro_problems: Vec<MacroProblem>,
    expanded: HashMap<Id, nf_graph::NodeDefinition>,
}

struct Library {
    folder: String,
    namespace: String,
    functions: Vec<nf_graph::LibraryFunction>,
    classes: Vec<nf_graph::LibraryClass>,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum FileGroup {
    Enum,
    Class,
    Function,
    Graph,
}

struct OutputFile {
    context: ScriptContext,
    group: FileGroup,
    depth: usize,
    relative: String,
}

struct PackageCompiler<'a, 'c> {
    catalog: &'c NodeCatalog,
    units: &'a [Unit<'c>],
    symbols: &'a PackageSymbols,
    problems: Vec<BuildProblem>,
    files: BTreeMap<String, String>,
    graph_files: HashMap<Id, String>,
    source_map: SourceMap,
    outputs: Vec<OutputFile>,
    taken_paths: HashSet<String>,
    step_hooks: bool,
    package_folder: String,
    libraries: &'a [Library],
}

pub fn library_namespace(package_folder: &str) -> String {
    pascal_case(package_folder)
}

fn collect_symbols(units: &[Unit<'_>], classes: &[ProjectClass], libraries: &[Library]) -> PackageSymbols {
    let mut symbols = PackageSymbols::default();
    for library in libraries {
        for function in &library.functions {
            symbols.libraries.insert(
                function.graph.id,
                LibrarySymbol {
                    namespace: library.namespace.clone(),
                    lua_name: pascal_case(&function.graph.name),
                },
            );
        }
    }
    let library_classes: Vec<&nf_graph::LibraryClass> = libraries.iter().flat_map(|library| &library.classes).collect();
    for class in classes {
        let (context, interfaces) = match units.iter().find(|unit| unit.summary.id == class.id) {
            Some(unit) => (unit.summary.context, unit.document.manifest.interfaces.clone()),
            None => match library_classes.iter().find(|library_class| library_class.graph.id == class.id) {
                Some(library_class) => (library_class.graph.context, Vec::new()),
                None => continue,
            },
        };
        symbols.classes.insert(
            class.id,
            ClassSymbol {
                id: class.id,
                name: class.name.clone(),
                parent: class.parent.clone(),
                parent_class: classes.iter().find(|other| other.name == class.parent).map(|other| other.id),
                context,
                api_root: class.api_root.clone(),
                interfaces,
            },
        );
    }
    for library_class in &library_classes {
        for method in &library_class.methods {
            symbols.functions.insert(
                method.graph.id,
                FunctionSymbol {
                    lua_name: pascal_case(&method.graph.name),
                    owner: Some(library_class.graph.id),
                    context: method.graph.context,
                    signature: method.signature.clone(),
                },
            );
        }
        for variable in &library_class.variables {
            symbols.variables.insert(
                variable.id,
                VariableSymbol {
                    owner: VariableOwner::Class(library_class.graph.id),
                    document: variable.clone(),
                },
            );
        }
    }
    for unit in units {
        if unit.summary.kind == GraphKind::Function {
            symbols.functions.insert(
                unit.summary.id,
                FunctionSymbol {
                    lua_name: pascal_case(&unit.summary.name),
                    owner: unit.summary.owner,
                    context: unit.summary.context,
                    signature: unit.document.manifest.signature.clone(),
                },
            );
        }
        if unit.summary.kind == GraphKind::Interface {
            symbols.interfaces.insert(
                unit.summary.id,
                InterfaceSymbol {
                    name: unit.summary.name.clone(),
                    functions: unit
                        .document
                        .manifest
                        .functions
                        .iter()
                        .map(|function| (function.id.clone(), pascal_case(&function.name), function.parameters.len()))
                        .collect(),
                },
            );
        }
        if unit.summary.kind == GraphKind::Enum {
            symbols.enums.insert(
                unit.summary.id,
                EnumSymbol {
                    name: unit.summary.name.clone(),
                    values: unit
                        .document
                        .manifest
                        .signature
                        .parameters
                        .iter()
                        .map(|value| (value.id.clone(), pascal_case(&value.name)))
                        .collect(),
                },
            );
        }
        if unit.summary.kind == GraphKind::Struct {
            symbols.structs.insert(
                unit.summary.id,
                StructSymbol {
                    name: unit.summary.name.clone(),
                    fields: unit
                        .document
                        .manifest
                        .signature
                        .parameters
                        .iter()
                        .map(|field| StructField {
                            id: field.id.clone(),
                            key: pascal_case(&field.name),
                            type_name: field.type_name.clone(),
                        })
                        .collect(),
                },
            );
        }
        if unit.summary.kind == GraphKind::Event {
            symbols.events.insert(
                unit.summary.id,
                EventSymbol {
                    lua_name: pascal_case(&unit.summary.name),
                    direction: unit.document.manifest.direction.unwrap_or_default(),
                },
            );
        }
        let owner = if unit.summary.kind == GraphKind::Class {
            VariableOwner::Class(unit.summary.id)
        } else {
            VariableOwner::Graph(unit.summary.id)
        };
        for variable in &unit.document.variables {
            symbols.variables.insert(
                variable.id,
                VariableSymbol {
                    owner,
                    document: variable.clone(),
                },
            );
        }
        for variable in &unit.macro_variables {
            symbols.variables.insert(
                variable.id,
                VariableSymbol {
                    owner: VariableOwner::Graph(unit.summary.id),
                    document: variable.clone(),
                },
            );
        }
    }
    symbols
}

pub fn compile_package(catalog: &NodeCatalog, package_directory: &Path) -> Result<CompiledPackage, ProjectError> {
    let manifest = PackageManifest::read(package_directory)?;
    let tree = scan_package_tree(package_directory)?;
    let mut documents = Vec::with_capacity(tree.graphs.len());
    for summary in &tree.graphs {
        documents.push((summary.clone(), GraphDocument::load(&summary.path)?));
    }
    let macros: HashMap<Id, MacroBody> = documents
        .iter()
        .filter(|(summary, _)| summary.kind == GraphKind::Macro)
        .map(|(summary, document)| {
            (
                summary.id,
                MacroBody {
                    graph: summary.id,
                    name: summary.name.clone(),
                    nodes: document.nodes.clone(),
                    variables: document.variables.clone(),
                },
            )
        })
        .collect();
    let mut units = Vec::with_capacity(documents.len());
    for (summary, document) in documents {
        let scope = GraphScope::new(catalog, nf_graph::build_scope(catalog, package_directory, &summary.path)?);
        let expansion = if summary.kind == GraphKind::Macro {
            nf_graph::MacroExpansion {
                nodes: document.nodes.clone(),
                ..Default::default()
            }
        } else {
            nf_graph::expand_macros(&document.nodes, &macros)
        };
        let macro_definitions: HashMap<String, nf_graph::NodeDefinition> = expansion
            .variables
            .iter()
            .flat_map(|variable| nf_graph::variable_definitions(variable, None))
            .map(|definition| (definition.id.clone(), definition))
            .collect();
        let expanded = expansion
            .nodes
            .iter()
            .filter_map(|node| match macro_definitions.get(&node.node_type) {
                Some(definition) => Some((node.id, definition.clone())),
                None => Some((node.id, nf_graph::expand_definition(scope.get(&node.node_type)?, &node.properties)?)),
            })
            .collect();
        let variables = document.variables.iter().chain(&expansion.variables).cloned().collect();
        units.push(Unit {
            summary,
            document,
            scope,
            nodes: expansion.nodes,
            variables,
            macro_variables: expansion.variables,
            origins: expansion.origins,
            macro_problems: expansion.problems,
            expanded,
        });
    }
    let libraries: Vec<Library> = nf_graph::required_libraries(package_directory)?
        .into_iter()
        .map(|library| Library {
            namespace: library_namespace(&library.folder),
            folder: library.folder,
            functions: library.functions,
            classes: library.classes,
        })
        .collect();
    let package_folder = package_directory
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let classes = nf_graph::project_classes(catalog, package_directory)?;
    let symbols = collect_symbols(&units, &classes, &libraries);

    let mut compiler = PackageCompiler {
        catalog,
        units: &units,
        symbols: &symbols,
        problems: Vec::new(),
        files: BTreeMap::new(),
        graph_files: HashMap::new(),
        source_map: BTreeMap::new(),
        outputs: Vec::new(),
        taken_paths: HashSet::new(),
        step_hooks: units.iter().any(|unit| {
            unit.document
                .nodes
                .iter()
                .any(|node| node.properties.get(crate::emit::BREAKPOINT_PROPERTY).and_then(toml::Value::as_bool) == Some(true))
        }),
        package_folder,
        libraries: &libraries,
    };
    compiler.check_graphs();
    compiler.check_classes();
    compiler.check_functions();
    compiler.check_events();
    compiler.check_structs();
    compiler.check_enums();
    compiler.check_dispatchers();
    compiler.check_interfaces();
    compiler.check_libraries();
    compiler.check_folders(&tree.folders);
    compiler.generate();
    compiler.write_indexes();
    compiler
        .files
        .insert(PACKAGE_FILE.to_owned(), manifest.to_nanos_package_toml());

    compiler.problems.sort_by_key(|problem| problem.severity != DiagnosticSeverity::Error);
    Ok(CompiledPackage {
        files: compiler.files,
        graph_files: compiler.graph_files,
        problems: compiler.problems,
        source_map: compiler.source_map,
    })
}

impl<'a, 'c> PackageCompiler<'a, 'c> {
    fn unit(&self, id: Id) -> Option<&'a Unit<'c>> {
        self.units.iter().find(|unit| unit.summary.id == id)
    }

    fn reference(&self, summary: &GraphSummary) -> GraphReference {
        let owner = summary
            .owner
            .and_then(|owner| self.symbols.classes.get(&owner))
            .map(|class| class.name.as_str());
        GraphReference::of(summary, owner)
    }

    fn report(&mut self, summary: Option<&GraphSummary>, issue: Issue) {
        let graph = summary.map(|summary| self.reference(summary));
        self.problems.push(issue.into_problem(graph));
    }

    fn check_graphs(&mut self) {
        for unit in self.units {
            let reference = self.reference(&unit.summary);
            for error in &unit.document.load_errors {
                let (message, hint) = if error.has_conflict_markers {
                    (
                        format!("The file '{}' still contains version control conflict markers.", error.file),
                        "Resolve the merge conflict in that file, then build again.".to_owned(),
                    )
                } else {
                    (
                        format!("The file '{}' could not be read: {}", error.file, error.message),
                        "Fix the file or delete it. Deleting removes that node or variable.".to_owned(),
                    )
                };
                self.problems.push(BuildProblem {
                    severity: DiagnosticSeverity::Error,
                    code: "damaged-file".to_owned(),
                    graph: Some(reference.clone()),
                    node: None,
                    pin: None,
                    message,
                    hint: Some(hint),
                });
            }
            for problem in &unit.macro_problems {
                self.problems.push(BuildProblem {
                    severity: DiagnosticSeverity::Error,
                    code: "macro-expansion".to_owned(),
                    graph: Some(reference.clone()),
                    node: Some(problem.node),
                    pin: None,
                    message: problem.message.clone(),
                    hint: Some(problem.hint.clone()),
                });
            }
            let diagnostics = analyze_graph(&unit.scope, unit.summary.context, unit.summary.kind, &unit.document.nodes);
            for diagnostic in diagnostics {
                self.problems.push(BuildProblem {
                    severity: diagnostic.severity,
                    code: diagnostic.code.to_owned(),
                    graph: Some(reference.clone()),
                    node: diagnostic.node,
                    pin: diagnostic.pin,
                    message: diagnostic.message,
                    hint: diagnostic.hint,
                });
            }
        }
    }

    fn global_taken_by(&self, name: &str) -> Option<String> {
        if KEYWORDS.contains(&name) || STANDARD_GLOBALS.contains(&name) {
            return Some("Lua itself".to_owned());
        }
        let payload = self.catalog.payload();
        if payload.classes.iter().any(|class| class.name == name) {
            return Some(format!("nanos world's {name} class"));
        }
        if payload.enums.iter().any(|enumeration| enumeration.name == name) {
            return Some(format!("nanos world's {name} enum"));
        }
        None
    }

    fn check_classes(&mut self) {
        let mut classes: Vec<&ClassSymbol> = self.symbols.classes.values().collect();
        classes.sort_by(|left, right| left.name.cmp(&right.name));
        for class in classes {
            let Some(unit) = self.unit(class.id) else {
                continue;
            };
            let summary = &unit.summary;
            let library_owner = self
                .libraries
                .iter()
                .find(|library| library.classes.iter().any(|library_class| library_class.graph.name == class.name))
                .map(|library| format!("the class {} exported by the package '{}'", class.name, library.folder));
            if let Some(owner) = self.global_taken_by(&class.name).or(library_owner) {
                self.report(
                    Some(summary),
                    Issue::error(
                        "class-name-taken",
                        None,
                        format!("The class name '{}' is already used by {owner}, so the class would replace it in Lua.", class.name),
                        format!("Rename the class, for example to 'My{}'.", class.name),
                    ),
                );
            }
            match class.parent_class.and_then(|parent| self.symbols.classes.get(&parent)) {
                Some(parent) => {
                    if class.api_root.is_none() {
                        self.report(
                            Some(summary),
                            Issue::error(
                                "class-loop",
                                None,
                                format!("{} inherits from itself through {}.", class.name, parent.name),
                                "Change the parent of one of the classes so the chain ends at a nanos world class.",
                            ),
                        );
                    } else if !parent.is_available_in(class.context) || parent.context != ScriptContext::Shared && class.context == ScriptContext::Shared {
                        self.report(
                            Some(summary),
                            Issue::error(
                                "parent-unavailable",
                                None,
                                format!(
                                    "{} is a {} class, but its parent {} only exists on the {}.",
                                    class.name,
                                    class.context.display_name(),
                                    parent.name,
                                    parent.context.display_name().to_lowercase()
                                ),
                                format!("Move {} to Shared, or move {} next to it.", parent.name, class.name),
                            ),
                        );
                    }
                }
                None if !self.catalog.is_inheritable(&class.parent) => {
                    let message = if class.parent.is_empty() {
                        format!("{} has no parent class.", class.name)
                    } else {
                        format!("{} cannot inherit from '{}'.", class.name, class.parent)
                    };
                    self.report(
                        Some(summary),
                        Issue::error(
                            "invalid-parent",
                            None,
                            message,
                            "Pick an entity class like Character, Prop or Weapon as parent in the class settings. Base classes like Actor or Pawn cannot be inherited in Lua.",
                        ),
                    );
                }
                None => {}
            }
            if class.context != ScriptContext::Shared {
                let side = class.context.display_name().to_lowercase();
                self.report(
                    Some(summary),
                    Issue::warning(
                        "class-one-side",
                        None,
                        format!(
                            "{} only exists on the {side}. nanos world needs inherited entity classes on both sides, so the other side will not know {} entities.",
                            class.name, class.name
                        ),
                        format!("Move {} into Shared unless it really is {side} only.", class.name),
                    ),
                );
            }
        }
    }

    fn api_method_owner(&self, class: &ClassSymbol, method: &str) -> Option<String> {
        let root = class.api_root.as_deref()?;
        let mut owners = vec![root.to_owned()];
        owners.extend(self.catalog.types().parents_of(root));
        owners
            .into_iter()
            .find(|owner| self.catalog.get(&format!("call/{owner}/{method}")).is_some())
    }

    fn check_functions(&mut self) {
        let mut functions: Vec<(&Id, &FunctionSymbol)> = self.symbols.functions.iter().collect();
        functions.sort_by(|left, right| left.1.lua_name.cmp(&right.1.lua_name).then(left.0.cmp(right.0)));
        for (index, (id, function)) in functions.iter().enumerate() {
            let Some(unit) = self.unit(**id) else {
                continue;
            };
            let summary = &unit.summary;
            match function.owner {
                None => {
                    if let Some(owner) = self
                        .global_taken_by(&function.lua_name)
                        .or_else(|| self.symbols.class_named(&function.lua_name).map(|_| format!("your class {}", function.lua_name)))
                    {
                        self.report(
                            Some(summary),
                            Issue::error(
                                "function-name-taken",
                                None,
                                format!("The function '{}' becomes '{}' in Lua, which is already used by {owner}.", summary.name, function.lua_name),
                                "Rename the function.",
                            ),
                        );
                    }
                    let clash = functions.iter().enumerate().find(|(other_index, (_, other))| {
                        *other_index != index
                            && other.owner.is_none()
                            && other.lua_name == function.lua_name
                            && (other.context == function.context
                                || other.context == ScriptContext::Shared
                                || function.context == ScriptContext::Shared)
                    });
                    if let Some((_, (other_id, _))) = clash
                        && let Some(other) = self.unit(**other_id)
                    {
                        let other_location = self.reference(&other.summary).location;
                        self.report(
                            Some(summary),
                            Issue::error(
                                "function-name-clash",
                                None,
                                format!(
                                    "The function '{}' and the function at {other_location} both become '{}' in Lua and run on the same side.",
                                    summary.name, function.lua_name
                                ),
                                "Rename one of them. Folders do not separate function names in Lua.",
                            ),
                        );
                    }
                }
                Some(owner) => {
                    let Some(class) = self.symbols.classes.get(&owner) else {
                        continue;
                    };
                    if let Some(api_owner) = self.api_method_owner(class, &function.lua_name) {
                        self.report(
                            Some(summary),
                            Issue::error(
                                "method-name-taken",
                                None,
                                format!(
                                    "'{}' is already a built-in function of {api_owner}, so this method would replace it for every {}.",
                                    function.lua_name, class.name
                                ),
                                "Rename the method.",
                            ),
                        );
                    }
                    let clash = functions.iter().enumerate().any(|(other_index, (_, other))| {
                        other_index != index && other.owner == Some(owner) && other.lua_name == function.lua_name
                    });
                    if clash {
                        self.report(
                            Some(summary),
                            Issue::error(
                                "method-name-clash",
                                None,
                                format!("Two methods of {} both become '{}' in Lua.", class.name, function.lua_name),
                                "Rename one of them.",
                            ),
                        );
                    }
                }
            }
        }
    }

    fn check_structs(&mut self) {
        let mut structs: Vec<(&Id, &StructSymbol)> = self.symbols.structs.iter().collect();
        structs.sort_by(|left, right| left.1.name.cmp(&right.1.name).then(left.0.cmp(right.0)));
        for (id, structure) in &structs {
            let Some(unit) = self.unit(**id) else {
                continue;
            };
            let taken = self
                .global_taken_by(&structure.name)
                .or_else(|| self.symbols.class_named(&structure.name).map(|_| format!("your class {}", structure.name)))
                .or_else(|| {
                    structs
                        .iter()
                        .any(|(other, candidate)| other != id && candidate.name.eq_ignore_ascii_case(&structure.name))
                        .then(|| "another struct".to_owned())
                });
            if let Some(owner) = taken {
                self.report(
                    Some(&unit.summary),
                    Issue::error(
                        "type-name-taken",
                        None,
                        format!("The struct name '{}' is already used by {owner}, so pins of this type would be ambiguous.", structure.name),
                        "Rename the struct.",
                    ),
                );
            }
        }
    }

    fn check_enums(&mut self) {
        let mut enums: Vec<(&Id, &EnumSymbol)> = self.symbols.enums.iter().collect();
        enums.sort_by(|left, right| left.1.name.cmp(&right.1.name).then(left.0.cmp(right.0)));
        for (id, enumeration) in &enums {
            let Some(unit) = self.unit(**id) else {
                continue;
            };
            let name = &enumeration.name;
            let taken = self
                .global_taken_by(name)
                .or_else(|| self.symbols.class_named(name).map(|_| format!("your class {name}")))
                .or_else(|| self.symbols.struct_named(name).map(|_| format!("your struct {name}")))
                .or_else(|| {
                    self.symbols
                        .functions
                        .values()
                        .any(|function| function.owner.is_none() && function.lua_name == *name)
                        .then(|| format!("your function {name}"))
                })
                .or_else(|| {
                    enums
                        .iter()
                        .any(|(other, candidate)| other != id && candidate.name.eq_ignore_ascii_case(name))
                        .then(|| "another enum".to_owned())
                });
            if let Some(owner) = taken {
                self.report(
                    Some(&unit.summary),
                    Issue::error(
                        "type-name-taken",
                        None,
                        format!("The enum name '{name}' is already used by {owner}, so pins of this type would be ambiguous."),
                        "Rename the enum.",
                    ),
                );
            }
            if enumeration.values.is_empty() {
                self.report(
                    Some(&unit.summary),
                    Issue::warning("empty-enum", None, format!("The enum '{name}' has no values yet."), "Add at least one value."),
                );
            }
            let clash = enumeration.values.iter().enumerate().find_map(|(index, (_, key))| {
                enumeration.values[..index].iter().any(|(_, other)| other == key).then_some(key)
            });
            if let Some(key) = clash {
                self.report(
                    Some(&unit.summary),
                    Issue::error(
                        "enum-value-clash",
                        None,
                        format!("Two values of the enum '{name}' both become '{key}' in Lua."),
                        "Rename one of them so they differ in more than spaces or '_'.",
                    ),
                );
            }
        }
    }

    fn check_events(&mut self) {
        let mut events: Vec<(&Id, &EventSymbol)> = self.symbols.events.iter().collect();
        events.sort_by(|left, right| left.1.lua_name.cmp(&right.1.lua_name).then(left.0.cmp(right.0)));
        for (id, event) in &events {
            let others: Vec<&Id> = events
                .iter()
                .filter(|(other, candidate)| other != id && candidate.lua_name == event.lua_name)
                .map(|(other, _)| *other)
                .collect();
            let (Some(unit), Some(other)) = (self.unit(**id), others.first().and_then(|other| self.unit(**other))) else {
                continue;
            };
            let other_location = self.reference(&other.summary).location;
            self.report(
                Some(&unit.summary),
                Issue::error(
                    "event-name-clash",
                    None,
                    format!(
                        "The event '{}' and the event at {other_location} are both sent as '{}', so their handlers would receive each other's messages.",
                        unit.summary.name, event.lua_name
                    ),
                    "Rename one of them.",
                ),
            );
        }
    }

    fn check_folders(&mut self, folders: &[nf_project::FolderSummary]) {
        for folder in folders {
            let is_top_level = folder.folders.is_empty() && folder.context != ScriptContext::Shared;
            if is_top_level && folder.name.eq_ignore_ascii_case(ScriptContext::Shared.lua_directory()) {
                self.problems.push(BuildProblem {
                    severity: DiagnosticSeverity::Error,
                    code: "reserved-folder".to_owned(),
                    graph: None,
                    node: None,
                    pin: None,
                    message: format!(
                        "The {} folder 'Shared' would hide the package's real Shared folder when nanos world loads files.",
                        folder.context.display_name()
                    ),
                    hint: Some("Rename the folder.".to_owned()),
                });
            }
        }
    }

    fn file_scope(&self) -> NameScope {
        let payload = self.catalog.payload();
        let mut reserved: Vec<&str> = payload.classes.iter().map(|class| class.name.as_str()).collect();
        reserved.extend(payload.enums.iter().map(|enumeration| enumeration.name.as_str()));
        reserved.extend(self.symbols.classes.values().map(|class| class.name.as_str()));
        reserved.extend(helpers::ALL);
        reserved.extend(
            self.symbols
                .functions
                .values()
                .filter(|function| function.owner.is_none())
                .map(|function| function.lua_name.as_str()),
        );
        NameScope::with_reserved(reserved)
    }

    fn output_path(&mut self, summary: &GraphSummary) -> String {
        let mut segments = vec![summary.context.lua_directory().to_owned()];
        segments.extend(summary.folders.iter().map(|folder| pascal_case(folder)));
        let directory = segments.join("/");
        let base = pascal_case(&summary.name);
        let mut candidate = format!("{directory}/{base}.lua");
        let mut counter = 2;
        while !self.taken_paths.insert(candidate.to_lowercase()) {
            candidate = format!("{directory}/{base}{counter}.lua");
            counter += 1;
        }
        candidate
    }

    fn uses_project_type(&self, type_name: &str) -> Option<String> {
        fn names(type_expr: &nf_api::TypeExpr, found: &mut Vec<String>) {
            match type_expr {
                nf_api::TypeExpr::Named(name) => found.push(name.clone()),
                nf_api::TypeExpr::Array(inner) | nf_api::TypeExpr::Optional(inner) | nf_api::TypeExpr::Variadic(inner) => names(inner, found),
                nf_api::TypeExpr::Union(members) => members.iter().for_each(|member| names(member, found)),
            }
        }
        let mut found = Vec::new();
        names(&nf_api::TypeExpr::parse(type_name).ok()?, &mut found);
        found
            .into_iter()
            .find(|name| self.symbols.class_named(name).is_some() || self.symbols.struct_named(name).is_some() || self.symbols.enum_named(name).is_some())
    }

    fn own_global(&self, name: &str) -> Option<String> {
        if let Some(class) = self.symbols.class_named(name) {
            return Some(format!("your class {}", class.name));
        }
        if self.symbols.struct_named(name).is_some() {
            return Some(format!("your struct {name}"));
        }
        if self.symbols.enum_named(name).is_some() {
            return Some(format!("your enum {name}"));
        }
        self.symbols
            .functions
            .values()
            .find(|function| function.owner.is_none() && function.lua_name == name)
            .map(|function| format!("your function {}", function.lua_name))
    }

    fn exported_functions(&self, side: ScriptContext) -> Vec<&'a FunctionSymbol> {
        let symbols: &'a PackageSymbols = self.symbols;
        let mut functions: Vec<&'a FunctionSymbol> = symbols
            .functions
            .values()
            .filter(|function| function.owner.is_none() && function.signature.exported)
            .filter(|function| function.context == side || function.context == ScriptContext::Shared)
            .collect();
        functions.sort_by(|left, right| left.lua_name.cmp(&right.lua_name));
        functions.dedup_by(|left, right| left.lua_name == right.lua_name);
        functions
    }

    fn check_libraries(&mut self) {
        let namespace = library_namespace(&self.package_folder);
        let mut exported: Vec<(Id, &FunctionSymbol)> = self
            .symbols
            .functions
            .iter()
            .filter(|(_, function)| function.signature.exported)
            .map(|(id, function)| (*id, function))
            .collect();
        exported.sort_by(|left, right| left.1.lua_name.cmp(&right.1.lua_name));
        for (id, function) in &exported {
            let Some(unit) = self.unit(*id) else {
                continue;
            };
            if function.owner.is_some() {
                self.report(
                    Some(&unit.summary),
                    Issue::error(
                        "export-method",
                        None,
                        format!("The method '{}' is marked as exported, but only functions can be exported.", unit.summary.name),
                        "Turn off 'Export to other packages', or move the logic into a function.",
                    ),
                );
                continue;
            }
            let pins = function.signature.parameters.iter().chain(&function.signature.returns);
            if let Some((pin, type_name)) = pins.filter_map(|pin| Some((pin, self.uses_project_type(&pin.type_name)?))).next() {
                self.report(
                    Some(&unit.summary),
                    Issue::error(
                        "export-project-type",
                        None,
                        format!(
                            "The exported function '{}' uses the type {type_name} for '{}'. Other packages do not know the classes, structs and enums of this package.",
                            unit.summary.name, pin.name
                        ),
                        "Use nanos world types, table or any for exported functions, or stop exporting it.",
                    ),
                );
            }
        }
        if let Some((id, _)) = exported.first()
            && let Some(owner) = self.global_taken_by(&namespace).or_else(|| self.own_global(&namespace))
            && let Some(unit) = self.unit(*id)
        {
            self.report(
                Some(&unit.summary),
                Issue::error(
                    "library-name-taken",
                    None,
                    format!("Other packages reach the exported functions of this package as '{namespace}', which is already used by {owner}."),
                    "Rename the package folder or the clashing entry.",
                ),
            );
        }

        let libraries = self.libraries;
        for (index, library) in libraries.iter().enumerate() {
            let problem = if let Some(owner) = self.global_taken_by(&library.namespace).or_else(|| self.own_global(&library.namespace)) {
                Some(format!("The required package '{}' is reached as '{}', which is already used by {owner}.", library.folder, library.namespace))
            } else {
                libraries[..index]
                    .iter()
                    .find(|other| other.namespace == library.namespace)
                    .map(|other| format!("The required packages '{}' and '{}' are both reached as '{}'.", other.folder, library.folder, library.namespace))
            };
            if let Some(message) = problem
                && library.functions.iter().any(|function| self.is_called(function.graph.id))
            {
                self.report(None, Issue::error("library-name-taken", None, message, "Rename one of the package folders or the clashing entry."));
            }
        }
    }

    fn is_called(&self, function: Id) -> bool {
        let node_type = nf_graph::library_node_id(function);
        self.units.iter().any(|unit| unit.nodes.iter().any(|node| node.node_type == node_type))
    }

    fn collect_issues(&mut self, unit: &Unit<'_>, issues: Vec<Issue>) {
        for mut issue in issues {
            if let Some(origin) = issue.node.and_then(|node| unit.origins.get(&node)) {
                issue.node = Some(origin.instance);
            }
            self.report(Some(&unit.summary), issue);
        }
    }

    fn class_depth(&self, class: &ClassSymbol) -> usize {
        let mut depth = 0;
        let mut current = class.parent_class;
        while let Some(parent) = current.and_then(|parent| self.symbols.classes.get(&parent)) {
            depth += 1;
            if depth > 32 {
                break;
            }
            current = parent.parent_class;
        }
        depth
    }

    fn generate(&mut self) {
        for context in [ScriptContext::Server, ScriptContext::Client] {
            self.taken_paths
                .insert(format!("{}/{}", context.lua_directory(), INDEX_FILE).to_lowercase());
        }
        let file_scope = self.file_scope();
        let mut units: Vec<&'a Unit<'c>> = self.units.iter().filter(|unit| unit.summary.owner.is_none()).collect();
        units.sort_by(|left, right| left.summary.path.cmp(&right.summary.path));
        for unit in units {
            let summary = &unit.summary;
            let (block, group, depth) = match summary.kind {
                GraphKind::EventGraph => {
                    let mut emitter = Emitter::new(self.source(unit, None, true), &file_scope);
                    let mut block = emitter.declare_file_locals();
                    let events = emitter.event_statements();
                    let is_empty = events.is_empty();
                    block.extend(events);
                    let used = emitter.take_helpers();
                    let states = emitter.take_state_locals();
                    self.collect_issues(unit, emitter.into_issues());
                    if is_empty {
                        continue;
                    }
                    (with_helpers(&used, states, block), FileGroup::Graph, 0)
                }
                GraphKind::Function => {
                    let Some(symbol) = self.symbols.functions.get(&summary.id) else {
                        continue;
                    };
                    let mut emitter = Emitter::new(self.source(unit, None, true), &file_scope);
                    let statement = emitter.function_statement(&symbol.lua_name, false, &symbol.signature);
                    let used = emitter.take_helpers();
                    let states = emitter.take_state_locals();
                    self.collect_issues(unit, emitter.into_issues());
                    (with_helpers(&used, states, vec![statement]), FileGroup::Function, 0)
                }
                GraphKind::Class => {
                    let Some(class) = self.symbols.classes.get(&summary.id) else {
                        continue;
                    };
                    let block = self.class_block(unit, class, &file_scope);
                    (block, FileGroup::Class, self.class_depth(class))
                }
                GraphKind::Enum => {
                    let Some(enumeration) = self.symbols.enums.get(&summary.id) else {
                        continue;
                    };
                    let fields: Vec<String> = enumeration
                        .values
                        .iter()
                        .enumerate()
                        .map(|(index, (_, key))| format!("\t{key} = {},", index + 1))
                        .collect();
                    let table = if fields.is_empty() { "{}".to_owned() } else { format!("{{\n{}\n}}", fields.join("\n")) };
                    (vec![Statement::Raw(format!("{} = {table}", enumeration.name))], FileGroup::Enum, 0)
                }
                GraphKind::Event | GraphKind::Struct | GraphKind::Macro | GraphKind::Dispatcher | GraphKind::Interface => continue,
            };
            let relative = self.output_path(summary);
            let mut rendered = String::new();
            render_block(&block, 0, &mut rendered);
            let (content, markers) = extract_markers(&rendered);
            let spans: Vec<SourceSpan> = markers
                .into_iter()
                .filter_map(|(line, payload)| {
                    let (graph, node) = payload.split_once('/')?;
                    Some(SourceSpan {
                        line,
                        graph: graph.parse().ok()?,
                        node: node.parse().ok()?,
                    })
                })
                .collect();
            if !spans.is_empty() {
                self.source_map.insert(relative.clone(), spans);
            }
            self.graph_files.insert(summary.id, relative.clone());
            if summary.kind == GraphKind::Class {
                for method in self.units.iter().filter(|method| method.summary.owner == Some(summary.id)) {
                    self.graph_files.insert(method.summary.id, relative.clone());
                }
            }
            self.files.insert(relative.clone(), content);
            self.outputs.push(OutputFile {
                context: summary.context,
                group,
                depth,
                relative,
            });
        }
    }

    fn source(&self, unit: &'a Unit<'c>, class: Option<&'a ClassSymbol>, with_variables: bool) -> GraphSource<'a> {
        GraphSource {
            scope: &unit.scope,
            symbols: self.symbols,
            graph: &unit.summary,
            class,
            nodes: &unit.nodes,
            variables: if with_variables { &unit.variables } else { &unit.macro_variables },
            expanded: &unit.expanded,
            origins: &unit.origins,
            step_hooks: self.step_hooks,
        }
    }

    fn class_block(&mut self, unit: &'a Unit<'c>, class: &'a ClassSymbol, file_scope: &NameScope) -> Vec<Statement> {
        let parent = self
            .symbols
            .classes
            .get(&class.parent_class.unwrap_or(class.id))
            .filter(|parent| parent.id != class.id)
            .map(|parent| parent.name.clone())
            .unwrap_or_else(|| class.parent.clone());
        let mut block = vec![Statement::Assign {
            targets: vec![class.name.clone()],
            values: vec![Expr::call(&format!("{parent}.Inherit"), vec![Expr::string(&class.name)])],
        }];
        let mut used: Vec<&'static str> = Vec::new();
        let mut methods: Vec<&'a Unit<'c>> = self
            .units
            .iter()
            .filter(|method| method.summary.owner == Some(class.id))
            .collect();
        methods.sort_by(|left, right| left.summary.name.cmp(&right.summary.name));
        let mut scope = file_scope.clone();
        let mut states = Vec::new();
        for method in methods {
            let Some(symbol) = self.symbols.functions.get(&method.summary.id) else {
                continue;
            };
            let mut emitter = Emitter::new(self.source(method, Some(class), true), &scope);
            let name = format!("{}:{}", class.name, symbol.lua_name);
            block.push(emitter.function_statement(&name, true, &symbol.signature));
            used.extend(emitter.take_helpers());
            states.extend(emitter.take_state_locals());
            scope = emitter.file_scope();
            self.collect_issues(method, emitter.into_issues());
        }
        for interface in &class.interfaces {
            block.push(Statement::Assign {
                targets: vec![format!("{}.{}", class.name, nf_graph::interface_flag(interface))],
                values: vec![Expr::boolean(true)],
            });
        }
        let dispatchers = self.dispatcher_methods(class);
        if !dispatchers.is_empty() {
            used.push(helpers::LISTENERS_OF);
            block.extend(dispatchers);
        }
        let mut emitter = Emitter::new(self.source(unit, Some(class), false), &scope);
        let macro_locals = emitter.declare_file_locals();
        block.extend(macro_locals);
        block.extend(emitter.event_statements());
        used.extend(emitter.take_helpers());
        states.extend(emitter.take_state_locals());
        self.collect_issues(unit, emitter.into_issues());
        let inherit = block.remove(0);
        let mut statements = vec![inherit];
        statements.extend(states);
        statements.extend(block);
        with_helpers(&used, Vec::new(), statements)
    }

    fn dispatchers_of(&self, class: Id) -> Vec<&'a Unit<'c>> {
        let mut dispatchers: Vec<&'a Unit<'c>> = self
            .units
            .iter()
            .filter(|unit| unit.summary.owner == Some(class) && unit.summary.kind == GraphKind::Dispatcher)
            .collect();
        dispatchers.sort_by(|left, right| left.summary.name.cmp(&right.summary.name));
        dispatchers
    }

    fn dispatcher_methods(&self, class: &ClassSymbol) -> Vec<Statement> {
        let class_name = &class.name;
        self.dispatchers_of(class.id)
            .into_iter()
            .map(|dispatcher| {
                let name = &dispatcher.summary.name;
                let key = Expr::string(&dispatcher.summary.id.to_string()).text().to_owned();
                let member = |action: &str| nf_graph::dispatcher_member(action, name);
                Statement::Raw(format!(
                    "function {class_name}:{bind}(callback)\n\ttable.insert(listeners_of(self, {key}), callback)\n\treturn callback\nend\n\n\
                     function {class_name}:{unbind}(callback)\n\tlocal listeners = listeners_of(self, {key})\n\tfor index = #listeners, 1, -1 do\n\t\tif listeners[index] == callback then\n\t\t\ttable.remove(listeners, index)\n\t\tend\n\tend\nend\n\n\
                     function {class_name}:{unbind_all}()\n\tlocal listeners = listeners_of(self, {key})\n\tfor index = #listeners, 1, -1 do\n\t\tlisteners[index] = nil\n\tend\nend\n\n\
                     function {class_name}:{call}(...)\n\tfor _, callback in ipairs({{ table.unpack(listeners_of(self, {key})) }}) do\n\t\tcallback(...)\n\tend\nend",
                    bind = member("Bind"),
                    unbind = member("Unbind"),
                    unbind_all = member("UnbindAll"),
                    call = member("Call"),
                ))
            })
            .collect()
    }

    fn check_interfaces(&mut self) {
        let mut classes: Vec<&ClassSymbol> = self.symbols.classes.values().collect();
        classes.sort_by(|left, right| left.name.cmp(&right.name));
        for class in classes {
            let Some(unit) = self.unit(class.id) else {
                continue;
            };
            for interface_name in &class.interfaces {
                let Some(interface) = self.symbols.interfaces.values().find(|interface| interface.name == *interface_name) else {
                    self.report(
                        Some(&unit.summary),
                        Issue::error(
                            "unknown-interface",
                            None,
                            format!("{} implements '{interface_name}', but this package has no interface with that name.", class.name),
                            "Remove it from the class settings, or create the interface.",
                        ),
                    );
                    continue;
                };
                let ancestors = self.ancestors(class.id);
                for (_, function, parameter_count) in &interface.functions {
                    let method = self
                        .symbols
                        .functions
                        .values()
                        .find(|candidate| candidate.owner.is_some_and(|owner| ancestors.contains(&owner)) && candidate.lua_name == *function);
                    let message = match method {
                        None => Some(format!("{} implements {interface_name}, but has no method '{function}'.", class.name)),
                        Some(method) if method.signature.parameters.len() != *parameter_count => Some(format!(
                            "The method '{function}' of {} has {} parameters, but {interface_name} expects {parameter_count}.",
                            class.name,
                            method.signature.parameters.len()
                        )),
                        Some(_) => None,
                    };
                    if let Some(message) = message {
                        self.report(
                            Some(&unit.summary),
                            Issue::error(
                                "interface-method-missing",
                                None,
                                message,
                                "Open the class settings and save them again. NanosFlow then adds the missing methods with the right parameters.",
                            ),
                        );
                    }
                }
            }
        }
    }

    fn ancestors(&self, class: Id) -> Vec<Id> {
        let mut chain = Vec::new();
        let mut current = Some(class);
        while let Some(id) = current {
            if chain.contains(&id) {
                break;
            }
            chain.push(id);
            current = self.symbols.classes.get(&id).and_then(|candidate| candidate.parent_class);
        }
        chain
    }

    fn check_dispatchers(&mut self) {
        let mut classes: Vec<&ClassSymbol> = self.symbols.classes.values().collect();
        classes.sort_by(|left, right| left.name.cmp(&right.name));
        for class in classes {
            let methods: Vec<String> = self
                .symbols
                .functions
                .values()
                .filter(|function| function.owner == Some(class.id))
                .map(|function| function.lua_name.clone())
                .collect();
            let dispatchers = self.dispatchers_of(class.id);
            for (index, dispatcher) in dispatchers.iter().enumerate() {
                let name = &dispatcher.summary.name;
                let members: Vec<String> = ["Bind", "Unbind", "UnbindAll", "Call"].iter().map(|action| nf_graph::dispatcher_member(action, name)).collect();
                let clash = methods
                    .iter()
                    .find(|method| members.contains(method))
                    .map(|method| format!("the method {method}"))
                    .or_else(|| {
                        dispatchers[..index]
                            .iter()
                            .find(|other| nf_graph::dispatcher_member("Call", &other.summary.name) == members[3])
                            .map(|other| format!("the event dispatcher {}", other.summary.name))
                    });
                if let Some(owner) = clash {
                    self.report(
                        Some(&dispatcher.summary),
                        Issue::error(
                            "dispatcher-name-clash",
                            None,
                            format!("The event dispatcher '{name}' of {} needs the Lua methods {}, which clash with {owner}.", class.name, members.join(", ")),
                            "Rename the event dispatcher or the method.",
                        ),
                    );
                }
            }
        }
    }

    fn write_indexes(&mut self) {
        for side in [ScriptContext::Server, ScriptContext::Client] {
            let mut entries: Vec<&OutputFile> = self
                .outputs
                .iter()
                .filter(|output| output.context == side || output.context == ScriptContext::Shared)
                .collect();
            if entries.is_empty() {
                continue;
            }
            entries.sort_by(|left, right| {
                left.group
                    .cmp(&right.group)
                    .then(left.depth.cmp(&right.depth))
                    .then((left.context != ScriptContext::Shared).cmp(&(right.context != ScriptContext::Shared)))
                    .then(left.relative.cmp(&right.relative))
            });
            let prefix = format!("{}/", side.lua_directory());
            let mut block: Vec<Statement> = entries
                .iter()
                .map(|output| {
                    let path = output.relative.strip_prefix(&prefix).unwrap_or(&output.relative);
                    Statement::Expression(Expr::call("Package.Require", vec![Expr::string(path)]))
                })
                .collect();
            let exported = self.exported_functions(side);
            if !exported.is_empty() {
                let fields: Vec<String> = exported
                    .iter()
                    .map(|function| format!("\t{} = {},", function.lua_name, function.lua_name))
                    .collect();
                let table = Expr::table(format!("{{\n{}\n}}", fields.join("\n")));
                block.push(Statement::Expression(Expr::call(
                    "Package.Export",
                    vec![Expr::string(&library_namespace(&self.package_folder)), table],
                )));
            }
            let mut content = String::new();
            render_block(&block, 0, &mut content);
            self.files.insert(format!("{prefix}{INDEX_FILE}"), content);
        }
    }
}

fn with_helpers(used: &[&'static str], states: Vec<Statement>, block: Vec<Statement>) -> Vec<Statement> {
    let mut statements: Vec<Statement> = helpers::ALL
        .iter()
        .filter(|name| used.contains(name))
        .map(|name| Statement::Raw(helpers::render(name)))
        .collect();
    statements.extend(states);
    statements.extend(block);
    statements
}
