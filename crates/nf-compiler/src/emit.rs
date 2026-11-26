use std::collections::{HashMap, HashSet};

use nf_api::TypeExpr;
use nf_graph::{
    BindingKind, CLASS_SELF, CallbackDefinition, FUNCTION_ENTRY, FUNCTION_RETURN, GraphScope, NodeDefinition, NodeKind, PinDefinition,
    TypeCategory,
};
use nf_project::{EventDirection, FunctionSignature, GraphSummary, Id, NodeDocument, PinValue, VariableDocument};

use crate::helpers;
use crate::lua::{Block, Expr, Statement};
use crate::naming::NameScope;
use crate::symbols::{ClassSymbol, FunctionSymbol, Issue, PackageSymbols, VariableOwner};

pub const STRUCT_FIELDS: [(&str, &[&str]); 5] = [
    ("Vector", &["X", "Y", "Z"]),
    ("Vector2D", &["X", "Y"]),
    ("Rotator", &["Pitch", "Yaw", "Roll"]),
    ("Color", &["R", "G", "B", "A"]),
    ("Quat", &["X", "Y", "Z", "W"]),
];

const SELF_PIN: &str = "self";
pub const WATCH_PROPERTY: &str = "watch";
pub const BREAKPOINT_PROPERTY: &str = "breakpoint";
pub const STEP_FLAG: &str = "NANOSFLOW_STEP";
pub const CALL_STACK: &str = "NANOSFLOW_STACK";

fn insert_exits(block: Block, exit: &Statement) -> Block {
    let mut result = Block::with_capacity(block.len());
    for statement in block {
        let statement = match statement {
            Statement::Return(values) => {
                result.push(exit.clone());
                Statement::Return(values)
            }
            Statement::If { condition, then_block, else_block } => Statement::If {
                condition,
                then_block: insert_exits(then_block, exit),
                else_block: insert_exits(else_block, exit),
            },
            Statement::NumericFor { variable, start, limit, body } => Statement::NumericFor {
                variable,
                start,
                limit,
                body: insert_exits(body, exit),
            },
            Statement::GenericFor { variables, iterator, body } => Statement::GenericFor {
                variables,
                iterator,
                body: insert_exits(body, exit),
            },
            Statement::While { condition, body } => Statement::While {
                condition,
                body: insert_exits(body, exit),
            },
            other => other,
        };
        result.push(statement);
    }
    result
}

fn wrapped_in_frame(entry: Vec<Statement>, body: Block, exit: Statement) -> Block {
    let mut result = entry;
    result.extend(insert_exits(body, &exit));
    if !matches!(result.last(), Some(Statement::Return(_))) {
        result.push(exit);
    }
    result
}
pub const WATCH_PREFIX: &str = "[NanosFlow:";
const CALLBACK_PLACEHOLDER: &str = "\u{0}callback\u{0}";

pub struct GraphSource<'a> {
    pub scope: &'a GraphScope<'a>,
    pub symbols: &'a PackageSymbols,
    pub graph: &'a GraphSummary,
    pub class: Option<&'a ClassSymbol>,
    pub nodes: &'a [NodeDocument],
    pub variables: &'a [VariableDocument],
    pub expanded: &'a HashMap<Id, NodeDefinition>,
    pub origins: &'a HashMap<Id, nf_graph::MacroOrigin>,
    pub step_hooks: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    Same,
    Nested,
    Loop,
    Callback,
}

struct Placement {
    segment: Id,
    path: Vec<u32>,
    order: u32,
}

#[derive(Default)]
struct Plan {
    placements: HashMap<Id, Placement>,
    nested: HashMap<(Id, String), u32>,
    junctions: HashSet<Id>,
    junction_order: Vec<Id>,
    calls: HashMap<Id, Vec<(Id, bool)>>,
    returns: HashSet<Id>,
    hoisted: HashSet<Id>,
    order: u32,
    blocks: u32,
}

#[derive(Default)]
struct RootState {
    plan: Plan,
    bindings: HashMap<(Id, String), Expr>,
    hoisted_names: Vec<String>,
    junction_names: HashMap<Id, String>,
    returning_junctions: HashSet<Id>,
    self_available: bool,
}

#[derive(Default)]
struct StatementState {
    uses: HashMap<(Id, String), usize>,
    cache: HashMap<(Id, String), Expr>,
    evaluating: HashSet<Id>,
}

enum PureValue {
    Outputs(Vec<(String, Expr)>),
    Call(Expr),
}

pub struct Emitter<'a> {
    source: GraphSource<'a>,
    nodes: HashMap<Id, &'a NodeDocument>,
    consumed: HashSet<(Id, String)>,
    file_names: NameScope,
    names: NameScope,
    locals: HashMap<Id, String>,
    issues: Vec<Issue>,
    root: RootState,
    statement: StatementState,
    helpers: Vec<&'static str>,
    state_names: Vec<(Id, String)>,
    loop_depth: usize,
    entered_through: Option<String>,
}

fn struct_fields(name: &str) -> Option<&'static [&'static str]> {
    STRUCT_FIELDS.iter().find(|(struct_name, _)| *struct_name == name).map(|(_, fields)| *fields)
}

fn title_base(definition: &NodeDefinition) -> String {
    definition.title.split(" (").next().unwrap_or_default().to_owned()
}

fn output_name(definition: &NodeDefinition, pin: &PinDefinition) -> String {
    if let Some(class_name) = definition.id.strip_prefix("cast/") {
        return class_name.to_owned();
    }
    if pin.name.starts_with("return_value") {
        if let Some(api) = &definition.api {
            if api.kind == BindingKind::Constructor {
                return api.class.clone();
            }
            return api
                .member
                .strip_prefix("Get")
                .filter(|rest| rest.starts_with(|character: char| character.is_ascii_uppercase()))
                .unwrap_or(&api.member)
                .to_owned();
        }
        if let Some(class_name) = definition.title.strip_prefix("Spawn ") {
            return class_name.to_owned();
        }
        return title_base(definition);
    }
    if !pin.label.is_empty() {
        pin.label.clone()
    } else if pin.name == "result" || pin.name == "value" {
        title_base(definition)
    } else {
        pin.name.clone()
    }
}

fn data_outputs(definition: &NodeDefinition) -> Vec<&PinDefinition> {
    definition.outputs.iter().filter(|pin| !pin.pin_type.is_exec()).collect()
}

fn data_inputs(definition: &NodeDefinition) -> impl Iterator<Item = &PinDefinition> {
    definition.inputs.iter().filter(|pin| !pin.pin_type.is_exec())
}

fn is_root_kind(definition: &NodeDefinition) -> bool {
    matches!(definition.kind, NodeKind::Event | NodeKind::FunctionEntry)
}

fn binary_operator(node_type: &str) -> Option<&'static str> {
    Some(match node_type {
        "logic/and" => "and",
        "logic/or" => "or",
        "compare/equal" => "==",
        "compare/not-equal" => "~=",
        "compare/less" => "<",
        "compare/less-equal" => "<=",
        "compare/greater" => ">",
        "compare/greater-equal" => ">=",
        "math/add" => "+",
        "math/subtract" => "-",
        "math/multiply" => "*",
        "math/divide" => "/",
        "math/modulo" => "%",
        "math/power" => "^",
        "string/concat" => "..",
        _ => return None,
    })
}

fn metamethod_operator(member: &str) -> Option<&'static str> {
    Some(match member {
        "__add" => "+",
        "__sub" => "-",
        "__mul" => "*",
        "__div" => "/",
        "__idiv" => "//",
        "__mod" => "%",
        "__pow" => "^",
        "__eq" => "==",
        "__lt" => "<",
        "__le" => "<=",
        "__concat" => "..",
        _ => return None,
    })
}

fn parse_builtin_default(text: &str) -> Option<Expr> {
    let text = text.trim();
    match text {
        "nil" => Some(Expr::nil()),
        "true" => Some(Expr::boolean(true)),
        "false" => Some(Expr::boolean(false)),
        _ => {
            if let Ok(value) = text.parse::<f64>() {
                return Some(Expr::number(value));
            }
            let quoted = text.len() >= 2 && text.starts_with('"') && text.ends_with('"');
            quoted.then(|| Expr::string(&text[1..text.len() - 1]))
        }
    }
}

fn push_if(block: &mut Block, condition: Expr, then_block: Block, else_block: Block) {
    match (then_block.is_empty(), else_block.is_empty()) {
        (true, true) => {}
        (true, false) => block.push(Statement::If {
            condition: Expr::not(condition),
            then_block: else_block,
            else_block: Vec::new(),
        }),
        _ => block.push(Statement::If {
            condition,
            then_block,
            else_block,
        }),
    }
}

impl<'a> Emitter<'a> {
    pub fn new(source: GraphSource<'a>, file_names: &NameScope) -> Emitter<'a> {
        let nodes: HashMap<Id, &'a NodeDocument> = source.nodes.iter().map(|node| (node.id, node)).collect();
        let consumed = source
            .nodes
            .iter()
            .flat_map(|node| node.inputs.values())
            .filter_map(|value| match value {
                PinValue::Link(link) => Some((link.node, link.pin.clone())),
                PinValue::Literal { .. } => None,
            })
            .collect();
        let mut file_names = file_names.clone();
        let mut state_names = Vec::new();
        let mut stateful: Vec<&NodeDocument> = source
            .nodes
            .iter()
            .filter(|node| ["flow/do-once", "flow/flip-flop", nf_graph::TIMELINE].contains(&node.node_type.as_str()))
            .collect();
        stateful.sort_by_key(|node| node.id);
        for node in stateful {
            let base = match node.node_type.as_str() {
                "flow/do-once" => "done once",
                "flow/flip-flop" => "flip flop",
                _ => "timeline",
            };
            state_names.push((node.id, file_names.claim(base)));
        }
        let mut emitter = Emitter {
            source,
            nodes,
            consumed,
            names: file_names.clone(),
            file_names,
            locals: HashMap::new(),
            issues: Vec::new(),
            root: RootState::default(),
            statement: StatementState::default(),
            helpers: Vec::new(),
            state_names: Vec::new(),
            entered_through: None,
            loop_depth: 0,
        };
        emitter.state_names = state_names;
        emitter
    }

    pub fn take_state_locals(&mut self) -> Vec<Statement> {
        let per_instance = self.source.class.is_some();
        self.state_names
            .iter()
            .map(|(id, name)| {
                let is_timeline = self.nodes.get(id).is_some_and(|node| node.node_type == nf_graph::TIMELINE);
                Statement::Local {
                    names: vec![name.clone()],
                    values: vec![if per_instance {
                        Expr::call("setmetatable", vec![Expr::table("{}"), Expr::table("{ __mode = \"k\" }")])
                    } else if is_timeline {
                        Expr::table("{}")
                    } else {
                        Expr::boolean(false)
                    }],
                }
            })
            .collect()
    }

    pub fn file_scope(&self) -> NameScope {
        self.file_names.clone()
    }

    fn state_slot(&self, id: Id) -> Option<(Expr, Expr)> {
        let name = self.state_names.iter().find(|(node, _)| *node == id).map(|(_, name)| Expr::name(name))?;
        let key = match self.source.class {
            None => Expr::integer(1),
            Some(_) if self.root.self_available => Expr::name(SELF_PIN),
            Some(class) => Expr::name(&class.name),
        };
        Some((name, key))
    }

    fn state_expression(&self, id: Id) -> Option<Expr> {
        let name = self.state_names.iter().find(|(node, _)| *node == id).map(|(_, name)| Expr::name(name))?;
        let Some(class) = self.source.class else {
            return Some(name);
        };
        let key = if self.root.self_available { Expr::name(SELF_PIN) } else { Expr::name(&class.name) };
        Some(Expr::index(&name, key))
    }

    pub fn take_helpers(&mut self) -> Vec<&'static str> {
        std::mem::take(&mut self.helpers)
    }

    fn helper(&mut self, name: &'static str) -> &'static str {
        if !self.helpers.contains(&name) {
            self.helpers.push(name);
        }
        name
    }

    fn cheap_operand(&mut self, node: &NodeDocument, pin: &str, base: &str, block: &mut Block) -> Expr {
        let value = self.input_or_default(node, pin, block);
        if value.is_cheap() {
            return value;
        }
        let name = self.names.claim(base);
        block.push(Statement::Local {
            names: vec![name.clone()],
            values: vec![value],
        });
        Expr::name(name)
    }

    pub fn into_issues(self) -> Vec<Issue> {
        let mut unique: Vec<Issue> = Vec::new();
        for issue in self.issues {
            if !unique.contains(&issue) {
                unique.push(issue);
            }
        }
        unique
    }

    pub fn declare_file_locals(&mut self) -> Block {
        let variables = self.source.variables;
        let mut block = Block::new();
        for variable in variables {
            let name = self.file_names.claim(&variable.name);
            self.locals.insert(variable.id, name.clone());
            block.push(Statement::Local {
                names: vec![name],
                values: vec![self.variable_default(variable)],
            });
        }
        self.names = self.file_names.clone();
        block
    }

    pub fn event_statements(&mut self) -> Block {
        let mut roots: Vec<&NodeDocument> = self
            .source
            .nodes
            .iter()
            .filter(|node| self.definition(node.id).is_some_and(|definition| definition.kind == NodeKind::Event))
            .collect();
        roots.sort_by(|left, right| {
            left.position[1]
                .total_cmp(&right.position[1])
                .then(left.position[0].total_cmp(&right.position[0]))
                .then(left.id.cmp(&right.id))
        });
        let ids: Vec<Id> = roots.iter().map(|node| node.id).collect();
        ids.into_iter().filter_map(|root| self.emit_event(root)).collect()
    }

    pub fn function_statement(&mut self, lua_name: &str, is_method: bool, signature: &FunctionSignature) -> Statement {
        self.reset_root();
        let entry = self
            .sorted_nodes_of_type(FUNCTION_ENTRY)
            .first()
            .copied();
        let mut parameters = Vec::new();
        for parameter in &signature.parameters {
            let name = self.names.claim(&parameter.name);
            if let Some(entry) = entry {
                self.root.bindings.insert((entry, parameter.id.clone()), Expr::name(&name));
            }
            parameters.push(name);
        }
        if is_method {
            self.root.self_available = true;
            if let Some(entry) = entry {
                self.root.bindings.insert((entry, SELF_PIN.to_owned()), Expr::name(SELF_PIN));
            }
        }

        let mut body = Block::new();
        for variable in self.source.variables {
            let name = self.names.claim(&variable.name);
            self.locals.insert(variable.id, name.clone());
            body.push(Statement::Local {
                names: vec![name],
                values: vec![self.variable_default(variable)],
            });
        }

        if signature.pure {
            let returns = self.sorted_nodes_of_type(FUNCTION_RETURN);
            for extra in returns.iter().skip(1) {
                self.issues.push(Issue::error(
                    "duplicate-return",
                    Some(*extra),
                    "A pure function can only have one Return node, because it has no execution wires to choose between them.",
                    "Delete the extra Return node.",
                ));
            }
            if let Some(&return_node) = returns.first() {
                self.begin_statement(return_node);
                let values = self.return_values(return_node, &mut body);
                body.push(Statement::Return(values));
            }
        } else if let Some(entry) = entry {
            self.emit_root_body(entry, &mut body);
        }
        if let Some(entry) = entry {
            let location = self.location(entry);
            body = self.function_frame(location, body);
        }

        Statement::Function {
            name: lua_name.to_owned(),
            parameters,
            body,
        }
    }

    fn sorted_nodes_of_type(&self, node_type: &str) -> Vec<Id> {
        let mut ids: Vec<Id> = self
            .source
            .nodes
            .iter()
            .filter(|node| node.node_type == node_type && self.definition(node.id).is_some())
            .map(|node| node.id)
            .collect();
        ids.sort();
        ids
    }

    fn reset_root(&mut self) {
        self.names = self.file_names.clone();
        self.root = RootState::default();
        self.statement = StatementState::default();
    }

    fn definition(&self, id: Id) -> Option<&'a NodeDefinition> {
        let expanded: &'a HashMap<Id, NodeDefinition> = self.source.expanded;
        if let Some(definition) = expanded.get(&id) {
            return Some(definition);
        }
        let scope: &'a GraphScope<'a> = self.source.scope;
        let node = self.nodes.get(&id)?;
        scope.get(&node.node_type)
    }

    fn title(&self, id: Id) -> String {
        self.definition(id)
            .map(|definition| definition.title.clone())
            .or_else(|| self.nodes.get(&id).map(|node| node.node_type.clone()))
            .unwrap_or_default()
    }

    fn is_exec(&self, id: Id) -> bool {
        self.definition(id).is_some_and(NodeDefinition::has_exec_input)
    }

    fn is_pure(&self, id: Id) -> bool {
        self.definition(id)
            .is_some_and(|definition| !definition.has_exec_input() && !is_root_kind(definition))
    }

    fn is_root(&self, id: Id) -> bool {
        self.definition(id).is_some_and(is_root_kind)
    }

    fn exec_target(&self, id: Id, pin: &str) -> Option<Id> {
        let link = self.nodes.get(&id)?.exec.get(pin)?;
        let target = self.definition(link.node)?;
        target
            .input(&link.pin)
            .is_some_and(|input| input.pin_type.is_exec())
            .then_some(link.node)
    }

    fn exec_layout(&self, id: Id) -> Vec<(String, Flow)> {
        let (Some(node), Some(definition)) = (self.nodes.get(&id), self.definition(id)) else {
            return Vec::new();
        };
        let fixed = |pins: &[(&str, Flow)]| pins.iter().map(|(pin, flow)| ((*pin).to_owned(), *flow)).collect();
        match node.node_type.as_str() {
            "flow/branch" => fixed(&[("true", Flow::Nested), ("false", Flow::Nested)]),
            "flow/for-loop" | "flow/for-each" | "flow/for-each-pair" | "flow/while" => fixed(&[("loop_body", Flow::Loop), ("completed", Flow::Same)]),
            "flow/delay" => fixed(&[("completed", Flow::Callback)]),
            "flow/do-once" => fixed(&[("completed", Flow::Nested)]),
            nf_graph::TIMELINE => fixed(&[("update", Flow::Callback), ("finished", Flow::Callback)]),
            "flow/flip-flop" => fixed(&[("a", Flow::Nested), ("b", Flow::Nested)]),
            "flow/is-valid" => fixed(&[("valid", Flow::Nested), ("invalid", Flow::Nested)]),
            node_type if node_type.starts_with("flow/switch-") => definition
                .outputs
                .iter()
                .filter(|pin| pin.pin_type.is_exec())
                .map(|pin| (pin.name.clone(), Flow::Nested))
                .collect(),
            node_type if node_type.starts_with("cast/") => fixed(&[("then", Flow::Nested), ("failed", Flow::Nested)]),
            _ => {
                let is_callback = |pin: &PinDefinition| definition.callbacks.iter().any(|callback| callback.exec == pin.name);
                let exec_outputs = definition.outputs.iter().filter(|pin| pin.pin_type.is_exec());
                exec_outputs
                    .clone()
                    .filter(|pin| is_callback(pin))
                    .map(|pin| (pin.name.clone(), Flow::Callback))
                    .chain(exec_outputs.filter(|pin| !is_callback(pin)).map(|pin| (pin.name.clone(), Flow::Same)))
                    .collect()
            }
        }
    }

    fn is_return(&self, id: Id) -> bool {
        self.nodes
            .get(&id)
            .is_some_and(|node| node.node_type == "flow/return" || node.node_type == FUNCTION_RETURN)
    }

    fn output_scope(&self, id: Id, pin: &str) -> Option<String> {
        let node = self.nodes.get(&id)?;
        match node.node_type.as_str() {
            "flow/for-loop" if pin == "index" => Some("loop_body".to_owned()),
            "flow/for-each" if pin == "index" || pin == "element" => Some("loop_body".to_owned()),
            "flow/for-each-pair" if pin == "key" || pin == "value" => Some("loop_body".to_owned()),
            _ => self
                .definition(id)?
                .callbacks
                .iter()
                .find(|callback| callback.arguments.iter().any(|argument| argument == pin))
                .map(|callback| callback.exec.clone()),
        }
    }

    fn callback_blocks(&self, plan: &Plan, id: Id) -> Vec<u32> {
        self.definition(id)
            .map(|definition| {
                definition
                    .callbacks
                    .iter()
                    .filter_map(|callback| plan.nested.get(&(id, callback.exec.clone())).copied())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn static_output(&self, id: Id, pin: &str) -> Option<Expr> {
        let node = self.nodes.get(&id)?;
        if node.node_type == "flow/flip-flop" && pin == "is_a" {
            return self.state_expression(id);
        }
        let variable: Id = node.node_type.strip_prefix("variable/set/")?.parse().ok()?;
        self.locals.get(&variable).map(Expr::name)
    }

    fn visit_exec(&self, id: Id, state: &mut HashMap<Id, bool>, incoming: &mut HashMap<Id, usize>, cycle: &mut Option<Id>) {
        state.insert(id, true);
        for (pin, _) in self.exec_layout(id) {
            let Some(target) = self.exec_target(id, &pin) else {
                continue;
            };
            if self.nodes.get(&target).is_none_or(|node| node.node_type != nf_graph::TIMELINE) {
                *incoming.entry(target).or_insert(0) += 1;
            }
            match state.get(&target) {
                Some(true) => {
                    cycle.get_or_insert(target);
                }
                Some(false) => {}
                None => self.visit_exec(target, state, incoming, cycle),
            }
        }
        state.insert(id, false);
    }

    fn walk(&self, plan: &mut Plan, node: Id, segment: Id, path: &mut Vec<u32>, tail: bool) {
        if plan.junctions.contains(&node) && node != segment {
            plan.calls.entry(segment).or_default().push((node, tail));
            return;
        }
        if self.is_return(node) {
            plan.returns.insert(segment);
        }
        plan.order += 1;
        plan.placements.insert(
            node,
            Placement {
                segment,
                path: path.clone(),
                order: plan.order,
            },
        );
        let layout: Vec<(String, Flow, Option<Id>)> = self
            .exec_layout(node)
            .into_iter()
            .map(|(pin, flow)| {
                let target = self.exec_target(node, &pin);
                (pin, flow, target)
            })
            .collect();
        for (index, (pin, flow, target)) in layout.iter().enumerate() {
            let later_same = layout[index + 1..]
                .iter()
                .any(|(_, flow, target)| *flow == Flow::Same && target.is_some());
            let block = (*flow != Flow::Same).then(|| {
                plan.blocks += 1;
                plan.nested.insert((node, pin.clone()), plan.blocks);
                plan.blocks
            });
            let Some(target) = target else {
                continue;
            };
            let child_tail = match flow {
                Flow::Same | Flow::Nested => tail && !later_same,
                Flow::Loop => false,
                Flow::Callback => true,
            };
            if let Some(block) = block {
                path.push(block);
            }
            self.walk(plan, *target, segment, path, child_tail);
            if block.is_some() {
                path.pop();
            }
        }
    }

    fn collect_dependencies(&self, id: Id, dependencies: &mut Vec<(Id, String)>, visited: &mut HashSet<Id>) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        for value in node.inputs.values() {
            let PinValue::Link(link) = value else {
                continue;
            };
            if self.is_exec(link.node) || self.is_root(link.node) {
                dependencies.push((link.node, link.pin.clone()));
            } else if self.is_pure(link.node) && visited.insert(link.node) {
                self.collect_dependencies(link.node, dependencies, visited);
            }
        }
    }

    fn plan(&mut self, root: Id) -> bool {
        let mut state = HashMap::new();
        let mut incoming = HashMap::new();
        let mut cycle = None;
        self.visit_exec(root, &mut state, &mut incoming, &mut cycle);
        if let Some(target) = cycle {
            self.issues.push(Issue::error(
                "exec-loop",
                Some(target),
                format!("The execution wires loop back to '{}', so this logic would run forever.", self.title(target)),
                "Use a While Loop or For Loop node to repeat logic instead of wiring back.",
            ));
            return false;
        }

        let mut plan = Plan {
            junctions: incoming
                .iter()
                .filter(|(id, count)| **count > 1 && **id != root)
                .map(|(id, _)| *id)
                .collect(),
            ..Plan::default()
        };
        self.walk(&mut plan, root, root, &mut Vec::new(), true);
        let mut junctions: Vec<Id> = plan.junctions.iter().copied().collect();
        junctions.sort_by(|left, right| {
            let position = |id: &Id| self.nodes.get(id).map(|node| node.position).unwrap_or_default();
            position(left)[0]
                .total_cmp(&position(right)[0])
                .then(position(left)[1].total_cmp(&position(right)[1]))
                .then(left.cmp(right))
        });
        for junction in &junctions {
            self.walk(&mut plan, *junction, *junction, &mut Vec::new(), true);
        }

        let mut visited = HashSet::new();
        for junction in &junctions {
            order_junctions(&plan.calls, *junction, &mut visited, &mut plan.junction_order);
        }
        let mut returning = HashSet::new();
        for junction in &plan.junction_order {
            let calls_returning = plan
                .calls
                .get(junction)
                .is_some_and(|calls| calls.iter().any(|(callee, _)| returning.contains(callee)));
            if plan.returns.contains(junction) || calls_returning {
                returning.insert(*junction);
            }
        }
        for (callee, tail) in plan.calls.values().flatten() {
            if returning.contains(callee) && !tail {
                self.issues.push(Issue::error(
                    "merged-return",
                    Some(*callee),
                    format!(
                        "'{}' is reached by several execution wires and leads to a Return, but one of the wires comes from a loop, sequence or delay where more logic follows. That combination cannot be compiled to Lua.",
                        self.title(*callee)
                    ),
                    "Give each path its own Return node, or move the shared part into a function.",
                ));
            }
        }

        let mut consumers: Vec<(Id, u32)> = plan.placements.iter().map(|(id, placement)| (*id, placement.order)).collect();
        consumers.sort_by_key(|(_, order)| *order);
        for (consumer, _) in &consumers {
            let mut dependencies = Vec::new();
            self.collect_dependencies(*consumer, &mut dependencies, &mut HashSet::new());
            let placement = &plan.placements[consumer];
            for (source, pin) in dependencies {
                if self.is_root(source) || self.static_output(source, &pin).is_some() {
                    continue;
                }
                let Some(produced) = plan.placements.get(&source) else {
                    continue;
                };
                let mut produced_path = produced.path.clone();
                let scope = self.output_scope(source, &pin);
                if let Some(scope) = &scope
                    && let Some(block) = plan.nested.get(&(source, scope.clone()))
                {
                    produced_path.push(*block);
                }
                let inside_own_callback = scope.is_none()
                    && self
                        .callback_blocks(&plan, source)
                        .iter()
                        .any(|block| placement.path.get(produced.path.len()) == Some(block));
                let visible = produced.segment == placement.segment
                    && placement.path.starts_with(&produced_path)
                    && produced.order < placement.order
                    && !inside_own_callback;
                if !visible {
                    plan.hoisted.insert(source);
                }
            }
        }

        let mut hoisted: Vec<Id> = plan.hoisted.iter().copied().collect();
        hoisted.sort_by_key(|id| plan.placements.get(id).map(|placement| placement.order));
        for id in hoisted {
            let Some(definition) = self.definition(id) else {
                continue;
            };
            let outputs = data_outputs(definition);
            let Some(last) = outputs
                .iter()
                .rposition(|pin| self.consumed.contains(&(id, pin.name.clone())))
            else {
                continue;
            };
            for pin in &outputs[..=last] {
                let name = self.names.claim(&output_name(definition, pin));
                self.root.bindings.insert((id, pin.name.clone()), Expr::name(&name));
                self.root.hoisted_names.push(name);
            }
        }
        for junction in &plan.junction_order {
            let title = self.definition(*junction).map(title_base).unwrap_or_default();
            let name = self.names.claim(&format!("continue {title}"));
            self.root.junction_names.insert(*junction, name);
        }
        self.root.returning_junctions = returning;
        self.root.plan = plan;
        true
    }

    fn emit_root_body(&mut self, root: Id, body: &mut Block) {
        if !self.plan(root) {
            return;
        }
        if !self.root.hoisted_names.is_empty() {
            body.push(Statement::Local {
                names: self.root.hoisted_names.clone(),
                values: Vec::new(),
            });
        }
        for junction in self.root.plan.junction_order.clone() {
            let name = self.root.junction_names[&junction].clone();
            let mut junction_body = Block::new();
            let depth = std::mem::take(&mut self.loop_depth);
            self.emit_node(junction, &mut junction_body);
            self.loop_depth = depth;
            body.push(Statement::LocalFunction {
                name,
                parameters: Vec::new(),
                body: junction_body,
            });
        }
        self.continue_same(root, body);
    }

    fn emit_event(&mut self, root: Id) -> Option<Statement> {
        let node = *self.nodes.get(&root)?;
        let definition = self.definition(root)?;
        self.reset_root();
        let class_event = node.node_type.strip_prefix("class-event/");
        let prefix = if let Some(event) = class_event {
            format!("{}.Subscribe({}, ", self.source.class?.name, Expr::string(event).text())
        } else if let Some(event) = node.node_type.strip_prefix("custom-event/") {
            let symbols: &'a PackageSymbols = self.source.symbols;
            let event = symbols.events.get(&event.parse::<Id>().ok()?)?;
            let subscribe = match event.direction {
                EventDirection::Local => "Events.Subscribe",
                EventDirection::ToServer | EventDirection::ToClients => "Events.SubscribeRemote",
            };
            format!("{subscribe}({}, ", Expr::string(&event.lua_name).text())
        } else {
            let api = definition.api.as_ref()?;
            format!("{}.Subscribe({}, ", api.class, Expr::string(&api.member).text())
        };
        let outputs = data_outputs(definition);
        let last_used = outputs
            .iter()
            .rposition(|pin| self.consumed.contains(&(root, pin.name.clone())) || (class_event.is_some() && pin.name == SELF_PIN));
        let mut parameters = Vec::new();
        for pin in outputs.iter().take(last_used.map_or(0, |last| last + 1)) {
            let name = if class_event.is_some() && pin.name == SELF_PIN {
                self.root.self_available = true;
                SELF_PIN.to_owned()
            } else {
                self.names.claim(&output_name(definition, pin))
            };
            self.root.bindings.insert((root, pin.name.clone()), Expr::name(&name));
            parameters.push(name);
        }
        if class_event.is_some() && definition.output(SELF_PIN).is_some() {
            self.root.self_available = true;
        }
        let mut body = Block::new();
        self.emit_root_body(root, &mut body);
        if body.is_empty() {
            return None;
        }
        let body = self.handler_frame(self.location(root), body);
        Some(Statement::Callback {
            prefix,
            parameters,
            body,
            suffix: ")".to_owned(),
        })
    }

    fn location(&self, id: Id) -> String {
        match self.source.origins.get(&id) {
            Some(origin) => format!("{}/{}", origin.graph, origin.node),
            None => format!("{}/{id}", self.source.graph.id),
        }
    }

    fn has_flag(&self, id: Id, property: &str) -> bool {
        self.nodes
            .get(&id)
            .and_then(|node| node.properties.get(property))
            .and_then(toml::Value::as_bool)
            .unwrap_or(false)
    }

    fn is_watched(&self, id: Id) -> bool {
        self.has_flag(id, WATCH_PROPERTY)
    }

    fn break_call(&mut self, id: Id, block: &mut Block) -> Option<Statement> {
        let (Some(node), Some(definition)) = (self.nodes.get(&id).copied(), self.definition(id)) else {
            return None;
        };
        let inputs: Vec<PinDefinition> = data_inputs(definition).cloned().collect();
        let mut arguments = vec![Expr::string(&self.location(id))];
        for pin in &inputs {
            let label = if pin.label.is_empty() { pin.name.clone() } else { pin.label.clone() };
            arguments.push(Expr::string(&label));
            arguments.push(self.input_or_default(node, &pin.name, block));
        }
        let helper = self.helper(helpers::DEBUG_BREAK);
        Some(Statement::Expression(Expr::call(helper, arguments)))
    }

    fn push_step_hook(&mut self, id: Id, block: &mut Block) {
        let can_pause = self.source.graph.context != nf_project::ScriptContext::Client;
        let has_exec_input = self.definition(id).is_some_and(NodeDefinition::has_exec_input);
        if !self.source.step_hooks || !can_pause || !has_exec_input {
            return;
        }
        let mut hook = Block::new();
        if let Some(call) = self.break_call(id, &mut hook) {
            hook.push(call);
            block.push(Statement::If {
                condition: Expr::binary(
                    "and",
                    Expr::name(STEP_FLAG),
                    Expr::binary("<=", Expr::unary("#", Expr::name(CALL_STACK)), Expr::name(STEP_FLAG)),
                ),
                then_block: hook,
                else_block: Vec::new(),
            });
        }
    }

    fn tracks_stack(&self) -> bool {
        self.source.step_hooks && self.source.graph.context != nf_project::ScriptContext::Client
    }

    fn function_frame(&mut self, location: String, body: Block) -> Block {
        if !self.tracks_stack() {
            return body;
        }
        let enter = self.helper(helpers::DEBUG_ENTER);
        let leave = self.helper(helpers::DEBUG_LEAVE);
        wrapped_in_frame(
            vec![Statement::Expression(Expr::call(enter, vec![Expr::string(&location)]))],
            body,
            Statement::Expression(Expr::call(leave, Vec::new())),
        )
    }

    fn handler_frame(&mut self, location: String, body: Block) -> Block {
        if !self.tracks_stack() || body.is_empty() {
            return body;
        }
        let outer = self.names.claim("nanosflow outer");
        wrapped_in_frame(
            vec![
                Statement::Local {
                    names: vec![outer.clone()],
                    values: vec![Expr::name(CALL_STACK)],
                },
                Statement::Assign {
                    targets: vec![CALL_STACK.to_owned()],
                    values: vec![Expr::table(format!("{{ {} }}", Expr::string(&location).text()))],
                },
            ],
            body,
            Statement::Assign {
                targets: vec![CALL_STACK.to_owned()],
                values: vec![Expr::name(&outer)],
            },
        )
    }

    fn push_breakpoint(&mut self, id: Id, block: &mut Block) {
        let Some(call) = self.break_call(id, block) else {
            return;
        };
        block.push(call);
        let (code, message, hint) = if self.source.graph.context == nf_project::ScriptContext::Client {
            (
                "breakpoint-client",
                format!("'{}' has a breakpoint, but client code cannot pause. NanosFlow only shows the values when it is reached.", self.title(id)),
                "Remove the breakpoint before you release the package. Pausing works for server and shared code running on the server.",
            )
        } else {
            (
                "breakpoint-node",
                format!("'{}' has a breakpoint. While NanosFlow is open, the whole server pauses when it is reached.", self.title(id)),
                "Remove the breakpoint before you release the package. While a package has breakpoints, every server node also gets a small check for stepping.",
            )
        };
        self.issues.push(Issue::warning(code, Some(id), message, hint));
    }

    fn watches_at_continuation(&self, id: Id) -> bool {
        let layout = self.exec_layout(id);
        !layout.is_empty() && layout.iter().all(|(_, flow)| *flow == Flow::Same)
    }

    fn push_watch(&mut self, id: Id, with_outputs: bool, block: &mut Block) {
        let (Some(node), Some(definition)) = (self.nodes.get(&id).copied(), self.definition(id)) else {
            return;
        };
        let label = |pin: &PinDefinition| if pin.label.is_empty() { pin.name.clone() } else { pin.label.clone() };
        let mut arguments = Vec::new();
        let inputs: Vec<PinDefinition> = data_inputs(definition).cloned().collect();
        for pin in &inputs {
            arguments.push(Expr::string(&label(pin)));
            arguments.push(self.input_or_default(node, &pin.name, block));
        }
        if with_outputs {
            for pin in data_outputs(definition) {
                let value = self
                    .static_output(id, &pin.name)
                    .or_else(|| self.root.bindings.get(&(id, pin.name.clone())).cloned());
                if let Some(value) = value {
                    arguments.push(Expr::string(&label(pin)));
                    arguments.push(value);
                }
            }
        }
        let tag = Expr::string(&format!("{WATCH_PREFIX}{}] ", self.location(id)));
        let message = if arguments.is_empty() {
            Expr::binary("..", tag, Expr::string("ran"))
        } else {
            let helper = self.helper(helpers::WATCH_VALUES);
            Expr::binary("..", tag, Expr::call(helper, arguments))
        };
        block.push(Statement::Expression(Expr::call("Console.Log", vec![message])));
        self.issues.push(Issue::warning(
            "watched-node",
            Some(id),
            format!("'{}' is watched and writes its values to the console every time it runs.", self.title(id)),
            "Right-click the node and pick Stop watching before you release the package.",
        ));
    }

    fn continue_same(&mut self, id: Id, block: &mut Block) {
        if self.is_watched(id) && self.watches_at_continuation(id) {
            self.push_watch(id, true, block);
        }
        for (pin, flow) in self.exec_layout(id) {
            if flow == Flow::Same {
                self.emit_output(id, &pin, block);
            }
        }
    }

    fn emit_output(&mut self, id: Id, pin: &str, block: &mut Block) {
        let Some(target) = self.exec_target(id, pin) else {
            return;
        };
        if self.root.plan.junctions.contains(&target) {
            let Some(name) = self.root.junction_names.get(&target) else {
                return;
            };
            let call = Expr::call(name, Vec::new());
            if self.root.returning_junctions.contains(&target) {
                block.push(Statement::Return(vec![call]));
            } else {
                block.push(Statement::Expression(call));
            }
            return;
        }
        self.entered_through = self.nodes.get(&id).and_then(|node| node.exec.get(pin)).map(|link| link.pin.clone());
        self.emit_node(target, block);
    }

    fn nested(&mut self, id: Id, pin: &str) -> Block {
        let mut block = Block::new();
        self.emit_output(id, pin, &mut block);
        block
    }

    fn emit_node(&mut self, id: Id, block: &mut Block) {
        let start = block.len();
        self.emit_node_body(id, block);
        if block.len() > start {
            block.insert(start, Statement::Marker(self.location(id)));
        }
    }

    fn emit_node_body(&mut self, id: Id, block: &mut Block) {
        let (Some(node), Some(definition)) = (self.nodes.get(&id).copied(), self.definition(id)) else {
            return;
        };
        self.begin_statement(id);
        if self.has_flag(id, BREAKPOINT_PROPERTY) {
            self.push_breakpoint(id, block);
        } else {
            self.push_step_hook(id, block);
        }
        if self.is_watched(id) && !self.watches_at_continuation(id) {
            self.push_watch(id, false, block);
        }
        let node_type = node.node_type.as_str();
        match node_type {
            "flow/branch" => {
                let condition = self.input_or_default(node, "condition", block);
                let then_block = self.nested(id, "true");
                let else_block = self.nested(id, "false");
                push_if(block, condition, then_block, else_block);
            }
            "flow/for-loop" => self.emit_for_loop(node, block),
            "flow/for-each" => self.emit_for_each(node, block),
            _ if node_type.starts_with("flow/switch-") => self.emit_switch(node, definition, block),
            "flow/do-once" => {
                let Some(state) = self.state_expression(id) else {
                    return;
                };
                let mut body = vec![Statement::Assign {
                    targets: vec![state.text().to_owned()],
                    values: vec![Expr::boolean(true)],
                }];
                body.extend(self.nested(id, "completed"));
                block.push(Statement::If {
                    condition: Expr::not(state),
                    then_block: body,
                    else_block: Vec::new(),
                });
            }
            "flow/flip-flop" => {
                let Some(state) = self.state_expression(id) else {
                    return;
                };
                block.push(Statement::Assign {
                    targets: vec![state.text().to_owned()],
                    values: vec![Expr::not(state.clone())],
                });
                let a = self.nested(id, "a");
                let b = self.nested(id, "b");
                push_if(block, state, a, b);
            }
            "flow/is-valid" => {
                let object = self.cheap_operand(node, "object", "object", block);
                let condition = Expr::binary(
                    "and",
                    Expr::binary("~=", object.clone(), Expr::nil()),
                    Expr::method_call(&object, "IsValid", Vec::new()),
                );
                let valid = self.nested(id, "valid");
                let invalid = self.nested(id, "invalid");
                push_if(block, condition, valid, invalid);
            }
            "flow/break" => {
                if self.loop_depth == 0 {
                    self.issues.push(Issue::error(
                        "break-outside-loop",
                        Some(id),
                        "'Break Loop' is not inside the body of a loop, so there is nothing to stop.",
                        "Connect it within the Loop Body of a For Loop, For Each or While Loop. Inside a Delay or a callback it cannot reach the loop.",
                    ));
                } else {
                    block.push(Statement::Break);
                }
            }
            "flow/for-each-pair" => self.emit_for_each_pair(node, block),
            _ if node_type.starts_with("struct/set/") => {
                self.emit_struct_set(node, definition, block);
                self.continue_same(id, block);
            }
            _ if node_type.starts_with("array/") || node_type.starts_with("map/") => {
                self.emit_collection_statement(node, definition, block);
                self.continue_same(id, block);
            }
            "flow/while" => self.emit_while(node, block),
            nf_graph::TIMELINE => self.emit_timeline(node, block),
            "flow/delay" => {
                let seconds = self.input_or_default(node, "seconds", block);
                let milliseconds = match seconds.number_value() {
                    Some(value) => Expr::number((value * 1000.0).round()),
                    None => Expr::call("math.floor", vec![Expr::binary("*", seconds, Expr::integer(1000))]),
                };
                let depth = std::mem::take(&mut self.loop_depth);
                let body = self.nested(id, "completed");
                self.loop_depth = depth;
                let body = self.handler_frame(self.location(id), body);
                block.push(Statement::Callback {
                    prefix: "Timer.SetTimeout(".to_owned(),
                    parameters: Vec::new(),
                    body,
                    suffix: format!(", {})", milliseconds.text()),
                });
            }
            "flow/return" => {
                let values = self.input_value(node, "value", block).into_iter().collect();
                block.push(Statement::Return(values));
            }
            FUNCTION_RETURN => {
                let values = self.return_values(id, block);
                block.push(Statement::Return(values));
            }
            "debug/print" => {
                let is_text = self.linked_category(node, "value") == Some(TypeCategory::String);
                let value = self.input_or_default(node, "value", block);
                let message = if value.is_string_literal() || is_text {
                    value
                } else {
                    Expr::call("tostring", vec![value])
                };
                block.push(Statement::Expression(Expr::call("Console.Log", vec![message])));
                self.continue_same(id, block);
            }
            _ if node_type.starts_with("cast/") => self.emit_cast(node, block),
            _ if node_type.starts_with("variable/set/") => self.emit_variable_set(node, definition, block),
            _ if !definition.callbacks.is_empty() => self.emit_callback_call(node, definition, block),
            _ => {
                if let Some(call) = self.call_expression(node, definition, block) {
                    let outputs = data_outputs(definition);
                    self.bind_results(id, definition, &outputs, call, block);
                }
                self.continue_same(id, block);
            }
        }
    }

    fn return_values(&mut self, id: Id, block: &mut Block) -> Vec<Expr> {
        let (Some(node), Some(definition)) = (self.nodes.get(&id).copied(), self.definition(id)) else {
            return Vec::new();
        };
        let mut values: Vec<Option<Expr>> = data_inputs(definition)
            .map(|pin| self.input_value(node, &pin.name, block))
            .collect();
        while values.last().is_some_and(Option::is_none) {
            values.pop();
        }
        values.into_iter().map(|value| value.unwrap_or_else(Expr::nil)).collect()
    }

    fn produced_name(&mut self, id: Id, pin: &PinDefinition, definition: &NodeDefinition) -> (String, bool) {
        if self.root.plan.hoisted.contains(&id)
            && let Some(existing) = self.root.bindings.get(&(id, pin.name.clone()))
        {
            return (existing.text().to_owned(), true);
        }
        let name = self.names.claim(&output_name(definition, pin));
        self.root.bindings.insert((id, pin.name.clone()), Expr::name(&name));
        (name, false)
    }

    fn result_targets(&mut self, id: Id, definition: &NodeDefinition, outputs: &[&PinDefinition]) -> Option<(bool, Vec<String>)> {
        let consumed: Vec<usize> = outputs
            .iter()
            .enumerate()
            .filter(|(_, pin)| self.consumed.contains(&(id, pin.name.clone())))
            .map(|(index, _)| index)
            .collect();
        let &last = consumed.last()?;
        if self.root.plan.hoisted.contains(&id) {
            let targets = outputs[..=last]
                .iter()
                .map(|pin| {
                    self.root
                        .bindings
                        .get(&(id, pin.name.clone()))
                        .map(|expression| expression.text().to_owned())
                        .unwrap_or_else(|| "_".to_owned())
                })
                .collect();
            return Some((false, targets));
        }
        let mut names = Vec::new();
        for (index, pin) in outputs[..=last].iter().enumerate() {
            if consumed.contains(&index) {
                names.push(self.produced_name(id, pin, definition).0);
            } else {
                names.push("_".to_owned());
            }
        }
        Some((true, names))
    }

    fn bind_results(&mut self, id: Id, definition: &NodeDefinition, outputs: &[&PinDefinition], value: Expr, block: &mut Block) {
        match self.result_targets(id, definition, outputs) {
            None => block.push(Statement::Expression(value)),
            Some((true, names)) => block.push(Statement::Local {
                names,
                values: vec![value],
            }),
            Some((false, targets)) => block.push(Statement::Assign {
                targets,
                values: vec![value],
            }),
        }
    }

    fn callback_parameters(&mut self, id: Id, definition: &NodeDefinition, callback: &CallbackDefinition, body: &mut Block) -> Vec<String> {
        let last_used = callback
            .arguments
            .iter()
            .rposition(|argument| self.consumed.contains(&(id, argument.clone())));
        let Some(last_used) = last_used else {
            return Vec::new();
        };
        let mut parameters = Vec::new();
        for (index, argument) in callback.arguments.iter().enumerate().take(last_used + 1) {
            let base = definition
                .output(argument)
                .map(|pin| pin.label.clone())
                .unwrap_or_else(|| argument.clone());
            if callback.variadic && index + 1 == callback.arguments.len() {
                parameters.push("...".to_owned());
                let mut collected = Block::new();
                let name = self.loop_variable(id, argument, &base, &mut collected);
                if collected.is_empty() {
                    body.push(Statement::Local {
                        names: vec![name],
                        values: vec![Expr::name("{ ... }")],
                    });
                } else {
                    for statement in &mut collected {
                        if let Statement::Assign { values, .. } = statement {
                            *values = vec![Expr::name("{ ... }")];
                        }
                    }
                    body.extend(collected);
                }
            } else {
                parameters.push(self.loop_variable(id, argument, &base, body));
            }
        }
        parameters
    }

    fn emit_callback_call(&mut self, node: &NodeDocument, definition: &'a NodeDefinition, block: &mut Block) {
        let Some(api) = definition.api.as_ref() else {
            self.continue_same(node.id, block);
            return;
        };
        let target = match api.kind {
            BindingKind::Method => Some(self.method_target(node, block)),
            _ => None,
        };
        let inputs = data_inputs(definition).filter(|pin| pin.name != SELF_PIN);
        let mut arguments = self.arguments(node, inputs, block);
        let mut callbacks: Vec<&CallbackDefinition> = definition.callbacks.iter().collect();
        callbacks.sort_by_key(|callback| callback.position);
        for (index, callback) in callbacks.iter().enumerate() {
            while arguments.len() < callback.position {
                arguments.push(Expr::nil());
            }
            let value = if index == 0 { Expr::name(CALLBACK_PLACEHOLDER) } else { Expr::nil() };
            arguments.insert(callback.position, value);
        }
        let call = match (api.kind, &target) {
            (BindingKind::Method, Some(target)) => Expr::method_call(target, &api.member, arguments),
            (BindingKind::Constructor, _) => Expr::call(&api.class, arguments),
            _ => Expr::call(Expr::field(&Expr::name(&api.class), &api.member).text(), arguments),
        };
        let outputs: Vec<&PinDefinition> = data_outputs(definition)
            .into_iter()
            .filter(|pin| self.output_scope(node.id, &pin.name).is_none())
            .collect();
        let assignment = match self.result_targets(node.id, definition, &outputs) {
            None => String::new(),
            Some((true, names)) => format!("local {} = ", names.join(", ")),
            Some((false, targets)) => format!("{} = ", targets.join(", ")),
        };
        let (before, after) = call.text().split_once(CALLBACK_PLACEHOLDER).unwrap_or((call.text(), ""));
        let (before, after) = (before.to_owned(), after.to_owned());
        let mut body = Block::new();
        let callback = callbacks[0];
        let parameters = self.callback_parameters(node.id, definition, callback, &mut body);
        let depth = std::mem::take(&mut self.loop_depth);
        self.emit_output(node.id, &callback.exec, &mut body);
        self.loop_depth = depth;
        let body = self.handler_frame(self.location(node.id), body);
        block.push(Statement::Callback {
            prefix: format!("{assignment}{before}"),
            parameters,
            body,
            suffix: after,
        });
        self.continue_same(node.id, block);
    }

    fn emit_timeline(&mut self, node: &NodeDocument, block: &mut Block) {
        let Some((holder, key)) = self.state_slot(node.id) else {
            return;
        };
        let action = match self.entered_through.take().as_deref() {
            Some("play_from_start") => "start",
            Some("stop") => "stop",
            Some("reverse") => "reverse",
            _ => "play",
        };
        let run = self.helper(helpers::TIMELINE_RUN);
        let mut arguments = vec![holder, key, Expr::string(action)];
        if action != "stop" {
            let length = nf_graph::timeline_length(&node.properties);
            let looping = nf_graph::timeline_looping(&node.properties);
            arguments.push(Expr::number(length));
            arguments.push(Expr::boolean(looping));

            let depth = std::mem::take(&mut self.loop_depth);
            let time = self.names.claim("time");
            self.root.bindings.insert((node.id, "time".to_owned()), Expr::name(&time));
            let tracks = nf_graph::timeline_tracks(&node.properties);
            if !tracks.is_empty() {
                let value = self.helper(helpers::TIMELINE_VALUE);
                for track in tracks {
                    let keys: Vec<String> = track
                        .keys
                        .iter()
                        .map(|(at, amount)| format!("{{ {}, {} }}", Expr::number(*at).text(), Expr::number(*amount).text()))
                        .collect();
                    let table = if keys.is_empty() { "{}".to_owned() } else { format!("{{ {} }}", keys.join(", ")) };
                    self.root.bindings.insert(
                        (node.id, format!("{}{}", nf_graph::TRACK_PREFIX, track.id)),
                        Expr::call(value, vec![Expr::table(table), Expr::name(&time)]),
                    );
                }
            }
            let update = self.nested(node.id, "update");
            let finished = self.nested(node.id, "finished");
            self.loop_depth = depth;
            for (output, body, parameters) in [("update", update, vec![time]), ("finished", finished, Vec::new())] {
                if body.is_empty() {
                    arguments.push(Expr::nil());
                    continue;
                }
                let name = self.names.claim(&format!("timeline {output}"));
                let body = self.handler_frame(self.location(node.id), body);
                block.push(Statement::LocalFunction {
                    name: name.clone(),
                    parameters,
                    body,
                });
                arguments.push(Expr::name(&name));
            }
        }
        block.push(Statement::Expression(Expr::call(run, arguments)));
    }

    fn loop_variable(&mut self, id: Id, pin_name: &str, base: &str, prefix: &mut Block) -> String {
        if !self.consumed.contains(&(id, pin_name.to_owned())) {
            return "_".to_owned();
        }
        if self.root.plan.hoisted.contains(&id) {
            let target = self.root.bindings[&(id, pin_name.to_owned())].text().to_owned();
            let variable = self.names.claim(&format!("current {base}"));
            prefix.push(Statement::Assign {
                targets: vec![target],
                values: vec![Expr::name(&variable)],
            });
            return variable;
        }
        let variable = self.names.claim(base);
        self.root.bindings.insert((id, pin_name.to_owned()), Expr::name(&variable));
        variable
    }

    fn loop_body(&mut self, id: Id, body: &mut Block) {
        self.loop_depth += 1;
        self.emit_output(id, "loop_body", body);
        self.loop_depth -= 1;
    }

    fn emit_switch(&mut self, node: &NodeDocument, definition: &NodeDefinition, block: &mut Block) {
        let value = self.cheap_operand(node, "value", "value", block);
        let enumeration = node.node_type.strip_prefix("flow/switch-enum/");
        let cases = nf_graph::switch_cases(&node.properties);
        let mut branches = Vec::new();
        for pin in definition.outputs.iter().filter(|pin| pin.pin_type.is_exec() && pin.name != "default") {
            let Some(key) = pin.name.strip_prefix(nf_graph::CASE_PREFIX) else {
                continue;
            };
            let literal = match enumeration {
                Some(name) => Expr::field(&Expr::name(name), &self.enum_key(name, key)),
                None => match cases.iter().find(|case| case.id == key).map(|case| &case.value) {
                    Some(toml::Value::Integer(number)) => Expr::integer(*number),
                    Some(toml::Value::Float(number)) => Expr::number(*number),
                    Some(toml::Value::String(text)) => Expr::string(text),
                    _ => continue,
                },
            };
            let body = self.nested(node.id, &pin.name);
            branches.push((literal, body));
        }
        let mut chain = self.nested(node.id, "default");
        let has_default = !chain.is_empty();
        for (literal, body) in branches.into_iter().rev() {
            if body.is_empty() && !has_default {
                continue;
            }
            chain = vec![Statement::If {
                condition: Expr::binary("==", value.clone(), literal),
                then_block: body,
                else_block: chain,
            }];
        }
        block.extend(chain);
    }

    fn emit_for_loop(&mut self, node: &NodeDocument, block: &mut Block) {
        let start = self.input_or_default(node, "first_index", block);
        let limit = self.input_or_default(node, "last_index", block);
        let mut body = Block::new();
        let variable = self.loop_variable(node.id, "index", "index", &mut body);
        self.loop_body(node.id, &mut body);
        block.push(Statement::NumericFor {
            variable,
            start,
            limit,
            body,
        });
        self.emit_output(node.id, "completed", block);
    }

    fn emit_for_each(&mut self, node: &NodeDocument, block: &mut Block) {
        let array = self.input_or_default(node, "array", block);
        let mut body = Block::new();
        let index = self.loop_variable(node.id, "index", "index", &mut body);
        let element = self.loop_variable(node.id, "element", "element", &mut body);
        let variables = if element == "_" { vec![index] } else { vec![index, element] };
        self.loop_body(node.id, &mut body);
        block.push(Statement::GenericFor {
            variables,
            iterator: Expr::call("ipairs", vec![array]),
            body,
        });
        self.emit_output(node.id, "completed", block);
    }

    fn emit_struct_set(&mut self, node: &NodeDocument, definition: &NodeDefinition, block: &mut Block) {
        let symbols: &'a PackageSymbols = self.source.symbols;
        let Some(structure) = node
            .node_type
            .trim_start_matches("struct/set/")
            .parse::<Id>()
            .ok()
            .and_then(|id| symbols.structs.get(&id))
        else {
            return;
        };
        let target = self.cheap_operand(node, "value", &structure.name, block);
        for field in &structure.fields {
            if let Some(value) = self.input_value(node, &field.id, block) {
                block.push(Statement::Assign {
                    targets: vec![Expr::field(&target, &field.key).text().to_owned()],
                    values: vec![value],
                });
            }
        }
        if let Some(pin) = definition.output("value")
            && self.consumed.contains(&(node.id, pin.name.clone()))
        {
            self.bind_results(node.id, definition, &[pin], target, block);
        }
    }

    fn emit_for_each_pair(&mut self, node: &NodeDocument, block: &mut Block) {
        let map = self.input_or_default(node, "map", block);
        let mut body = Block::new();
        let key = self.loop_variable(node.id, "key", "key", &mut body);
        let value = self.loop_variable(node.id, "value", "value", &mut body);
        let variables = if value == "_" { vec![key] } else { vec![key, value] };
        self.loop_body(node.id, &mut body);
        block.push(Statement::GenericFor {
            variables,
            iterator: Expr::call("pairs", vec![map]),
            body,
        });
        self.emit_output(node.id, "completed", block);
    }

    fn emit_collection_statement(&mut self, node: &NodeDocument, definition: &NodeDefinition, block: &mut Block) {
        let call = |function: &str, arguments: Vec<Expr>| Statement::Expression(Expr::call(function, arguments));
        match node.node_type.as_str() {
            "array/add" => {
                let array = self.cheap_operand(node, "array", "array", block);
                let element = self.input_or_default(node, "element", block);
                block.push(call("table.insert", vec![array.clone(), element]));
                if let Some(pin) = definition.output("index")
                    && self.consumed.contains(&(node.id, pin.name.clone()))
                {
                    let length = Expr::unary("#", array);
                    self.bind_results(node.id, definition, &[pin], length, block);
                }
            }
            "array/insert" => {
                let array = self.input_or_default(node, "array", block);
                let index = self.input_or_default(node, "index", block);
                let element = self.input_or_default(node, "element", block);
                block.push(call("table.insert", vec![array, index, element]));
            }
            "array/remove-at" => {
                let array = self.input_or_default(node, "array", block);
                let index = self.input_or_default(node, "index", block);
                let outputs = data_outputs(definition);
                self.bind_results(node.id, definition, &outputs, Expr::call("table.remove", vec![array, index]), block);
            }
            "array/remove-element" => {
                let array = self.input_or_default(node, "array", block);
                let element = self.input_or_default(node, "element", block);
                let helper = self.helper(helpers::ARRAY_REMOVE_ELEMENT);
                block.push(call(helper, vec![array, element]));
            }
            "array/clear" => {
                let array = self.input_or_default(node, "array", block);
                let helper = self.helper(helpers::ARRAY_CLEAR);
                block.push(call(helper, vec![array]));
            }
            "array/set" | "map/set" | "map/remove" => {
                let (container, key, value) = if node.node_type == "array/set" {
                    ("array", "index", self.input_or_default(node, "element", block))
                } else if node.node_type == "map/set" {
                    ("map", "key", self.input_or_default(node, "value", block))
                } else {
                    ("map", "key", Expr::nil())
                };
                let target = self.input_or_default(node, container, block);
                let key = self.input_or_default(node, key, block);
                block.push(Statement::Assign {
                    targets: vec![Expr::index(&target, key).text().to_owned()],
                    values: vec![value],
                });
            }
            _ => {}
        }
    }

    fn evaluate_collection(&mut self, node: &NodeDocument, block: &mut Block) -> Option<Expr> {
        Some(match node.node_type.as_str() {
            "array/make" | "map/make" => Expr::table("{}"),
            "array/of" => {
                let mut elements = Vec::new();
                if let Some(definition) = self.definition(node.id) {
                    for pin in data_inputs(definition) {
                        if let Some(value) = self.input_value(node, &pin.name, block) {
                            elements.push(value.text().to_owned());
                        }
                    }
                }
                if elements.is_empty() {
                    Expr::table("{}")
                } else {
                    Expr::table(format!("{{ {} }}", elements.join(", ")))
                }
            }
            "array/get" => {
                let array = self.input_or_default(node, "array", block);
                Expr::index(&array, self.input_or_default(node, "index", block))
            }
            "map/get" => {
                let map = self.input_or_default(node, "map", block);
                Expr::index(&map, self.input_or_default(node, "key", block))
            }
            "map/has" => {
                let map = self.input_or_default(node, "map", block);
                let value = Expr::index(&map, self.input_or_default(node, "key", block));
                Expr::binary("~=", value, Expr::nil())
            }
            "array/length" => Expr::unary("#", self.input_or_default(node, "array", block)),
            "array/is-empty" => Expr::binary("==", Expr::unary("#", self.input_or_default(node, "array", block)), Expr::integer(0)),
            "array/contains" | "array/find" => {
                let array = self.input_or_default(node, "array", block);
                let element = self.input_or_default(node, "element", block);
                let found = Expr::call(self.helper(helpers::ARRAY_FIND), vec![array, element]);
                if node.node_type == "array/contains" {
                    Expr::binary("~=", found, Expr::nil())
                } else {
                    found
                }
            }
            "array/last" => {
                let array = self.cheap_operand(node, "array", "array", block);
                Expr::index(&array, Expr::unary("#", array.clone()))
            }
            "array/random" => {
                let array = self.cheap_operand(node, "array", "array", block);
                Expr::index(&array, Expr::call("math.random", vec![Expr::unary("#", array.clone())]))
            }
            "array/copy" => {
                let array = self.input_or_default(node, "array", block);
                Expr::table(format!("{{ table.unpack({}) }}", array.text()))
            }
            "array/join" => {
                let array = self.input_or_default(node, "array", block);
                let separator = self.input_or_default(node, "separator", block);
                Expr::call("table.concat", vec![array, separator])
            }
            "map/count" => Expr::call(self.helper(helpers::MAP_COUNT), vec![self.input_or_default(node, "map", block)]),
            _ => return None,
        })
    }

    fn emit_while(&mut self, node: &NodeDocument, block: &mut Block) {
        let mut prelude = Block::new();
        let condition = self.input_or_default(node, "condition", &mut prelude);
        let mut body = Block::new();
        self.loop_body(node.id, &mut body);
        if prelude.is_empty() {
            block.push(Statement::While { condition, body });
        } else {
            prelude.push(Statement::If {
                condition: Expr::not(condition),
                then_block: vec![Statement::Break],
                else_block: Vec::new(),
            });
            prelude.extend(body);
            block.push(Statement::While {
                condition: Expr::boolean(true),
                body: prelude,
            });
        }
        self.emit_output(node.id, "completed", block);
    }

    fn emit_cast(&mut self, node: &NodeDocument, block: &mut Block) {
        let class_name = node.node_type.trim_start_matches("cast/").to_owned();
        let Some(object) = self.input_value(node, "object", block) else {
            self.emit_output(node.id, "failed", block);
            return;
        };
        let result_key = (node.id, "result".to_owned());
        let alias = if self.root.plan.hoisted.contains(&node.id) {
            let target = self.root.bindings[&result_key].clone();
            block.push(Statement::Assign {
                targets: vec![target.text().to_owned()],
                values: vec![object],
            });
            target
        } else if object.is_name() {
            self.root.bindings.insert(result_key, object.clone());
            object
        } else {
            let name = self.names.claim(&class_name);
            block.push(Statement::Local {
                names: vec![name.clone()],
                values: vec![object],
            });
            self.root.bindings.insert(result_key, Expr::name(&name));
            Expr::name(name)
        };
        let condition = self.class_check(node.id, &alias, &class_name, false);
        let then_block = self.nested(node.id, "then");
        let else_block = self.nested(node.id, "failed");
        push_if(block, condition, then_block, else_block);
    }

    fn emit_variable_set(&mut self, node: &NodeDocument, definition: &NodeDefinition, block: &mut Block) {
        let variable: Option<Id> = node.node_type.trim_start_matches("variable/set/").parse().ok();
        let symbol = variable.and_then(|variable| self.source.symbols.variables.get(&variable));
        let Some((variable, symbol)) = variable.zip(symbol) else {
            self.continue_same(node.id, block);
            return;
        };
        let value = self.input_or_default(node, "value", block);
        match symbol.owner {
            VariableOwner::Graph(_) => {
                if let Some(local) = self.locals.get(&variable) {
                    block.push(Statement::Assign {
                        targets: vec![local.clone()],
                        values: vec![value],
                    });
                }
            }
            VariableOwner::Class(_) => {
                let target = self.target_or_self(node, block);
                let value = match definition.output("value") {
                    Some(pin) if self.consumed.contains(&(node.id, pin.name.clone())) => {
                        let (name, hoisted) = self.produced_name(node.id, pin, definition);
                        if hoisted {
                            block.push(Statement::Assign {
                                targets: vec![name.clone()],
                                values: vec![value],
                            });
                        } else {
                            block.push(Statement::Local {
                                names: vec![name.clone()],
                                values: vec![value],
                            });
                        }
                        Expr::name(name)
                    }
                    _ => value,
                };
                let mut arguments = vec![Expr::string(&symbol.document.name), value];
                if symbol.document.sync {
                    arguments.push(Expr::boolean(true));
                }
                block.push(Statement::Expression(Expr::method_call(&target, "SetValue", arguments)));
            }
        }
        self.continue_same(node.id, block);
    }

    fn function_symbol(&self, text: &str) -> Option<&'a FunctionSymbol> {
        let symbols: &'a PackageSymbols = self.source.symbols;
        symbols.functions.get(&text.parse::<Id>().ok()?)
    }

    fn call_expression(&mut self, node: &NodeDocument, definition: &'a NodeDefinition, block: &mut Block) -> Option<Expr> {
        let node_type = node.node_type.as_str();
        let arguments_of = |definition: &'a NodeDefinition| data_inputs(definition).filter(|pin| pin.name != SELF_PIN);
        if let Some(rest) = node_type.strip_prefix("method/") {
            let symbol = self.function_symbol(rest)?;
            let target = self.target_or_self(node, block);
            let arguments = self.arguments(node, arguments_of(definition), block);
            return Some(Expr::method_call(&target, &symbol.lua_name, arguments));
        }
        if let Some(rest) = node_type.strip_prefix("function/") {
            let symbol = self.function_symbol(rest)?;
            let arguments = self.arguments(node, arguments_of(definition), block);
            return Some(Expr::call(&symbol.lua_name, arguments));
        }
        if let Some(rest) = node_type.strip_prefix("interface/") {
            let symbols: &'a PackageSymbols = self.source.symbols;
            let (action, ids) = rest.split_once('/')?;
            let (interface_id, function_id) = match ids.split_once('/') {
                Some((interface, function)) => (interface, Some(function)),
                None => (ids, None),
            };
            let interface = symbols.interfaces.get(&interface_id.parse::<Id>().ok()?)?;
            let target = self.input_value(node, SELF_PIN, block).unwrap_or_else(Expr::nil);
            let flag = Expr::string(&nf_graph::interface_flag(&interface.name));
            if action == "implements" {
                let helper = self.helper(helpers::IMPLEMENTS_INTERFACE);
                return Some(Expr::call(helper, vec![target, flag]));
            }
            let (_, method, _) = interface.functions.iter().find(|(id, _, _)| Some(id.as_str()) == function_id)?;
            let mut arguments = vec![target, flag, Expr::string(method)];
            arguments.extend(self.arguments(node, arguments_of(definition), block));
            let helper = self.helper(helpers::INTERFACE_CALL);
            return Some(Expr::call(helper, arguments));
        }
        if let Some(rest) = node_type.strip_prefix("library/") {
            let symbols: &'a PackageSymbols = self.source.symbols;
            let function = symbols.libraries.get(&rest.parse::<Id>().ok()?)?;
            let arguments = self.arguments(node, arguments_of(definition), block);
            return Some(Expr::call(&format!("{}.{}", function.namespace, function.lua_name), arguments));
        }
        if let Some(rest) = node_type.strip_prefix("send-event/") {
            let (event, to_player) = match rest.strip_suffix("/player") {
                Some(event) => (event, true),
                None => (rest, false),
            };
            let symbols: &'a PackageSymbols = self.source.symbols;
            let event = symbols.events.get(&event.parse::<Id>().ok()?)?;
            let function = match (event.direction, to_player) {
                (EventDirection::Local, _) => "Events.Call",
                (EventDirection::ToClients, false) => "Events.BroadcastRemote",
                _ => "Events.CallRemote",
            };
            let mut arguments = vec![Expr::string(&event.lua_name)];
            arguments.extend(self.arguments(node, arguments_of(definition), block));
            return Some(Expr::call(function, arguments));
        }
        if let Some(rest) = node_type.strip_prefix("class/spawn/") {
            let symbols: &'a PackageSymbols = self.source.symbols;
            let class = symbols.classes.get(&rest.parse::<Id>().ok()?)?;
            self.check_class_available(node.id, class);
            let arguments = self.arguments(node, arguments_of(definition), block);
            return Some(Expr::call(&class.name, arguments));
        }
        let api = definition.api.as_ref()?;
        match api.kind {
            BindingKind::Method => {
                let target = self.method_target(node, block);
                let arguments = self.arguments(node, arguments_of(definition), block);
                Some(Expr::method_call(&target, &api.member, arguments))
            }
            BindingKind::StaticFunction => {
                let arguments = self.arguments(node, arguments_of(definition), block);
                Some(Expr::call(Expr::field(&Expr::name(&api.class), &api.member).text(), arguments))
            }
            BindingKind::Constructor => {
                let arguments = self.arguments(node, arguments_of(definition), block);
                Some(Expr::call(&api.class, arguments))
            }
            _ => None,
        }
    }

    fn arguments<'p>(&mut self, node: &NodeDocument, pins: impl Iterator<Item = &'p PinDefinition>, block: &mut Block) -> Vec<Expr> {
        let definition = self.definition(node.id);
        let mut values: Vec<Option<Expr>> = Vec::new();
        for pin in pins {
            let value = match self.input_value(node, &pin.name, block) {
                Some(value) => Some(value),
                None if pin.optional => None,
                None => Some(match definition {
                    Some(definition) => self.default_value(definition, pin),
                    None => Expr::nil(),
                }),
            };
            values.push(value);
        }
        while values.last().is_some_and(Option::is_none) {
            values.pop();
        }
        values.into_iter().map(|value| value.unwrap_or_else(Expr::nil)).collect()
    }

    fn self_expression(&mut self, node: Id) -> Expr {
        if self.root.self_available {
            return Expr::name(SELF_PIN);
        }
        let class_name = self.source.class.map(|class| class.name.clone()).unwrap_or_else(|| "class".to_owned());
        self.issues.push(Issue::error(
            "no-self",
            Some(node),
            format!("'{}' needs a {class_name}, but there is no Self here, because this logic does not run for one specific {class_name}.", self.title(node)),
            format!("Connect a {class_name} to the Target input, or move the logic into an event of the class or into one of its methods."),
        ));
        Expr::nil()
    }

    fn method_target(&mut self, node: &NodeDocument, block: &mut Block) -> Expr {
        if node.node_type.starts_with("dispatcher/") {
            return self.target_or_self(node, block);
        }
        self.input_value(node, SELF_PIN, block).unwrap_or_else(Expr::nil)
    }

    fn target_or_self(&mut self, node: &NodeDocument, block: &mut Block) -> Expr {
        match self.input_value(node, SELF_PIN, block) {
            Some(target) => target,
            None => self.self_expression(node.id),
        }
    }

    fn check_class_available(&mut self, node: Id, class: &ClassSymbol) {
        let context = self.source.graph.context;
        if class.is_available_in(context) {
            return;
        }
        let side = class.context.display_name().to_lowercase();
        self.issues.push(Issue::error(
            "class-unavailable",
            Some(node),
            format!(
                "The class {} only exists on the {side}, but this {} graph {}.",
                class.name,
                context.display_name(),
                if context == nf_project::ScriptContext::Shared { "also runs on the other side" } else { "runs on the other side" }
            ),
            format!("Move {} to Shared, or move this logic into a {side} graph.", class.name),
        ));
    }

    fn class_check(&mut self, node: Id, object: &Expr, class_name: &str, strict: bool) -> Expr {
        let guard = if strict {
            Expr::binary("~=", object.clone(), Expr::nil())
        } else {
            object.clone()
        };
        let symbols: &'a PackageSymbols = self.source.symbols;
        let check = if class_name == "Entity" {
            None
        } else if let Some(class) = symbols.class_named(class_name) {
            self.check_class_available(node, class);
            Some(Expr::method_call(object, "IsA", vec![Expr::name(&class.name)]))
        } else {
            let concrete = self.source.scope.catalog().concrete_classes_of(class_name);
            concrete
                .iter()
                .map(|concrete| Expr::method_call(object, "IsA", vec![Expr::name(*concrete)]))
                .reduce(|left, right| Expr::binary("or", left, right))
        };
        match check {
            Some(check) => Expr::binary("and", guard, check),
            None => guard,
        }
    }

    fn begin_statement(&mut self, id: Id) {
        let mut uses = HashMap::new();
        let mut visited = HashSet::new();
        self.count_uses(id, &mut uses, &mut visited);
        self.statement = StatementState {
            uses,
            ..StatementState::default()
        };
    }

    fn count_uses(&self, id: Id, uses: &mut HashMap<(Id, String), usize>, visited: &mut HashSet<Id>) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        for value in node.inputs.values() {
            let PinValue::Link(link) = value else {
                continue;
            };
            *uses.entry((link.node, link.pin.clone())).or_insert(0) += 1;
            if self.is_pure(link.node) && visited.insert(link.node) {
                self.count_uses(link.node, uses, visited);
            }
        }
    }

    fn uses(&self, id: Id, pin: &str) -> usize {
        self.statement.uses.get(&(id, pin.to_owned())).copied().unwrap_or(0)
    }

    fn value_of(&mut self, source: Id, pin: &str, block: &mut Block) -> Expr {
        let Some(definition) = self.definition(source) else {
            return Expr::nil();
        };
        if definition.has_exec_input() || is_root_kind(definition) {
            if let Some(local) = self.static_output(source, pin) {
                return local;
            }
            return self
                .root
                .bindings
                .get(&(source, pin.to_owned()))
                .cloned()
                .unwrap_or_else(Expr::nil);
        }
        let key = (source, pin.to_owned());
        if let Some(expression) = self.statement.cache.get(&key) {
            return expression.clone();
        }
        if !self.statement.evaluating.insert(source) {
            return Expr::nil();
        }
        let value = self.evaluate_pure(source, definition, block);
        self.statement.evaluating.remove(&source);
        self.store_pure(source, definition, value, block);
        self.statement.cache.get(&key).cloned().unwrap_or_else(Expr::nil)
    }

    fn store_pure(&mut self, source: Id, definition: &NodeDefinition, value: PureValue, block: &mut Block) {
        let outputs = data_outputs(definition);
        match value {
            PureValue::Outputs(list) => {
                for (pin_name, expression) in list {
                    let expression = if self.uses(source, &pin_name) > 1 && !expression.is_cheap() {
                        let base = outputs
                            .iter()
                            .find(|pin| pin.name == pin_name)
                            .map(|pin| output_name(definition, pin))
                            .unwrap_or_else(|| pin_name.clone());
                        let name = self.names.claim(&base);
                        block.push(Statement::Local {
                            names: vec![name.clone()],
                            values: vec![expression],
                        });
                        Expr::name(name)
                    } else {
                        expression
                    };
                    self.statement.cache.insert((source, pin_name), expression);
                }
            }
            PureValue::Call(call) => {
                let used: Vec<usize> = outputs
                    .iter()
                    .enumerate()
                    .filter(|(_, pin)| self.uses(source, &pin.name) > 0)
                    .map(|(index, _)| index)
                    .collect();
                if used == [0] && self.uses(source, &outputs[0].name) == 1 {
                    self.statement.cache.insert((source, outputs[0].name.clone()), call);
                    return;
                }
                let Some(&last) = used.last() else {
                    return;
                };
                let mut names = Vec::new();
                for (index, pin) in outputs[..=last].iter().enumerate() {
                    if used.contains(&index) {
                        let name = self.names.claim(&output_name(definition, pin));
                        self.statement.cache.insert((source, pin.name.clone()), Expr::name(&name));
                        names.push(name);
                    } else {
                        names.push("_".to_owned());
                    }
                }
                block.push(Statement::Local {
                    names,
                    values: vec![call],
                });
            }
        }
    }

    fn operand(&mut self, node: &NodeDocument, pin: &str, block: &mut Block) -> Expr {
        self.input_or_default(node, pin, block)
    }

    fn evaluate_pure(&mut self, id: Id, definition: &'a NodeDefinition, block: &mut Block) -> PureValue {
        let node = self.nodes[&id];
        let node_type = node.node_type.as_str();
        let single = |pin: &str, expression: Expr| PureValue::Outputs(vec![(pin.to_owned(), expression)]);
        if (node_type.starts_with("array/") || node_type.starts_with("map/"))
            && let Some(expression) = self.evaluate_collection(node, block)
        {
            let pin = data_outputs(definition).first().map(|pin| pin.name.clone()).unwrap_or_default();
            return single(&pin, expression);
        }
        if node_type.starts_with("literal/") {
            return single("result", self.input_or_default(node, "value", block));
        }
        if let Some(operator) = binary_operator(node_type) {
            let left = self.operand(node, "a", block);
            let right = self.operand(node, "b", block);
            return single("result", Expr::binary(operator, left, right));
        }
        match node_type {
            "logic/not" => return single("result", Expr::not(self.operand(node, "a", block))),
            "string/to-string" => return single("result", Expr::call("tostring", vec![self.operand(node, "value", block)])),
            "string/to-number" => return single("result", Expr::call("tonumber", vec![self.operand(node, "value", block)])),
            nf_graph::REROUTE => return single("value", self.operand(node, "value", block)),
            CLASS_SELF => return single(SELF_PIN, self.self_expression(id)),
            _ => {}
        }
        if let Some(class_name) = node_type.strip_prefix("is-a/") {
            let check = match self.input_value(node, "object", block) {
                None => Expr::boolean(false),
                Some(object) => {
                    let object = if object.is_cheap() {
                        object
                    } else {
                        let name = self.names.claim("object");
                        block.push(Statement::Local {
                            names: vec![name.clone()],
                            values: vec![object],
                        });
                        Expr::name(name)
                    };
                    self.class_check(id, &object, class_name, true)
                }
            };
            return single("result", check);
        }
        if node_type == "flow/select" {
            let condition = self.operand(node, "condition", block);
            let if_true = self.operand(node, "if_true", block);
            let if_false = self.operand(node, "if_false", block);
            let helper = self.helper(helpers::SELECT_VALUE);
            return single("value", Expr::call(helper, vec![condition, if_true, if_false]));
        }
        if let Some(structure) = node_type.strip_prefix("struct/make/") {
            let symbols: &'a PackageSymbols = self.source.symbols;
            let Some(structure) = structure.parse::<Id>().ok().and_then(|id| symbols.structs.get(&id)) else {
                return PureValue::Outputs(Vec::new());
            };
            let mut entries = Vec::new();
            for field in &structure.fields {
                let value = match self.input_value(node, &field.id, block) {
                    Some(value) => value,
                    None => self.zero_value(&TypeExpr::parse(&field.type_name).unwrap_or_else(|_| TypeExpr::named("any"))),
                };
                if !value.is_nil() {
                    entries.push(format!("{} = {}", field.key, value.text()));
                }
            }
            let table = if entries.is_empty() { "{}".to_owned() } else { format!("{{ {} }}", entries.join(", ")) };
            return single("value", Expr::table(table));
        }
        if let Some(structure) = node_type.strip_prefix("struct/break/") {
            let symbols: &'a PackageSymbols = self.source.symbols;
            let Some(structure) = structure.parse::<Id>().ok().and_then(|id| symbols.structs.get(&id)) else {
                return PureValue::Outputs(Vec::new());
            };
            let total: usize = structure.fields.iter().map(|field| self.uses(id, &field.id)).sum();
            let base = self.operand(node, "value", block);
            let base = if total > 1 && !base.is_cheap() {
                let name = self.names.claim(&structure.name);
                block.push(Statement::Local {
                    names: vec![name.clone()],
                    values: vec![base],
                });
                Expr::name(name)
            } else {
                base
            };
            return PureValue::Outputs(
                structure
                    .fields
                    .iter()
                    .map(|field| (field.id.clone(), Expr::field(&base, &field.key)))
                    .collect(),
            );
        }
        if let Some(variable) = node_type.strip_prefix("variable/get/") {
            return single("value", self.variable_get(node, variable, block));
        }
        if ["function/", "method/", "library/", "interface/", "class/spawn/"].iter().any(|prefix| node_type.starts_with(prefix)) {
            return match self.call_expression(node, definition, block) {
                Some(call) => PureValue::Call(call),
                None => PureValue::Outputs(Vec::new()),
            };
        }
        let Some(api) = &definition.api else {
            return PureValue::Outputs(Vec::new());
        };
        match api.kind {
            BindingKind::Operator => {
                let left = self.operand(node, "a", block);
                let expression = match (api.member.as_str(), metamethod_operator(&api.member)) {
                    ("__unm", _) => Expr::unary("-", left),
                    ("tostring", _) => Expr::call("tostring", vec![left]),
                    (_, Some(operator)) => {
                        let right = self.operand(node, "b", block);
                        Expr::binary(operator, left, right)
                    }
                    _ => Expr::nil(),
                };
                single("result", expression)
            }
            BindingKind::StaticProperty => single("value", Expr::field(&Expr::name(&api.class), &api.member)),
            BindingKind::BreakStruct => {
                let base = self.operand(node, SELF_PIN, block);
                let outputs = data_outputs(definition);
                let total_uses: usize = outputs.iter().map(|pin| self.uses(id, &pin.name)).sum();
                let base = if total_uses > 1 && !base.is_cheap() {
                    let name = self.names.claim(&api.class);
                    block.push(Statement::Local {
                        names: vec![name.clone()],
                        values: vec![base],
                    });
                    Expr::name(name)
                } else {
                    base
                };
                PureValue::Outputs(
                    outputs
                        .iter()
                        .map(|pin| (pin.name.clone(), Expr::field(&base, &pin.name)))
                        .collect(),
                )
            }
            BindingKind::Method | BindingKind::StaticFunction | BindingKind::Constructor => {
                match self.call_expression(node, definition, block) {
                    Some(call) => PureValue::Call(call),
                    None => PureValue::Outputs(Vec::new()),
                }
            }
            BindingKind::Event => PureValue::Outputs(Vec::new()),
        }
    }

    fn variable_get(&mut self, node: &NodeDocument, variable: &str, block: &mut Block) -> Expr {
        let Ok(variable) = variable.parse::<Id>() else {
            return Expr::nil();
        };
        let symbols: &'a PackageSymbols = self.source.symbols;
        let Some(symbol) = symbols.variables.get(&variable) else {
            return Expr::nil();
        };
        match symbol.owner {
            VariableOwner::Graph(_) => self.locals.get(&variable).map(Expr::name).unwrap_or_else(Expr::nil),
            VariableOwner::Class(_) => {
                let target = self.target_or_self(node, block);
                let mut arguments = vec![Expr::string(&symbol.document.name)];
                let fallback = self.variable_default(&symbol.document);
                if !fallback.is_nil() {
                    arguments.push(fallback);
                }
                Expr::method_call(&target, "GetValue", arguments)
            }
        }
    }

    fn linked_category(&self, node: &NodeDocument, pin_name: &str) -> Option<TypeCategory> {
        let PinValue::Link(link) = node.inputs.get(pin_name)? else {
            return None;
        };
        let output = self.definition(link.node)?.output(&link.pin)?;
        match self.resolved_type(link.node, output, 0)? {
            TypeExpr::Named(name) => Some(self.category(&name)),
            _ => None,
        }
    }

    fn resolved_type(&self, node: Id, pin: &PinDefinition, depth: usize) -> Option<TypeExpr> {
        let declared = pin.pin_type.type_expr()?;
        if !nf_graph::contains_type_parameter(declared) {
            return Some(declared.clone());
        }
        Some(nf_graph::substitute(declared, self.binding(node, depth).as_ref()))
    }

    fn binding(&self, node_id: Id, depth: usize) -> Option<TypeExpr> {
        if depth > 32 {
            return None;
        }
        let node = self.nodes.get(&node_id)?;
        let mut pins: Vec<&PinDefinition> = self
            .definition(node_id)?
            .inputs
            .iter()
            .filter(|pin| pin.pin_type.type_expr().is_some_and(nf_graph::contains_type_parameter))
            .collect();
        pins.sort_by_key(|pin| !matches!(pin.pin_type.type_expr(), Some(TypeExpr::Array(_))));
        for pin in pins {
            let Some(PinValue::Link(link)) = node.inputs.get(&pin.name) else {
                continue;
            };
            let Some(output) = self.definition(link.node).and_then(|source| source.output(&link.pin)) else {
                continue;
            };
            let Some(actual) = self.resolved_type(link.node, output, depth + 1) else {
                continue;
            };
            if let Some(binding) = nf_graph::infer_binding(pin.pin_type.type_expr()?, &actual) {
                return Some(binding);
            }
        }
        None
    }

    fn input_value(&mut self, node: &NodeDocument, pin_name: &str, block: &mut Block) -> Option<Expr> {
        let definition = self.definition(node.id)?;
        let pin = definition.input(pin_name)?;
        match node.inputs.get(pin_name)? {
            PinValue::Link(link) => Some(self.value_of(link.node, &link.pin, block)),
            PinValue::Literal { literal } => {
                let type_expr = self.resolved_type(node.id, pin, 0)?;
                Some(self.literal(&type_expr, literal))
            }
        }
    }

    fn input_or_default(&mut self, node: &NodeDocument, pin_name: &str, block: &mut Block) -> Expr {
        if let Some(value) = self.input_value(node, pin_name, block) {
            return value;
        }
        let Some(definition) = self.definition(node.id) else {
            return Expr::nil();
        };
        match definition.input(pin_name) {
            Some(pin) => self.default_value(definition, pin),
            None => Expr::nil(),
        }
    }

    fn default_value(&self, definition: &NodeDefinition, pin: &PinDefinition) -> Expr {
        if definition.api.is_none()
            && let Some(default) = pin.default.as_deref().filter(|default| !default.trim().is_empty())
            && let Some(value) = parse_builtin_default(default)
        {
            return value;
        }
        match pin.pin_type.type_expr() {
            Some(type_expr) => self.zero_value(type_expr),
            None => Expr::nil(),
        }
    }

    fn enum_key(&self, name: &str, key: &str) -> String {
        self.source
            .symbols
            .enum_named(name)
            .and_then(|enumeration| enumeration.values.iter().find(|(id, _)| id == key))
            .map_or_else(|| key.to_owned(), |(_, lua_key)| lua_key.clone())
    }

    fn category(&self, name: &str) -> TypeCategory {
        self.source.scope.types().category(name)
    }

    fn zero_value(&self, type_expr: &TypeExpr) -> Expr {
        if matches!(type_expr, TypeExpr::Array(_)) {
            return Expr::table("{}");
        }
        let TypeExpr::Named(name) = type_expr else {
            return Expr::nil();
        };
        let symbols: &'a PackageSymbols = self.source.symbols;
        if let Some(structure) = symbols.struct_named(name) {
            let entries: Vec<String> = structure
                .fields
                .iter()
                .filter_map(|field| {
                    let field_type = TypeExpr::parse(&field.type_name).ok()?;
                    let nested = matches!(&field_type, TypeExpr::Named(inner) if symbols.struct_named(inner).is_some());
                    let value = if nested { Expr::table("{}") } else { self.zero_value(&field_type) };
                    (!value.is_nil()).then(|| format!("{} = {}", field.key, value.text()))
                })
                .collect();
            return Expr::table(if entries.is_empty() { "{}".to_owned() } else { format!("{{ {} }}", entries.join(", ")) });
        }
        match self.category(name) {
            TypeCategory::Table => Expr::table("{}"),
            TypeCategory::Boolean => Expr::boolean(false),
            TypeCategory::Number | TypeCategory::Integer => Expr::integer(0),
            TypeCategory::String | TypeCategory::AssetPath => Expr::string(""),
            TypeCategory::Struct => match struct_fields(name) {
                Some(fields) => Expr::call(
                    name,
                    fields
                        .iter()
                        .map(|field| Expr::integer(i64::from(name == "Color" && *field == "A")))
                        .collect(),
                ),
                None => Expr::call(name, Vec::new()),
            },
            _ => Expr::nil(),
        }
    }

    fn variable_default(&self, variable: &VariableDocument) -> Expr {
        let type_expr = TypeExpr::parse(&variable.type_name).unwrap_or_else(|_| TypeExpr::named("any"));
        match &variable.default {
            Some(value) => self.literal(&type_expr, value),
            None => self.zero_value(&type_expr),
        }
    }

    fn literal_fits(&self, type_expr: &TypeExpr, value: &toml::Value) -> bool {
        let TypeExpr::Named(name) = type_expr else {
            return false;
        };
        match (self.category(name), value) {
            (TypeCategory::Any, _) => true,
            (TypeCategory::Boolean, toml::Value::Boolean(_)) => true,
            (TypeCategory::Number | TypeCategory::Integer, toml::Value::Integer(_) | toml::Value::Float(_)) => true,
            (TypeCategory::String | TypeCategory::AssetPath, toml::Value::String(_)) => true,
            (TypeCategory::Enum, toml::Value::String(key)) => self.source.scope.enum_has_key(name, key),
            (TypeCategory::Struct, toml::Value::Table(_)) => true,
            _ => false,
        }
    }

    fn literal(&self, type_expr: &TypeExpr, value: &toml::Value) -> Expr {
        match type_expr {
            TypeExpr::Optional(inner) | TypeExpr::Variadic(inner) => self.literal(inner, value),
            TypeExpr::Union(members) => match members.iter().find(|member| self.literal_fits(member, value)) {
                Some(member) => self.literal(member, value),
                None => plain_literal(value),
            },
            TypeExpr::Named(name) => match (self.category(name), value) {
                (TypeCategory::Enum, toml::Value::String(key)) => Expr::field(&Expr::name(name), &self.enum_key(name, key)),
                (TypeCategory::Struct, toml::Value::Table(fields)) => {
                    let Some(names) = struct_fields(name) else {
                        return Expr::call(name, Vec::new());
                    };
                    let values = names
                        .iter()
                        .map(|field| match fields.get(*field) {
                            Some(toml::Value::Integer(value)) => Expr::integer(*value),
                            Some(toml::Value::Float(value)) => Expr::number(*value),
                            _ => Expr::integer(i64::from(name == "Color" && *field == "A")),
                        })
                        .collect();
                    Expr::call(name, values)
                }
                (TypeCategory::Number, toml::Value::Integer(value)) => Expr::integer(*value),
                _ => plain_literal(value),
            },
            TypeExpr::Array(_) => Expr::nil(),
        }
    }
}

fn plain_literal(value: &toml::Value) -> Expr {
    match value {
        toml::Value::Boolean(value) => Expr::boolean(*value),
        toml::Value::Integer(value) => Expr::integer(*value),
        toml::Value::Float(value) => Expr::number(*value),
        toml::Value::String(value) => Expr::string(value),
        _ => Expr::nil(),
    }
}

fn order_junctions(calls: &HashMap<Id, Vec<(Id, bool)>>, junction: Id, visited: &mut HashSet<Id>, order: &mut Vec<Id>) {
    if !visited.insert(junction) {
        return;
    }
    if let Some(callees) = calls.get(&junction) {
        for (callee, _) in callees {
            order_junctions(calls, *callee, visited, order);
        }
    }
    order.push(junction);
}
