use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use mlua::Lua;
use nf_api::{ApiChannel, ApiDatabase};
use nf_graph::{DiagnosticSeverity, NodeCatalog};
use nf_project::{
    FunctionPin, FunctionSignature, GraphDocument, GraphKind, Id, NodeDocument, PackageManifest, PackageType, PinLink,
    PinValue, ScriptContext, VariableDocument, apply_graph_changes, apply_variable_changes, context_root, create_class,
    create_dispatcher, create_graph, create_method, update_event, update_function_signature, EventDirection,
};

use crate::{CompiledPackage, compile_package, write_output};

const RUNTIME: &str = r#"
log = {}
timers = {}
subscriptions = {}
Console = { Log = function(message) log[#log + 1] = tostring(message) end }
Timer = {
    SetTimeout = function(callback, milliseconds)
        timers[#timers + 1] = { callback = callback, milliseconds = milliseconds }
        return #timers
    end,
    SetInterval = function(callback, milliseconds)
        timers[#timers + 1] = { callback = callback, milliseconds = milliseconds }
        return #timers
    end,
    ClearInterval = function(id)
        timers[id].cleared = true
    end,
}
commands = {}
Console.RegisterCommand = function(name, callback)
    commands[name] = callback
end
bindings = {}
Input = {
    Bind = function(name, event, callback)
        bindings[name .. ":" .. tostring(event)] = callback
    end,
}
InputEvent = { Pressed = "Pressed", Released = "Released" }
Entity = {}
Entity.__index = function(instance, key)
    local current = rawget(instance, "class")
    while current do
        local value = rawget(current, key)
        if value ~= nil then
            return value
        end
        current = current.parent
    end
    return Entity[key]
end
function Entity:IsA(class)
    local current = self.class
    while current do
        if current == class then
            return true
        end
        current = current.parent
    end
    return false
end
function Entity:IsValid()
    return not self.destroyed
end
function Entity:SetValue(key, value, sync)
    self.values[key] = value
    self.synced[key] = sync == true
end
function Entity:GetValue(key, fallback)
    local value = self.values[key]
    if value == nil then
        return fallback
    end
    return value
end
function fire(key, ...)
    local result
    for _, handler in ipairs(subscriptions[key] or {}) do
        result = handler(...)
    end
    return result
end
function subscribe(key, callback)
    subscriptions[key] = subscriptions[key] or {}
    table.insert(subscriptions[key], callback)
end
sent = {}
local function record(kind, name, ...)
    sent[#sent + 1] = { kind = kind, name = name, arguments = { ... } }
end
Events = {
    Call = function(name, ...) fire("local." .. name, ...) end,
    Subscribe = function(name, callback) subscribe("local." .. name, callback) end,
    SubscribeRemote = function(name, callback) subscribe("remote." .. name, callback) end,
    CallRemote = function(name, ...) record("CallRemote", name, ...) end,
    BroadcastRemote = function(name, ...) record("BroadcastRemote", name, ...) end,
}
function make_class(name, parent)
    local class = { name = name, parent = parent }
    class.Subscribe = function(event, callback)
        local key = name .. "." .. event
        subscriptions[key] = subscriptions[key] or {}
        table.insert(subscriptions[key], callback)
    end
    class.Inherit = function(child)
        return make_class(child, class)
    end
    return setmetatable(class, {
        __call = function(_, ...)
            local instance = setmetatable({ class = class, values = {}, synced = {}, arguments = { ... } }, Entity)
            local current = class
            while current do
                fire(current.name .. ".Spawn", instance)
                current = current.parent
            end
            return instance
        end,
    })
end
Character = make_class("Character")
CharacterSimple = make_class("CharacterSimple")
Prop = make_class("Prop")
Player = make_class("Player")
Package = make_class("Package")
Vector = function(x, y, z) return { X = x, Y = y, Z = z } end
Rotator = function(pitch, yaw, roll) return { Pitch = pitch, Yaw = yaw, Roll = roll } end
"#;

fn catalog() -> &'static NodeCatalog {
    static CATALOG: OnceLock<NodeCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| NodeCatalog::from_database(&ApiDatabase::bundled(ApiChannel::Stable).unwrap()))
}

struct TestPackage {
    _directory: tempfile::TempDir,
    path: PathBuf,
}

impl TestPackage {
    fn new() -> TestPackage {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("game");
        TestPackage::write_manifest(&path, Vec::new());
        TestPackage {
            _directory: directory,
            path,
        }
    }

    fn sibling(&self, folder: &str) -> PathBuf {
        let path = self.path.with_file_name(folder);
        TestPackage::write_manifest(&path, Vec::new());
        path
    }

    fn require(&self, folders: &[&str]) {
        TestPackage::write_manifest(&self.path, folders.iter().map(|folder| (*folder).to_owned()).collect());
    }

    fn write_manifest(path: &Path, packages_requirements: Vec<String>) {
        let id = PackageManifest::read(path).map(|manifest| manifest.id).unwrap_or_else(|_| Id::generate());
        PackageManifest {
            id,
            package_type: PackageType::Script,
            title: path.file_name().unwrap().to_string_lossy().into_owned(),
            author: "Tester".to_owned(),
            version: "1.0.0".to_owned(),
            compatibility_version: "1.144".to_owned(),
            auto_cleanup: false,
            load_level_entities: false,
            packages_requirements,
            assets_requirements: Vec::new(),
            map_asset: None,
        }
        .write(path)
        .unwrap();
    }

    fn root(&self, context: ScriptContext) -> PathBuf {
        let root = context_root(&self.path, context);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn graph(&self, context: ScriptContext, name: &str, kind: GraphKind) -> PathBuf {
        create_graph(&self.root(context), name, kind).unwrap()
    }

    fn compile(&self) -> CompiledPackage {
        let compiled = compile_package(catalog(), &self.path).unwrap();
        for (path, code) in &compiled.files {
            if path.ends_with(".lua") {
                Lua::new()
                    .load(code.as_str())
                    .set_name(path.as_str())
                    .into_function()
                    .unwrap_or_else(|error| panic!("{path} is not valid Lua: {error}\n{code}"));
            }
        }
        compiled
    }
}

#[derive(Default)]
struct Nodes {
    nodes: Vec<NodeDocument>,
}

impl Nodes {
    fn loaded(graph: &Path) -> Nodes {
        Nodes {
            nodes: GraphDocument::load(graph).unwrap().nodes,
        }
    }

    fn add(&mut self, node_type: &str) -> Id {
        let node = NodeDocument::new(node_type, [0.0, self.nodes.len() as f64 * 100.0]);
        let id = node.id;
        self.nodes.push(node);
        id
    }

    fn node(&mut self, id: Id) -> &mut NodeDocument {
        self.nodes.iter_mut().find(|node| node.id == id).unwrap()
    }

    fn then(&mut self, source: Id, pin: &str, target: Id) {
        self.node(source).exec.insert(
            pin.to_owned(),
            PinLink {
                node: target,
                pin: "exec".to_owned(),
            },
        );
    }

    fn wire(&mut self, source: Id, source_pin: &str, target: Id, target_pin: &str) {
        self.node(target).inputs.insert(
            target_pin.to_owned(),
            PinValue::Link(PinLink {
                node: source,
                pin: source_pin.to_owned(),
            }),
        );
    }

    fn literal(&mut self, target: Id, pin: &str, value: impl Into<toml::Value>) {
        self.node(target)
            .inputs
            .insert(pin.to_owned(), PinValue::Literal { literal: value.into() });
    }

    fn print(&mut self, text: &str) -> Id {
        let print = self.add("debug/print");
        self.literal(print, "value", text);
        print
    }

    fn save(&self, graph: &Path) {
        apply_graph_changes(graph, &self.nodes, &[]).unwrap();
    }
}

fn entry_of(graph: &Path) -> Id {
    GraphDocument::load(graph).unwrap().nodes[0].id
}

fn run(compiled: &CompiledPackage, side: &str, script: &str) -> (Lua, Vec<String>) {
    run_packages(&[compiled], side, script)
}

fn run_packages(packages: &[&CompiledPackage], side: &str, script: &str) -> (Lua, Vec<String>) {
    let lua = Lua::new();
    lua.load(RUNTIME).exec().unwrap();
    lua.load("Package.Export = function(name, value) _G[name] = value end").exec().unwrap();
    for compiled in packages {
        let files = compiled.files.clone();
        let side_prefix = format!("{side}/");
        let require = lua
            .create_function(move |lua, path: String| {
                let code = files
                    .get(&format!("{side_prefix}{path}"))
                    .or_else(|| files.get(&path))
                    .cloned()
                    .ok_or_else(|| mlua::Error::runtime(format!("Package.Require could not find {path}")))?;
                lua.load(code).set_name(path).exec()
            })
            .unwrap();
        let package: mlua::Table = lua.globals().get("Package").unwrap();
        package.set("Require", require).unwrap();
        let index = &compiled.files[&format!("{side}/Index.lua")];
        lua.load(index.as_str()).exec().unwrap();
    }
    lua.load(script).exec().unwrap();
    let log: Vec<String> = lua.globals().get("log").unwrap();
    (lua, log)
}

fn codes(compiled: &CompiledPackage) -> Vec<&str> {
    compiled.problems.iter().map(|problem| problem.code.as_str()).collect()
}

fn assert_clean(compiled: &CompiledPackage) {
    let errors: Vec<_> = compiled
        .problems
        .iter()
        .filter(|problem| problem.severity == DiagnosticSeverity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");
}

#[test]
fn compiles_branches_loops_and_prints() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let branch = nodes.add("flow/branch");
    nodes.literal(branch, "condition", true);
    let yes = nodes.print("yes");
    let no = nodes.print("no");
    let for_loop = nodes.add("flow/for-loop");
    nodes.literal(for_loop, "last_index", 3);
    let print_index = nodes.add("debug/print");
    nodes.wire(for_loop, "index", print_index, "value");
    let done = nodes.print("done");
    nodes.then(load, "then", branch);
    nodes.then(branch, "true", yes);
    nodes.then(branch, "false", no);
    nodes.then(yes, "then", for_loop);
    nodes.then(for_loop, "loop_body", print_index);
    nodes.then(for_loop, "completed", done);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    let code = &compiled.files["Server/Main.lua"];
    assert_eq!(
        code,
        "Package.Subscribe(\"Load\", function()\n\tif true then\n\t\tConsole.Log(\"yes\")\n\t\tfor index = 1, 3 do\n\t\t\tConsole.Log(tostring(index))\n\t\tend\n\t\tConsole.Log(\"done\")\n\telse\n\t\tConsole.Log(\"no\")\n\tend\nend)\n"
    );
    assert_eq!(compiled.files["Server/Index.lua"], "Package.Require(\"Main.lua\")\n");
    assert!(compiled.files["Package.toml"].contains("[script]"));
    assert!(!compiled.files.contains_key("Client/Index.lua"));

    let (_, log) = run(&compiled, "Server", "fire('Package.Load')");
    assert_eq!(log, ["yes", "1", "2", "3", "done"]);
}

#[test]
fn compiles_functions_and_calls_them() {
    let package = TestPackage::new();
    let function = package.graph(ScriptContext::Shared, "Add Bonus", GraphKind::Function);
    update_function_signature(
        &function,
        FunctionSignature {
            parameters: vec![FunctionPin {
                id: "amount".to_owned(),
                name: "Amount".to_owned(),
                type_name: "number".to_owned(),
            }],
            returns: vec![FunctionPin {
                id: "total".to_owned(),
                name: "Total".to_owned(),
                type_name: "number".to_owned(),
            }],
            ..FunctionSignature::default()
        },
    )
    .unwrap();
    let entry = entry_of(&function);
    let mut body = Nodes::loaded(&function);
    let add = body.add("math/add");
    body.wire(entry, "amount", add, "a");
    body.literal(add, "b", 10);
    let result = body.add("function/return");
    body.wire(add, "result", result, "total");
    body.then(entry, "then", result);
    body.save(&function);

    let function_id = GraphDocument::load(&function).unwrap().manifest.id;
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let call = nodes.add(&format!("function/{function_id}"));
    nodes.literal(call, "amount", 5);
    let print = nodes.add("debug/print");
    nodes.wire(call, "total", print, "value");
    nodes.then(load, "then", call);
    nodes.then(call, "then", print);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    assert_eq!(compiled.files["Shared/AddBonus.lua"], "function AddBonus(amount)\n\treturn amount + 10\nend\n");
    assert!(compiled.files["Server/Main.lua"].contains("\tlocal total = AddBonus(5)\n\tConsole.Log(tostring(total))\n"));
    assert_eq!(
        compiled.files["Server/Index.lua"],
        "Package.Require(\"Shared/AddBonus.lua\")\nPackage.Require(\"Main.lua\")\n"
    );
    assert_eq!(compiled.files["Client/Index.lua"], "Package.Require(\"Shared/AddBonus.lua\")\n");

    let (_, log) = run(&compiled, "Server", "fire('Package.Load')");
    assert_eq!(log, ["15"]);
}

#[test]
fn compiles_classes_with_members_methods_events_and_casts() {
    let package = TestPackage::new();
    let shared = package.root(ScriptContext::Shared);
    let zombie = create_class(&shared, "Zombie", "Character").unwrap();
    let zombie_id = GraphDocument::load(&zombie).unwrap().manifest.id;
    let rage = VariableDocument {
        id: Id::generate(),
        name: "Rage".to_owned(),
        type_name: "number".to_owned(),
        default: Some(toml::Value::Integer(5)),
        description: String::new(),
        sync: true,
    };
    apply_variable_changes(&zombie, std::slice::from_ref(&rage), &[]).unwrap();

    let enrage = create_method(&zombie, "Enrage").unwrap();
    let enrage_id = GraphDocument::load(&enrage).unwrap().manifest.id;
    let entry = entry_of(&enrage);
    let mut method = Nodes::loaded(&enrage);
    let get = method.add(&format!("variable/get/{}", rage.id));
    let add = method.add("math/add");
    method.wire(get, "value", add, "a");
    method.literal(add, "b", 1);
    let set = method.add(&format!("variable/set/{}", rage.id));
    method.wire(add, "result", set, "value");
    method.then(entry, "then", set);
    method.save(&enrage);

    let mut class_graph = Nodes::default();
    let spawn_event = class_graph.add("class-event/Spawn");
    let call = class_graph.add(&format!("method/{enrage_id}"));
    class_graph.then(spawn_event, "then", call);
    class_graph.save(&zombie);

    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let spawn = nodes.add(&format!("class/spawn/{zombie_id}"));
    let cast = nodes.add("cast/Zombie");
    nodes.wire(spawn, "return_value", cast, "object");
    let read = nodes.add(&format!("variable/get/{}", rage.id));
    nodes.wire(cast, "result", read, "self");
    let print = nodes.add("debug/print");
    nodes.wire(read, "value", print, "value");
    nodes.then(load, "then", spawn);
    nodes.then(spawn, "then", cast);
    nodes.then(cast, "then", print);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    assert_eq!(
        compiled.files["Shared/Zombie.lua"],
        "Zombie = Character.Inherit(\"Zombie\")\n\nfunction Zombie:Enrage()\n\tself:SetValue(\"Rage\", self:GetValue(\"Rage\", 5) + 1, true)\nend\n\nZombie.Subscribe(\"Spawn\", function(self)\n\tself:Enrage()\nend)\n"
    );
    let main_code = &compiled.files["Server/Main.lua"];
    assert!(main_code.contains("local zombie = Zombie(Vector(0, 0, 0), Rotator(0, 0, 0), \"\")"), "{main_code}");
    assert!(main_code.contains("if zombie and zombie:IsA(Zombie) then"), "{main_code}");
    assert_eq!(compiled.graph_files[&enrage_id], "Shared/Zombie.lua");

    let (lua, log) = run(&compiled, "Server", "fire('Package.Load')");
    assert_eq!(log, ["6"]);
    let synced: bool = lua.load("return Zombie.parent == Character").eval().unwrap();
    assert!(synced);
}

#[test]
fn merges_shared_paths_into_local_functions() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let branch = nodes.add("flow/branch");
    nodes.literal(branch, "condition", false);
    let a = nodes.print("A");
    let b = nodes.print("B");
    let c = nodes.print("C");
    nodes.then(load, "then", branch);
    nodes.then(branch, "true", a);
    nodes.then(branch, "false", b);
    nodes.then(a, "then", c);
    nodes.then(b, "then", c);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    let code = &compiled.files["Server/Main.lua"];
    assert!(code.contains("\tlocal function continue_print()\n\t\tConsole.Log(\"C\")\n\tend\n"), "{code}");
    assert_eq!(code.matches("continue_print()").count(), 3, "{code}");
    let (_, log) = run(&compiled, "Server", "fire('Package.Load')");
    assert_eq!(log, ["B", "C"]);
}

#[test]
fn hoists_values_read_outside_of_their_block() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let sequence = nodes.add("flow/sequence");
    let branch = nodes.add("flow/branch");
    nodes.literal(branch, "condition", true);
    let spawn = nodes.add("construct/Prop");
    let is_prop = nodes.add("is-a/Prop");
    nodes.wire(spawn, "return_value", is_prop, "object");
    let print = nodes.add("debug/print");
    nodes.wire(is_prop, "result", print, "value");
    nodes.then(load, "then", sequence);
    nodes.then(sequence, "then_0", branch);
    nodes.then(branch, "true", spawn);
    nodes.then(sequence, "then_1", print);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    let code = &compiled.files["Server/Main.lua"];
    assert!(code.starts_with("Package.Subscribe(\"Load\", function()\n\tlocal prop\n"), "{code}");
    assert!(code.contains("\t\tprop = Prop(Vector(0, 0, 0), Rotator(0, 0, 0), \"\")\n"), "{code}");
    assert!(code.contains("Console.Log(tostring(prop ~= nil and prop:IsA(Prop)))"), "{code}");
    let (_, log) = run(&compiled, "Server", "fire('Package.Load')");
    assert_eq!(log, ["true"]);
}

#[test]
fn compiles_delays_while_loops_and_graph_variables() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let count = VariableDocument {
        id: Id::generate(),
        name: "Count".to_owned(),
        type_name: "integer".to_owned(),
        default: None,
        description: String::new(),
        sync: false,
    };
    apply_variable_changes(&main, std::slice::from_ref(&count), &[]).unwrap();
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let condition_value = nodes.add(&format!("variable/get/{}", count.id));
    let less = nodes.add("compare/less");
    nodes.wire(condition_value, "value", less, "a");
    nodes.literal(less, "b", 3);
    let while_loop = nodes.add("flow/while");
    nodes.wire(less, "result", while_loop, "condition");
    let current = nodes.add(&format!("variable/get/{}", count.id));
    let increment = nodes.add("math/add");
    nodes.wire(current, "value", increment, "a");
    nodes.literal(increment, "b", 1);
    let set = nodes.add(&format!("variable/set/{}", count.id));
    nodes.wire(increment, "result", set, "value");
    let delay = nodes.add("flow/delay");
    nodes.literal(delay, "seconds", 2.5);
    let print = nodes.add("debug/print");
    nodes.wire(set, "value", print, "value");
    nodes.then(load, "then", while_loop);
    nodes.then(while_loop, "loop_body", set);
    nodes.then(while_loop, "completed", delay);
    nodes.then(delay, "completed", print);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    let code = &compiled.files["Server/Main.lua"];
    assert!(code.starts_with("local count = 0\n\nPackage.Subscribe"), "{code}");
    assert!(code.contains("\twhile count < 3 do\n\t\tcount = count + 1\n\tend\n"), "{code}");
    assert!(code.contains("\tTimer.SetTimeout(function()\n\t\tConsole.Log(tostring(count))\n\tend, 2500)\n"), "{code}");
    let (_, log) = run(
        &compiled,
        "Server",
        "fire('Package.Load') assert(timers[1].milliseconds == 2500) timers[1].callback()",
    );
    assert_eq!(log, ["3"]);
}

#[test]
fn reports_name_clashes_missing_self_and_bad_classes() {
    let package = TestPackage::new();
    package.graph(ScriptContext::Shared, "Heal", GraphKind::Function);
    package.graph(ScriptContext::Server, "heal", GraphKind::Function);
    package.graph(ScriptContext::Client, "Player", GraphKind::Function);
    let shared = package.root(ScriptContext::Shared);
    create_class(&shared, "Character", "Prop").unwrap();
    let server = package.root(ScriptContext::Server);
    let guard = create_class(&server, "Guard", "Actor").unwrap();
    let shout = create_method(&guard, "Shout").unwrap();
    let shout_id = GraphDocument::load(&shout).unwrap().manifest.id;
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let call = nodes.add(&format!("method/{shout_id}"));
    nodes.then(load, "then", call);
    nodes.save(&guard);

    let compiled = package.compile();
    let codes = codes(&compiled);
    for expected in ["function-name-clash", "function-name-taken", "class-name-taken", "invalid-parent", "no-self", "class-one-side"] {
        assert!(codes.contains(&expected), "missing {expected} in {codes:?}");
    }
    assert!(compiled.has_errors());
    let clash = compiled.problems.iter().find(|problem| problem.code == "function-name-clash").unwrap();
    assert!(clash.message.contains("both become 'Heal'"), "{}", clash.message);
}

#[test]
fn writes_only_changed_files_and_removes_stale_ones() {
    let directory = tempfile::tempdir().unwrap();
    let mut files = BTreeMap::new();
    files.insert("Server/Index.lua".to_owned(), "Package.Require(\"Old.lua\")\n".to_owned());
    files.insert("Server/Old.lua".to_owned(), "print()\n".to_owned());
    std::fs::create_dir_all(directory.path().join("Client")).unwrap();
    std::fs::write(directory.path().join("Client/Handwritten.lua"), "x = 1\n").unwrap();
    assert_eq!(write_output(directory.path(), &files).unwrap().changed, 2);
    assert_eq!(write_output(directory.path(), &files).unwrap().changed, 0);

    files.remove("Server/Old.lua");
    files.insert("Server/Index.lua".to_owned(), String::new());
    let summary = write_output(directory.path(), &files).unwrap();
    assert_eq!((summary.changed, summary.removed), (1, 1));
    assert!(!directory.path().join("Server/Old.lua").exists());
    assert!(directory.path().join("Client/Handwritten.lua").exists());
}

#[test]
fn compiles_api_calls_literals_pure_functions_and_base_class_casts() {
    let package = TestPackage::new();
    let pure = package.graph(ScriptContext::Shared, "Double", GraphKind::Function);
    update_function_signature(
        &pure,
        FunctionSignature {
            pure: true,
            parameters: vec![FunctionPin {
                id: "value".to_owned(),
                name: "Value".to_owned(),
                type_name: "number".to_owned(),
            }],
            returns: vec![FunctionPin {
                id: "result".to_owned(),
                name: "Result".to_owned(),
                type_name: "number".to_owned(),
            }],
            ..FunctionSignature::default()
        },
    )
    .unwrap();
    let entry = entry_of(&pure);
    let mut body = Nodes::loaded(&pure);
    let multiply = body.add("math/multiply");
    body.wire(entry, "value", multiply, "a");
    body.literal(multiply, "b", 2);
    let result = body.add("function/return");
    body.wire(multiply, "result", result, "result");
    body.save(&pure);
    let pure_id = GraphDocument::load(&pure).unwrap().manifest.id;

    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let ready = nodes.add("event/Player/Ready");
    let name = nodes.add("call/Player/GetName");
    nodes.wire(ready, "self", name, "self");
    let greeting = nodes.add("string/concat");
    nodes.literal(greeting, "a", "Welcome ");
    nodes.wire(name, "player_name", greeting, "b");
    let broadcast = nodes.add("static/Chat/BroadcastMessage");
    nodes.wire(greeting, "result", broadcast, "message");
    let character = nodes.add("call/Player/GetControlledCharacter");
    nodes.wire(ready, "self", character, "self");
    let cast = nodes.add("cast/Pawn");
    nodes.wire(character, "controlled_character", cast, "object");
    let set_location = nodes.add("call/Actor/SetLocation");
    nodes.wire(cast, "result", set_location, "self");
    let mut location = toml::map::Map::new();
    location.insert("X".to_owned(), toml::Value::Integer(10));
    location.insert("Z".to_owned(), toml::Value::Float(2.5));
    nodes.literal(set_location, "location", toml::Value::Table(location));
    let collision = nodes.add("call/Actor/SetCollision");
    nodes.wire(cast, "result", collision, "self");
    nodes.literal(collision, "collision_type", "NoCollision");
    let double = nodes.add(&format!("function/{pure_id}"));
    nodes.literal(double, "value", 21);
    let print = nodes.add("debug/print");
    nodes.wire(double, "result", print, "value");
    nodes.then(ready, "then", broadcast);
    nodes.then(broadcast, "then", cast);
    nodes.then(cast, "then", set_location);
    nodes.then(set_location, "then", collision);
    nodes.then(cast, "failed", print);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    assert_eq!(compiled.files["Shared/Double.lua"], "function Double(value)\n\treturn value * 2\nend\n");
    let code = &compiled.files["Server/Main.lua"];
    let expected = [
        "Player.Subscribe(\"Ready\", function(player)",
        "\tChat.BroadcastMessage(\"Welcome \" .. player:GetName())",
        "\tlocal pawn = player:GetControlledCharacter()",
        "\tif pawn and (pawn:IsA(Character) or pawn:IsA(CharacterSimple)) then",
        "\t\tpawn:SetLocation(Vector(10, 0, 2.5))",
        "\t\tpawn:SetCollision(CollisionType.NoCollision)",
        "\telse",
        "\t\tConsole.Log(tostring(Double(21)))",
    ];
    for line in expected {
        assert!(code.contains(line), "missing {line:?} in\n{code}");
    }
}

#[test]
fn rejects_execution_loops() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let first = nodes.print("one");
    let second = nodes.print("two");
    nodes.then(load, "then", first);
    nodes.then(first, "then", second);
    nodes.then(second, "then", first);
    nodes.save(&main);

    let compiled = package.compile();
    assert!(codes(&compiled).contains(&"exec-loop"), "{:?}", codes(&compiled));
}

fn event(package: &TestPackage, name: &str, direction: EventDirection, parameter: (&str, &str)) -> Id {
    let path = package.graph(ScriptContext::Shared, name, GraphKind::Event);
    update_event(
        &path,
        direction,
        FunctionSignature {
            parameters: vec![FunctionPin {
                id: parameter.0.to_lowercase(),
                name: parameter.0.to_owned(),
                type_name: parameter.1.to_owned(),
            }],
            ..FunctionSignature::default()
        },
    )
    .unwrap();
    GraphDocument::load(&path).unwrap().manifest.id
}

#[test]
fn sends_network_events_between_client_and_server() {
    let package = TestPackage::new();
    let shot = event(&package, "Player Shot", EventDirection::ToServer, ("Damage", "number"));
    let round = event(&package, "Round Started", EventDirection::ToClients, ("Round", "integer"));
    let local = event(&package, "Score Changed", EventDirection::Local, ("Score", "integer"));

    let client = package.graph(ScriptContext::Client, "Hud", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let send = nodes.add(&format!("send-event/{shot}"));
    nodes.literal(send, "damage", 25);
    nodes.then(load, "then", send);
    let on_round = nodes.add(&format!("custom-event/{round}"));
    let print_round = nodes.add("debug/print");
    nodes.wire(on_round, "round", print_round, "value");
    nodes.then(on_round, "then", print_round);
    nodes.save(&client);

    let server = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let on_shot = nodes.add(&format!("custom-event/{shot}"));
    let print_damage = nodes.add("debug/print");
    nodes.wire(on_shot, "damage", print_damage, "value");
    let to_player = nodes.add(&format!("send-event/{round}/player"));
    nodes.wire(on_shot, "player", to_player, "player");
    nodes.literal(to_player, "round", 3);
    let to_all = nodes.add(&format!("send-event/{round}"));
    nodes.literal(to_all, "round", 4);
    let call_local = nodes.add(&format!("send-event/{local}"));
    nodes.literal(call_local, "score", 7);
    let on_local = nodes.add(&format!("custom-event/{local}"));
    let print_score = nodes.add("debug/print");
    nodes.wire(on_local, "score", print_score, "value");
    nodes.then(on_shot, "then", print_damage);
    nodes.then(print_damage, "then", to_player);
    nodes.then(to_player, "then", to_all);
    nodes.then(to_all, "then", call_local);
    nodes.then(on_local, "then", print_score);
    nodes.save(&server);

    let compiled = package.compile();
    assert_clean(&compiled);
    assert!(!compiled.files.keys().any(|path| path.contains("PlayerShot")));
    let server_code = &compiled.files["Server/Main.lua"];
    for line in [
        "Events.SubscribeRemote(\"PlayerShot\", function(player, damage)",
        "\tEvents.CallRemote(\"RoundStarted\", player, 3)",
        "\tEvents.BroadcastRemote(\"RoundStarted\", 4)",
        "\tEvents.Call(\"ScoreChanged\", 7)",
        "Events.Subscribe(\"ScoreChanged\", function(score)",
    ] {
        assert!(server_code.contains(line), "missing {line:?} in\n{server_code}");
    }
    assert!(compiled.files["Client/Hud.lua"].contains("Events.CallRemote(\"PlayerShot\", 25)"));

    let (lua, _) = run(&compiled, "Client", "fire('Package.Load')");
    let sent: String = lua.load("return sent[1].kind .. ' ' .. sent[1].name .. ' ' .. sent[1].arguments[1]").eval().unwrap();
    assert_eq!(sent, "CallRemote PlayerShot 25");

    let (lua, log) = run(&compiled, "Server", "fire('remote.PlayerShot', Player(), 25)");
    assert_eq!(log, ["25", "7"]);
    let sent: String = lua
        .load("return sent[1].kind .. ' ' .. sent[1].arguments[2] .. ' ' .. sent[2].kind .. ' ' .. sent[2].arguments[1]")
        .eval()
        .unwrap();
    assert_eq!(sent, "CallRemote 3 BroadcastRemote 4");

    let (_, log) = run(&compiled, "Client", "fire('remote.RoundStarted', 3)");
    assert_eq!(log, ["3"]);
}

#[test]
fn reports_network_events_on_the_wrong_side() {
    let package = TestPackage::new();
    let shot = event(&package, "Shot", EventDirection::ToServer, ("Damage", "number"));
    let client = package.graph(ScriptContext::Client, "Hud", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let handler = nodes.add(&format!("custom-event/{shot}"));
    let print = nodes.print("never");
    nodes.then(handler, "then", print);
    nodes.save(&client);
    let compiled = package.compile();
    let problem = compiled.problems.iter().find(|problem| problem.code == "wrong-side").unwrap();
    assert!(problem.message.contains("'On Shot' only works on the server"), "{}", problem.message);
}

#[test]
fn builds_into_the_local_output_folder_and_never_overwrites_foreign_folders() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("project");
    let project_file = root.join("Game.nfproj");
    nf_project::ProjectManifest::new("Game", ApiChannel::Stable).write(&project_file).unwrap();
    let package = root.join("packages").join("game");
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
    let main = create_graph(&context_root(&package, ScriptContext::Server), "Main", GraphKind::EventGraph).unwrap();
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let print = nodes.print("hi");
    nodes.then(load, "then", print);
    nodes.save(&main);

    let server_packages = directory.path().join("server").join("Packages");
    std::fs::create_dir_all(&server_packages).unwrap();
    nf_project::LocalSettings {
        output_directory: Some(server_packages.clone()),
        ..Default::default()
    }
    .write(&root)
    .unwrap();

    let project = nf_project::Project::open(&project_file).unwrap();
    assert_eq!(project.output_directory(), server_packages);
    let report = crate::build_project(&project, catalog()).unwrap();
    assert!(report.packages[0].written, "{:#?}", report.packages[0].problems);
    assert!(server_packages.join("game/Server/Main.lua").is_file());

    std::fs::remove_file(server_packages.join("game").join(crate::BUILD_MANIFEST)).unwrap();
    let report = crate::build_project(&project, catalog()).unwrap();
    assert!(!report.packages[0].written);
    assert_eq!(report.packages[0].problems[0].code, "output-not-owned");
}

#[test]
fn turns_function_parameters_into_inline_callbacks() {
    let package = TestPackage::new();
    let server = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let interval = nodes.add("static/Timer/SetInterval");
    nodes.literal(interval, "milliseconds", 100);
    let tick = nodes.print("tick");
    let clear = nodes.add("static/Timer/ClearInterval");
    nodes.wire(interval, "return_value", clear, "interval_id");
    let command = nodes.add("static/Console/RegisterCommand");
    nodes.literal(command, "command", "heal");
    let each = nodes.add("flow/for-each");
    nodes.wire(command, "callback_params", each, "array");
    let print = nodes.add("debug/print");
    nodes.wire(each, "element", print, "value");
    nodes.then(load, "then", interval);
    nodes.then(interval, "callback", tick);
    nodes.then(tick, "then", clear);
    nodes.then(interval, "then", command);
    nodes.then(command, "callback", each);
    nodes.then(each, "loop_body", print);
    nodes.save(&server);

    let client = package.graph(ScriptContext::Client, "Controls", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let bind = nodes.add("static/Input/Bind");
    nodes.literal(bind, "binding_name", "Jump");
    nodes.literal(bind, "input_event", "Pressed");
    let print = nodes.add("debug/print");
    nodes.wire(bind, "callback_delta", print, "value");
    nodes.then(load, "then", bind);
    nodes.then(bind, "callback", print);
    nodes.save(&client);

    let compiled = package.compile();
    assert_clean(&compiled);
    let code = &compiled.files["Server/Main.lua"];
    for line in [
        "\tlocal set_interval\n\tset_interval = Timer.SetInterval(function()\n\t\tConsole.Log(\"tick\")\n\t\tTimer.ClearInterval(set_interval)\n\tend, 100)\n",
        "\tConsole.RegisterCommand(\"heal\", function(...)\n\t\tlocal params = { ... }\n\t\tfor _, element in ipairs(params) do\n",
    ] {
        assert!(code.contains(line), "missing {line:?} in\n{code}");
    }
    assert!(
        compiled.files["Client/Controls.lua"].contains("Input.Bind(\"Jump\", InputEvent.Pressed, function(delta)"),
        "{}",
        compiled.files["Client/Controls.lua"]
    );

    let (lua, log) = run(
        &compiled,
        "Server",
        "fire('Package.Load') timers[1].callback() commands.heal('a', 'b')",
    );
    assert_eq!(log, ["tick", "a", "b"]);
    let cleared: bool = lua.load("return timers[1].cleared == true and timers[1].milliseconds == 100").eval().unwrap();
    assert!(cleared);
    let (_, log) = run(&compiled, "Client", "fire('Package.Load') bindings['Jump:Pressed'](0.5)");
    assert_eq!(log, ["0.5"]);
}

#[test]
fn reroutes_pass_values_and_execution_through_and_comments_are_ignored() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let comment = nodes.add("comment");
    nodes.node(comment).properties.insert("text".to_owned(), toml::Value::String("Greeting".to_owned()));
    let load = nodes.add("event/Package/Load");
    let text = nodes.add("literal/string");
    nodes.literal(text, "value", "hello");
    let knot = nodes.add("flow/reroute");
    nodes.wire(text, "result", knot, "value");
    let exec_knot = nodes.add("flow/reroute-exec");
    let print = nodes.add("debug/print");
    nodes.wire(knot, "value", print, "value");
    nodes.then(load, "then", exec_knot);
    nodes.then(exec_knot, "then", print);
    let bad = nodes.add("call/Actor/SetLocation");
    let bad_knot = nodes.add("flow/reroute");
    nodes.wire(text, "result", bad_knot, "value");
    nodes.wire(bad_knot, "value", bad, "self");
    nodes.then(print, "then", bad);
    nodes.save(&main);

    let compiled = package.compile();
    let mismatch = compiled.problems.iter().find(|problem| problem.code == "type-mismatch");
    assert!(mismatch.is_some_and(|problem| problem.message.contains("is a string")), "{:#?}", compiled.problems);
    assert_eq!(compiled.problems.len(), 1, "{:#?}", compiled.problems);
    assert!(compiled.files["Server/Main.lua"].contains("\tConsole.Log(\"hello\")\n"), "{}", compiled.files["Server/Main.lua"]);
}

#[test]
fn compiles_array_and_map_nodes_with_local_helpers() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let players = VariableDocument {
        id: Id::generate(),
        name: "Players".to_owned(),
        type_name: "string[]".to_owned(),
        default: None,
        description: String::new(),
        sync: false,
    };
    let scores = VariableDocument {
        id: Id::generate(),
        name: "Scores".to_owned(),
        type_name: "table".to_owned(),
        default: None,
        description: String::new(),
        sync: false,
    };
    apply_variable_changes(&main, &[players.clone(), scores.clone()], &[]).unwrap();
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let mut previous = (load, "then");
    let mut adds = Vec::new();
    for name in ["a", "b", "c"] {
        let list = nodes.add(&format!("variable/get/{}", players.id));
        let add = nodes.add("array/add");
        nodes.wire(list, "value", add, "array");
        nodes.literal(add, "element", name);
        nodes.then(previous.0, previous.1, add);
        previous = (add, "then");
        adds.push(add);
    }
    let list = nodes.add(&format!("variable/get/{}", players.id));
    let remove = nodes.add("array/remove-element");
    nodes.wire(list, "value", remove, "array");
    nodes.literal(remove, "element", "b");
    nodes.then(previous.0, previous.1, remove);
    let join = nodes.add("array/join");
    nodes.wire(list, "value", join, "array");
    let print_join = nodes.add("debug/print");
    nodes.wire(join, "result", print_join, "value");
    nodes.then(remove, "then", print_join);
    let contains = nodes.add("array/contains");
    nodes.wire(list, "value", contains, "array");
    nodes.literal(contains, "element", "c");
    let print_contains = nodes.add("debug/print");
    nodes.wire(contains, "result", print_contains, "value");
    nodes.then(print_join, "then", print_contains);
    let map = nodes.add(&format!("variable/get/{}", scores.id));
    let set = nodes.add("map/set");
    nodes.wire(map, "value", set, "map");
    nodes.literal(set, "key", "a");
    nodes.literal(set, "value", 10);
    nodes.then(print_contains, "then", set);
    let pairs = nodes.add("flow/for-each-pair");
    nodes.wire(map, "value", pairs, "map");
    let print_value = nodes.add("debug/print");
    nodes.wire(pairs, "value", print_value, "value");
    nodes.then(set, "then", pairs);
    nodes.then(pairs, "loop_body", print_value);
    let count = nodes.add("map/count");
    nodes.wire(map, "value", count, "map");
    let print_count = nodes.add("debug/print");
    nodes.wire(count, "count", print_count, "value");
    nodes.then(pairs, "completed", print_count);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    let code = &compiled.files["Server/Main.lua"];
    for line in [
        "local function array_find(array, element)",
        "local function array_remove_element(array, element)",
        "local function map_count(map)",
        "local players = {}\n",
        "local scores = {}\n",
        "\ttable.insert(players, \"a\")\n",
        "\tarray_remove_element(players, \"b\")\n",
        "\tConsole.Log(table.concat(players, \", \"))\n",
        "\tConsole.Log(tostring(array_find(players, \"c\") ~= nil))\n",
        "\tConsole.Log(table.concat(players, \", \"))\n",
        "\tscores[\"a\"] = 10\n",
        "\tfor _, value in pairs(scores) do\n",
    ] {
        assert!(code.contains(line), "missing {line:?} in\n{code}");
    }
    assert!(!code.contains("array_clear"), "{code}");
    let (_, log) = run(&compiled, "Server", "fire('Package.Load')");
    assert_eq!(log, ["a, c", "true", "10", "1"]);
}

#[test]
fn compiles_project_structs_to_plain_tables() {
    let package = TestPackage::new();
    let stats = package.graph(ScriptContext::Shared, "PlayerStats", GraphKind::Struct);
    let field = |id: &str, name: &str, type_name: &str| FunctionPin {
        id: id.to_owned(),
        name: name.to_owned(),
        type_name: type_name.to_owned(),
    };
    update_function_signature(
        &stats,
        FunctionSignature {
            parameters: vec![field("kills", "Kills", "integer"), field("name", "Name", "string"), field("spot", "Spawn Spot", "Vector")],
            ..FunctionSignature::default()
        },
    )
    .unwrap();
    let stats_id = GraphDocument::load(&stats).unwrap().manifest.id;
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let variable = VariableDocument {
        id: Id::generate(),
        name: "Stats".to_owned(),
        type_name: "PlayerStats".to_owned(),
        default: None,
        description: String::new(),
        sync: false,
    };
    apply_variable_changes(&main, std::slice::from_ref(&variable), &[]).unwrap();
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let current = nodes.add(&format!("variable/get/{}", variable.id));
    let set = nodes.add(&format!("struct/set/{stats_id}"));
    nodes.wire(current, "value", set, "value");
    nodes.literal(set, "kills", 3);
    let split = nodes.add(&format!("struct/break/{stats_id}"));
    nodes.wire(current, "value", split, "value");
    let print_kills = nodes.add("debug/print");
    nodes.wire(split, "kills", print_kills, "value");
    let make = nodes.add(&format!("struct/make/{stats_id}"));
    nodes.literal(make, "name", "bob");
    let split_made = nodes.add(&format!("struct/break/{stats_id}"));
    nodes.wire(make, "value", split_made, "value");
    let print_name = nodes.add("debug/print");
    nodes.wire(split_made, "name", print_name, "value");
    let location = nodes.add("call/Actor/SetLocation");
    nodes.wire(make, "value", location, "location");
    nodes.then(load, "then", set);
    nodes.then(set, "then", print_kills);
    nodes.then(print_kills, "then", print_name);
    nodes.save(&main);

    let compiled = package.compile();
    let codes = codes(&compiled);
    assert!(codes.contains(&"type-mismatch"), "{codes:?}");
    let code = &compiled.files["Server/Main.lua"];
    for line in [
        "local stats = { Kills = 0, Name = \"\", SpawnSpot = Vector(0, 0, 0) }",
        "\tstats.Kills = 3\n",
        "\tConsole.Log(tostring(stats.Kills))\n",
        "\tConsole.Log(({ Kills = 0, Name = \"bob\", SpawnSpot = Vector(0, 0, 0) }).Name)\n",
    ] {
        assert!(code.contains(line), "missing {line:?} in\n{code}");
    }
    assert!(!compiled.files.keys().any(|path| path.contains("PlayerStats")));
}

#[test]
fn array_nodes_take_their_element_type_from_the_connected_array() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let players = VariableDocument {
        id: Id::generate(),
        name: "Players".to_owned(),
        type_name: "Player[]".to_owned(),
        default: None,
        description: String::new(),
        sync: false,
    };
    apply_variable_changes(&main, std::slice::from_ref(&players), &[]).unwrap();
    let mut nodes = Nodes::default();
    let ready = nodes.add("event/Player/Ready");
    let list = nodes.add(&format!("variable/get/{}", players.id));
    let add = nodes.add("array/add");
    nodes.wire(list, "value", add, "array");
    nodes.wire(ready, "self", add, "element");
    let first = nodes.add("array/get");
    nodes.wire(list, "value", first, "array");
    let name = nodes.add("call/Player/GetName");
    nodes.wire(first, "element", name, "self");
    let broadcast = nodes.add("static/Chat/BroadcastMessage");
    nodes.wire(name, "player_name", broadcast, "message");
    let location = nodes.add("call/Actor/SetLocation");
    nodes.wire(first, "element", location, "self");
    let wrong = nodes.add("array/add");
    nodes.wire(list, "value", wrong, "array");
    nodes.literal(wrong, "element", 5);
    let vector = nodes.add("construct/Vector");
    let wrong_wire = nodes.add("array/add");
    nodes.wire(list, "value", wrong_wire, "array");
    nodes.wire(vector, "return_value", wrong_wire, "element");
    nodes.then(ready, "then", add);
    nodes.then(add, "then", broadcast);
    nodes.then(broadcast, "then", location);
    nodes.then(location, "then", wrong);
    nodes.then(wrong, "then", wrong_wire);
    nodes.save(&main);

    let compiled = package.compile();
    let mismatches: Vec<&str> = compiled
        .problems
        .iter()
        .filter(|problem| problem.code == "type-mismatch")
        .map(|problem| problem.message.as_str())
        .collect();
    assert_eq!(mismatches.len(), 2, "{:#?}", compiled.problems);
    assert!(mismatches.iter().any(|message| message.contains("is a Player") && message.contains("needs an Actor")), "{mismatches:?}");
    assert!(mismatches.iter().any(|message| message.contains("is a Vector") && message.contains("needs a Player")), "{mismatches:?}");
    assert!(compiled.problems.iter().any(|problem| problem.code == "invalid-literal"), "{:#?}", compiled.problems);
}

fn case(id: &str, value: impl Into<toml::Value>) -> toml::Value {
    let mut table = toml::map::Map::new();
    table.insert("id".to_owned(), toml::Value::String(id.to_owned()));
    table.insert("value".to_owned(), value.into());
    toml::Value::Table(table)
}

#[test]
fn compiles_switches_state_nodes_breaks_and_dynamic_pins() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let sequence = nodes.add("flow/sequence");
    nodes.node(sequence).properties.insert("count".to_owned(), toml::Value::Integer(4));
    nodes.then(load, "then", sequence);

    let looping = nodes.add("flow/for-loop");
    nodes.literal(looping, "last_index", 10);
    let switch = nodes.add("flow/switch-integer");
    nodes
        .node(switch)
        .properties
        .insert("cases".to_owned(), toml::Value::Array(vec![case("two", 2), case("four", 4)]));
    nodes.wire(looping, "index", switch, "value");
    let two = nodes.print("two");
    let stop = nodes.add("flow/break");
    let other = nodes.add("debug/print");
    nodes.wire(looping, "index", other, "value");
    nodes.then(sequence, "then_0", looping);
    nodes.then(looping, "loop_body", switch);
    nodes.then(switch, "case_two", two);
    nodes.then(switch, "case_four", stop);
    nodes.then(switch, "default", other);

    let once = nodes.add("flow/do-once");
    let once_print = nodes.print("once");
    nodes.then(sequence, "then_1", once);
    nodes.then(once, "completed", once_print);

    let flip = nodes.add("flow/flip-flop");
    let a = nodes.print("A");
    let b = nodes.print("B");
    nodes.then(sequence, "then_2", flip);
    nodes.then(flip, "a", a);
    nodes.then(flip, "b", b);

    let prop = nodes.add("construct/Prop");
    let valid = nodes.add("flow/is-valid");
    nodes.wire(prop, "return_value", valid, "object");
    let select = nodes.add("flow/select");
    nodes.literal(select, "condition", false);
    nodes.literal(select, "if_true", "yes");
    nodes.literal(select, "if_false", "no");
    let print_select = nodes.add("debug/print");
    nodes.wire(select, "value", print_select, "value");
    let list = nodes.add("array/of");
    nodes.literal(list, "element_0", "x");
    nodes.literal(list, "element_1", "y");
    let join = nodes.add("array/join");
    nodes.wire(list, "result", join, "array");
    nodes.literal(join, "separator", "-");
    let print_join = nodes.add("debug/print");
    nodes.wire(join, "result", print_join, "value");
    let event_switch = nodes.add("flow/switch-enum/InputEvent");
    nodes.literal(event_switch, "value", "Released");
    let pressed = nodes.print("pressed");
    let released = nodes.print("released");
    nodes.then(sequence, "then_3", prop);
    nodes.then(prop, "then", valid);
    nodes.then(valid, "valid", print_select);
    nodes.then(print_select, "then", print_join);
    nodes.then(print_join, "then", event_switch);
    nodes.then(event_switch, "case_Pressed", pressed);
    nodes.then(event_switch, "case_Released", released);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    let code = &compiled.files["Server/Main.lua"];
    for line in [
        "local done_once = false\n",
        "local flip_flop = false\n",
        "\t\tif index == 2 then\n",
        "\t\telseif index == 4 then\n\t\t\tbreak\n",
        "\tflip_flop = not flip_flop\n",
        "\tif prop ~= nil and prop:IsValid() then\n",
        "Console.Log(table.concat({ \"x\", \"y\" }, \"-\"))",
        "if InputEvent.Released == InputEvent.Pressed then",
    ] {
        assert!(code.contains(line), "missing {line:?} in\n{code}");
    }
    let (_, log) = run(&compiled, "Server", "fire('Package.Load') fire('Package.Load')");
    let round = |flip: &'static str, first: bool| {
        let mut expected = vec!["1", "two", "3"];
        if first {
            expected.push("once");
        }
        expected.extend([flip, "no", "x-y", "released"]);
        expected
    };
    let mut expected = round("A", true);
    expected.extend(round("B", false));
    assert_eq!(log, expected);
}

#[test]
fn reports_break_outside_of_loops() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let stop = nodes.add("flow/break");
    nodes.then(load, "then", stop);
    nodes.save(&main);
    assert!(codes(&package.compile()).contains(&"break-outside-loop"));
}

#[test]
fn keeps_state_nodes_per_class_instance() {
    let package = TestPackage::new();
    let shared = package.root(ScriptContext::Shared);
    let zombie = create_class(&shared, "Zombie", "Character").unwrap();
    let zombie_id = GraphDocument::load(&zombie).unwrap().manifest.id;
    let greet = create_method(&zombie, "Greet").unwrap();
    let greet_id = GraphDocument::load(&greet).unwrap().manifest.id;
    let entry = entry_of(&greet);
    let mut method = Nodes::loaded(&greet);
    let sequence = method.add("flow/sequence");
    method.then(entry, "then", sequence);
    let once = method.add("flow/do-once");
    let hello = method.print("hello");
    method.then(sequence, "then_1", once);
    method.then(once, "completed", hello);
    let flip = method.add("flow/flip-flop");
    let a = method.print("A");
    let b = method.print("B");
    method.then(sequence, "then_0", flip);
    method.then(flip, "a", a);
    method.then(flip, "b", b);
    method.save(&greet);

    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let first = nodes.add(&format!("class/spawn/{zombie_id}"));
    let second = nodes.add(&format!("class/spawn/{zombie_id}"));
    nodes.then(load, "then", first);
    nodes.then(first, "then", second);
    let mut previous = (second, "then");
    for spawned in [first, first, second] {
        let call = nodes.add(&format!("method/{greet_id}"));
        nodes.wire(spawned, "return_value", call, "self");
        nodes.then(previous.0, previous.1, call);
        previous = (call, "then");
    }
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    let code = &compiled.files["Shared/Zombie.lua"];
    for line in [
        "local done_once = setmetatable({}, { __mode = \"k\" })\n",
        "\tif not done_once[self] then\n\t\tdone_once[self] = true\n",
        "\tflip_flop[self] = not flip_flop[self]\n\tif flip_flop[self] then\n",
    ] {
        assert!(code.contains(line), "missing {line:?} in\n{code}");
    }
    let (_, log) = run(&compiled, "Server", "fire('Package.Load')");
    assert_eq!(log, ["A", "hello", "B", "A", "hello"]);
}

#[test]
fn collapses_a_selection_into_a_function_that_behaves_the_same() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let start = nodes.print("start");
    let base = nodes.add("math/add");
    nodes.literal(base, "a", 1);
    nodes.literal(base, "b", 2);
    let multiply = nodes.add("math/multiply");
    nodes.wire(base, "result", multiply, "a");
    nodes.literal(multiply, "b", 10);
    let inner = nodes.add("debug/print");
    nodes.wire(multiply, "result", inner, "value");
    let after = nodes.add("debug/print");
    nodes.wire(multiply, "result", after, "value");
    nodes.then(load, "then", start);
    nodes.then(start, "then", inner);
    nodes.then(inner, "then", after);
    nodes.save(&main);
    let expected = ["start", "30", "30"];
    assert_eq!(run(&package.compile(), "Server", "fire('Package.Load')").1, expected);

    let collapsed = nf_graph::collapse_into_function(catalog(), &package.path, &main, &[multiply, inner], "Scale").unwrap();
    let function = GraphDocument::load(&collapsed.path).unwrap();
    let parameter_types: Vec<&str> = function.manifest.signature.parameters.iter().map(|pin| pin.type_name.as_str()).collect();
    let return_types: Vec<&str> = function.manifest.signature.returns.iter().map(|pin| pin.type_name.as_str()).collect();
    assert_eq!(parameter_types, ["number"]);
    assert_eq!(return_types, ["number"]);
    assert!(!function.manifest.signature.pure);
    assert_eq!(function.nodes.len(), 4);
    let remaining = GraphDocument::load(&main).unwrap().nodes;
    assert_eq!(remaining.len(), 5);
    assert!(remaining.iter().all(|node| node.id != multiply && node.id != inner));

    let compiled = package.compile();
    assert_clean(&compiled);
    assert_eq!(run(&compiled, "Server", "fire('Package.Load')").1, expected);
}

#[test]
fn collapses_pure_nodes_and_class_logic_into_methods() {
    let package = TestPackage::new();
    let shared = package.root(ScriptContext::Shared);
    let zombie = create_class(&shared, "Zombie", "Character").unwrap();
    let zombie_id = GraphDocument::load(&zombie).unwrap().manifest.id;
    let rage = VariableDocument {
        id: Id::generate(),
        name: "Rage".to_owned(),
        type_name: "number".to_owned(),
        default: Some(toml::Value::Integer(5)),
        description: String::new(),
        sync: false,
    };
    apply_variable_changes(&zombie, std::slice::from_ref(&rage), &[]).unwrap();
    let mut class_graph = Nodes::default();
    let spawn_event = class_graph.add("class-event/Spawn");
    let get = class_graph.add(&format!("variable/get/{}", rage.id));
    let add = class_graph.add("math/add");
    class_graph.wire(get, "value", add, "a");
    class_graph.literal(add, "b", 1);
    let set = class_graph.add(&format!("variable/set/{}", rage.id));
    class_graph.wire(spawn_event, "self", set, "self");
    class_graph.wire(add, "result", set, "value");
    let print = class_graph.add("debug/print");
    class_graph.wire(set, "value", print, "value");
    class_graph.then(spawn_event, "then", set);
    class_graph.then(set, "then", print);
    class_graph.save(&zombie);

    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let spawn = nodes.add(&format!("class/spawn/{zombie_id}"));
    nodes.then(load, "then", spawn);
    nodes.save(&main);

    let pure = nf_graph::collapse_into_function(catalog(), &package.path, &zombie, &[get, add], "Next Rage").unwrap();
    let signature = GraphDocument::load(&pure.path).unwrap().manifest.signature;
    assert!(signature.pure);
    assert!(signature.parameters.is_empty());
    let method = nf_graph::collapse_into_function(catalog(), &package.path, &zombie, &[set, print], "Enrage").unwrap();
    let parameters = GraphDocument::load(&method.path).unwrap().manifest.signature.parameters;
    let parameter_types: Vec<&str> = parameters.iter().map(|pin| pin.type_name.as_str()).collect();
    assert_eq!(parameter_types, ["number"]);

    let compiled = package.compile();
    assert_clean(&compiled);
    let code = &compiled.files["Shared/Zombie.lua"];
    assert!(code.contains("function Zombie:NextRage()"), "{code}");
    assert!(code.contains("self:Enrage(self:NextRage())"), "{code}");
    assert_eq!(run(&compiled, "Server", "fire('Package.Load')").1, ["6"]);
}

#[test]
fn refuses_selections_that_cannot_become_one_function() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let branch = nodes.add("flow/branch");
    let yes = nodes.print("yes");
    let no = nodes.print("no");
    nodes.then(load, "then", branch);
    nodes.then(branch, "true", yes);
    nodes.then(branch, "false", no);
    nodes.save(&main);
    let failure = |selection: &[Id]| match nf_graph::collapse_into_function(catalog(), &package.path, &main, selection, "Part") {
        Err(nf_graph::CollapseFailure::Selection(error)) => error.message,
        other => panic!("expected a selection error, got {other:?}"),
    };
    assert!(failure(&[branch]).contains("several different nodes"));
    assert!(failure(&[load, branch]).contains("Events"));
    assert!(failure(&[yes, no]).contains("several places") || failure(&[yes, no]).contains("starting points"));
    assert_eq!(GraphDocument::load(&main).unwrap().nodes.len(), 4);
}

#[test]
fn maps_lua_lines_to_nodes_and_logs_watched_values() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let nobody = VariableDocument {
        id: Id::generate(),
        name: "Nobody".to_owned(),
        type_name: "Player".to_owned(),
        default: None,
        description: String::new(),
        sync: false,
    };
    apply_variable_changes(&main, std::slice::from_ref(&nobody), &[]).unwrap();
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let sum = nodes.add("math/add");
    nodes.literal(sum, "a", 2);
    nodes.literal(sum, "b", 3);
    let watched = nodes.add("debug/print");
    nodes.wire(sum, "result", watched, "value");
    nodes.node(watched).properties.insert("watch".to_owned(), toml::Value::Boolean(true));
    let read = nodes.add(&format!("variable/get/{}", nobody.id));
    let name = nodes.add("call/Player/GetName");
    nodes.wire(read, "value", name, "self");
    let print_name = nodes.add("debug/print");
    nodes.wire(name, "player_name", print_name, "value");
    nodes.then(load, "then", watched);
    nodes.then(watched, "then", print_name);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    let code = &compiled.files["Server/Main.lua"];
    assert!(!code.contains("--@nanosflow"), "{code}");
    assert_eq!(codes(&compiled), ["watched-node"]);
    let graph = GraphDocument::load(&main).unwrap().manifest.id;
    assert!(code.contains(&format!("Console.Log(\"[NanosFlow:{graph}/{watched}] \" .. watch_values(\"Value\", 2 + 3))")), "{code}");
    let spans = &compiled.source_map["Server/Main.lua"];
    let line_of = |needle: &str| code.lines().position(|line| line.contains(needle)).unwrap() + 1;
    assert_eq!(crate::locate(spans, line_of("GetName")).unwrap().node, print_name);
    assert_eq!(crate::locate(spans, line_of("Console.Log(\"[NanosFlow")).unwrap().node, watched);

    let lua = Lua::new();
    lua.load(RUNTIME).exec().unwrap();
    lua.load(code.as_str()).set_name("Server/Main.lua").exec().unwrap();
    let failure = lua.load("fire('Package.Load')").exec().unwrap_err().to_string();
    let log: Vec<String> = lua.globals().get("log").unwrap();
    let mut text = log.join("\n");
    text.push('\n');
    text.push_str(&failure);
    let maps: crate::PackageMaps = [("game".to_owned(), compiled.source_map.clone())].into_iter().collect();
    let entries = crate::parse_log(&text, &maps);
    let watch = entries.iter().find_map(|entry| entry.watch.as_ref()).unwrap();
    assert_eq!((watch.node, watch.values.as_str()), (watched, "Value = 5"));
    let error = entries.iter().find(|entry| entry.severity == crate::LogSeverity::Error).unwrap();
    assert_eq!(error.locations.first().and_then(|location| location.node), Some(print_name), "{text}");
}

fn http_post(uri: &str, endpoint: &str, data: &str) -> (u16, String) {
    use std::io::{Read, Write};
    let address = uri.trim_start_matches("http://");
    let mut stream = std::net::TcpStream::connect(address).unwrap();
    let request = format!(
        "POST {endpoint} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{data}",
        data.len()
    );
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response.split_whitespace().nth(1).unwrap().parse().unwrap();
    let body = response.split_once("\r\n\r\n").map(|(_, body)| body.to_owned()).unwrap_or_default();
    (status, body)
}

fn run_with_debugger(code: &str) -> (Vec<String>, Vec<String>) {
    let lua = Lua::new();
    lua.load(RUNTIME).exec().unwrap();
    let requests = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let seen = requests.clone();
    let request = lua
        .create_function(move |lua, (uri, endpoint, _method, data): (String, String, mlua::Value, String)| {
            seen.lock().unwrap().push(endpoint.clone());
            let (status, body) = http_post(&uri, &endpoint, &data);
            let response = lua.create_table()?;
            response.set("Status", status)?;
            response.set("Data", body)?;
            Ok(response)
        })
        .unwrap();
    let http = lua.create_table().unwrap();
    http.set("Request", request).unwrap();
    lua.globals().set("HTTP", http).unwrap();
    lua.load("HTTPMethod = { POST = 1 }").exec().unwrap();
    lua.load(code).set_name("Server/Main.lua").exec().unwrap();
    lua.load("fire('Package.Load')").exec().unwrap();
    let log: Vec<String> = lua.globals().get("log").unwrap();
    let requests = requests.lock().unwrap().clone();
    (log, requests)
}

#[test]
fn breakpoints_pause_the_server_until_nanosflow_resumes() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let before = nodes.print("before");
    let sum = nodes.add("math/add");
    nodes.literal(sum, "a", 40);
    nodes.literal(sum, "b", 2);
    let stopped = nodes.add("debug/print");
    nodes.wire(sum, "result", stopped, "value");
    nodes.node(stopped).properties.insert("breakpoint".to_owned(), toml::Value::Boolean(true));
    let after = nodes.print("after");
    nodes.then(load, "then", before);
    nodes.then(before, "then", stopped);
    nodes.then(stopped, "then", after);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    assert_eq!(codes(&compiled), ["breakpoint-node"]);
    let graph = GraphDocument::load(&main).unwrap().manifest.id;
    let code = compiled.files["Server/Main.lua"].clone();
    assert!(code.contains(&format!("debug_break(\"{graph}/{stopped}\", \"Value\", 40 + 2)")), "{code}");

    let (sender, receiver) = std::sync::mpsc::channel();
    let debugger = crate::Debugger::start(0, move |event| {
        let _ = sender.send(event);
    })
    .unwrap();
    let code = code.replace(&format!(":{}", crate::DEBUGGER_PORT), &format!(":{}", debugger.port()));
    let resumer = debugger.clone();
    let watcher = std::thread::spawn(move || {
        let crate::DebuggerEvent::Paused(hit) = receiver.recv_timeout(std::time::Duration::from_secs(10)).unwrap() else {
            panic!("expected a pause");
        };
        assert!(resumer.paused().is_some());
        std::thread::sleep(std::time::Duration::from_millis(2500));
        resumer.resume(crate::ResumeMode::Continue);
        let resumed = receiver.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        (hit, resumed)
    });

    let (log, requests) = run_with_debugger(&code);
    assert_eq!(log, ["before", "42", "after"]);

    let (hit, resumed) = watcher.join().unwrap();
    assert_eq!((hit.graph, hit.node, hit.side.as_str()), (Some(graph), Some(stopped), "server"));
    assert_eq!(hit.values, [("Value".to_owned(), "42".to_owned())]);
    assert_eq!(resumed, crate::DebuggerEvent::Resumed);
    assert_eq!(requests, ["/pause", "/wait"]);
    assert!(debugger.paused().is_none());

    let client = Lua::new();
    client.load("HTTPMethod = { POST = 1 } sent = {} HTTP = { RequestAsync = function(uri, endpoint, method, data) sent[#sent + 1] = endpoint .. ' ' .. data end }").exec().unwrap();
    let helper = crate::helpers::source(crate::helpers::DEBUG_BREAK);
    client.load(format!("{helper}\ndebug_break(\"a/b\", \"Text\", 'say \"hi\"\\n')").as_str()).exec().unwrap();
    let sent: Vec<String> = client.globals().get("sent").unwrap();
    assert_eq!(sent, [r#"/hit {"location":"a/b","side":"client","values":[["Text","say \u0022hi\u0022\u000a"]],"stack":[]}"#]);
}

#[test]
fn stepping_pauses_at_the_next_node_that_runs() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let client = package.graph(ScriptContext::Client, "Hud", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let first = nodes.print("a");
    nodes.node(first).properties.insert("breakpoint".to_owned(), toml::Value::Boolean(true));
    let second = nodes.print("b");
    let third = nodes.print("c");
    nodes.then(load, "then", first);
    nodes.then(first, "then", second);
    nodes.then(second, "then", third);
    nodes.save(&main);
    let mut client_nodes = Nodes::default();
    let client_load = client_nodes.add("event/Package/Load");
    let client_print = client_nodes.print("hud");
    client_nodes.then(client_load, "then", client_print);
    client_nodes.save(&client);

    let compiled = package.compile();
    assert_clean(&compiled);
    let code = compiled.files["Server/Main.lua"].clone();
    let graph = GraphDocument::load(&main).unwrap().manifest.id;
    assert!(
        code.contains(&format!("\tif NANOSFLOW_STEP and #NANOSFLOW_STACK <= NANOSFLOW_STEP then\n\t\tdebug_break(\"{graph}/{second}\", \"Value\", \"b\")\n\tend\n")),
        "{code}"
    );
    assert!(!compiled.files["Client/Hud.lua"].contains("NANOSFLOW_STEP"));

    let (sender, receiver) = std::sync::mpsc::channel();
    let debugger = crate::Debugger::start(0, move |event| {
        let _ = sender.send(event);
    })
    .unwrap();
    let code = code.replace(&format!(":{}", crate::DEBUGGER_PORT), &format!(":{}", debugger.port()));
    let controller = debugger.clone();
    let watcher = std::thread::spawn(move || {
        let mut paused_at = Vec::new();
        let next = || receiver.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        for step in [crate::ResumeMode::Into, crate::ResumeMode::Continue] {
            let crate::DebuggerEvent::Paused(hit) = next() else {
                panic!("expected a pause");
            };
            paused_at.push(hit.node);
            controller.resume(step);
            assert_eq!(next(), crate::DebuggerEvent::Resumed);
        }
        paused_at
    });
    let (log, requests) = run_with_debugger(&code);
    assert_eq!(log, ["a", "b", "c"]);
    assert_eq!(watcher.join().unwrap(), [Some(first), Some(second)]);
    assert_eq!(requests, ["/pause", "/pause"]);
}

fn pin(id: &str, name: &str, type_name: &str) -> FunctionPin {
    FunctionPin {
        id: id.to_owned(),
        name: name.to_owned(),
        type_name: type_name.to_owned(),
    }
}

#[test]
fn macros_are_copied_into_every_place_that_uses_them() {
    let package = TestPackage::new();
    let twice = package.graph(ScriptContext::Shared, "Say Twice", GraphKind::Macro);
    update_function_signature(
        &twice,
        FunctionSignature {
            parameters: vec![pin("run", "Run", "exec"), pin("text", "Text", "string")],
            returns: vec![pin("done", "Done", "exec"), pin("first", "First Time", "exec"), pin("copy", "Copy", "string")],
            ..FunctionSignature::default()
        },
    )
    .unwrap();
    let twice_id = GraphDocument::load(&twice).unwrap().manifest.id;
    let mut body = Nodes::loaded(&twice);
    let inputs = body.nodes.iter().find(|node| node.node_type == "macro/inputs").unwrap().id;
    let outputs = body.nodes.iter().find(|node| node.node_type == "macro/outputs").unwrap().id;
    let first = body.add("debug/print");
    let second = body.add("debug/print");
    let once = body.add("flow/do-once");
    body.wire(inputs, "text", first, "value");
    body.wire(inputs, "text", second, "value");
    body.wire(inputs, "text", outputs, "copy");
    body.node(inputs).exec.insert("run".to_owned(), PinLink { node: first, pin: "exec".to_owned() });
    body.then(first, "then", second);
    body.then(second, "then", once);
    body.node(once).exec.insert("completed".to_owned(), PinLink { node: outputs, pin: "first".to_owned() });
    body.save(&twice);

    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let command = nodes.add("event/Package/Load");
    let a = nodes.add(&format!("macro/{twice_id}"));
    nodes.literal(a, "text", "a");
    let b = nodes.add(&format!("macro/{twice_id}"));
    nodes.literal(b, "text", "b");
    let first_time = nodes.print("first b");
    let after = nodes.add("debug/print");
    nodes.wire(b, "copy", after, "value");
    nodes.node(command).exec.insert("then".to_owned(), PinLink { node: a, pin: "run".to_owned() });
    nodes.node(a).exec.insert("first".to_owned(), PinLink { node: b, pin: "run".to_owned() });
    nodes.node(b).exec.insert("first".to_owned(), PinLink { node: first_time, pin: "exec".to_owned() });
    nodes.then(first_time, "then", after);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    assert!(compiled.files.keys().all(|file| !file.contains("SayTwice")), "{:?}", compiled.files.keys());
    let (lua, log) = run(&compiled, "Server", "fire('Package.Load')");
    assert_eq!(log, ["a", "a", "b", "b", "first b", "b"]);
    lua.load("fire('Package.Load')").exec().unwrap();
    let log: Vec<String> = lua.globals().get("log").unwrap();
    assert_eq!(log[6..], ["a", "a"]);

    let spans = &compiled.source_map["Server/Main.lua"];
    let main_id = GraphDocument::load(&main).unwrap().manifest.id;
    assert!(spans.iter().any(|span| span.graph == twice_id && span.node == first));
    assert!(spans.iter().any(|span| span.graph == main_id && span.node == after));
    assert!(spans.iter().all(|span| span.node != a && span.node != b));

    let mut looping = Nodes::loaded(&twice);
    let again = looping.add(&format!("macro/{twice_id}"));
    looping.then(second, "then", again);
    looping.save(&twice);
    let compiled = compile_package(catalog(), &package.path).unwrap();
    assert!(codes(&compiled).contains(&"macro-expansion"), "{:?}", codes(&compiled));
    let reported: Vec<Option<Id>> = compiled
        .problems
        .iter()
        .filter(|problem| problem.code == "macro-expansion")
        .map(|problem| problem.node)
        .collect();
    assert!(reported.contains(&Some(a)) && reported.contains(&Some(b)), "{reported:?}");
}

#[test]
fn libraries_export_functions_to_packages_that_require_them() {
    let package = TestPackage::new();
    let core = package.sibling("arena-core");
    let shared = context_root(&core, ScriptContext::Shared);
    std::fs::create_dir_all(&shared).unwrap();
    let heal = create_graph(&shared, "Heal Amount", GraphKind::Function).unwrap();
    update_function_signature(
        &heal,
        FunctionSignature {
            exported: true,
            parameters: vec![pin("base", "Base", "number")],
            returns: vec![pin("total", "Total", "number")],
            ..FunctionSignature::default()
        },
    )
    .unwrap();
    let entry = entry_of(&heal);
    let mut body = Nodes::loaded(&heal);
    let add = body.add("math/add");
    body.wire(entry, "base", add, "a");
    body.literal(add, "b", 25);
    let result = body.add("function/return");
    body.wire(add, "result", result, "total");
    body.then(entry, "then", result);
    body.save(&heal);
    let heal_id = GraphDocument::load(&heal).unwrap().manifest.id;

    let library = compile_package(catalog(), &core).unwrap();
    assert_clean(&library);
    for side in ["Server", "Client"] {
        assert!(
            library.files[&format!("{side}/Index.lua")].ends_with("Package.Export(\"ArenaCore\", {\n\tHealAmount = HealAmount,\n})\n"),
            "{}",
            library.files[&format!("{side}/Index.lua")]
        );
    }

    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let call = nodes.add(&format!("library/{heal_id}"));
    nodes.literal(call, "base", 5);
    let print = nodes.add("debug/print");
    nodes.wire(call, "total", print, "value");
    nodes.then(load, "then", call);
    nodes.then(call, "then", print);
    nodes.save(&main);

    let without = compile_package(catalog(), &package.path).unwrap();
    assert!(codes(&without).contains(&"missing-library-function"), "{:?}", codes(&without));

    package.require(&["arena-core"]);
    let game = package.compile();
    assert_clean(&game);
    assert!(game.files["Server/Main.lua"].contains("ArenaCore.HealAmount(5)"), "{}", game.files["Server/Main.lua"]);
    assert!(game.files["Package.toml"].contains("packages_requirements = [\"arena-core\"]"));
    let (_, log) = run_packages(&[&library, &game], "Server", "fire('Package.Load')");
    assert_eq!(log, ["30"]);

    create_graph(&shared, "Damage", GraphKind::Struct).unwrap();
    update_function_signature(
        &heal,
        FunctionSignature {
            exported: true,
            parameters: vec![pin("base", "Base", "Damage")],
            returns: vec![pin("total", "Total", "number")],
            ..FunctionSignature::default()
        },
    )
    .unwrap();
    let library = compile_package(catalog(), &core).unwrap();
    assert!(codes(&library).contains(&"export-project-type"), "{:?}", codes(&library));
}

#[test]
fn project_enums_become_lua_tables_with_switches_and_literals() {
    let package = TestPackage::new();
    let phase = package.graph(ScriptContext::Shared, "RoundPhase", GraphKind::Enum);
    let signature = |middle: &str| FunctionSignature {
        parameters: vec![pin("lobby", "Lobby", "integer"), pin("playing", middle, "integer"), pin("ended", "Ended", "integer")],
        ..FunctionSignature::default()
    };
    update_function_signature(&phase, signature("In Game")).unwrap();

    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let switch = nodes.add("flow/switch-enum/RoundPhase");
    nodes.literal(switch, "value", "playing");
    let lobby = nodes.print("lobby");
    let playing = nodes.print("playing");
    let other = nodes.print("other");
    let compare = nodes.add("debug/print");
    nodes.literal(compare, "value", "done");
    nodes.then(load, "then", switch);
    nodes.then(switch, "case_lobby", lobby);
    nodes.then(switch, "case_playing", playing);
    nodes.then(switch, "default", other);
    nodes.then(playing, "then", compare);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    assert_eq!(compiled.files["Shared/RoundPhase.lua"], "RoundPhase = {\n\tLobby = 1,\n\tInGame = 2,\n\tEnded = 3,\n}\n");
    assert!(compiled.files["Server/Main.lua"].contains("RoundPhase.InGame == RoundPhase.Lobby"), "{}", compiled.files["Server/Main.lua"]);
    assert!(compiled.files["Server/Index.lua"].starts_with("Package.Require(\"Shared/RoundPhase.lua\")\n"));
    let (_, log) = run(&compiled, "Server", "fire('Package.Load')");
    assert_eq!(log, ["playing", "done"]);

    update_function_signature(&phase, signature("Running")).unwrap();
    let renamed = package.compile();
    assert_clean(&renamed);
    let (_, log) = run(&renamed, "Server", "fire('Package.Load')");
    assert_eq!(log, ["playing", "done"]);

    let mut broken = Nodes::loaded(&main);
    broken.literal(switch, "value", "missing");
    broken.save(&main);
    assert!(codes(&compile_package(catalog(), &package.path).unwrap()).contains(&"invalid-literal"));
}

#[test]
fn macro_variables_get_their_own_copy_in_every_place() {
    let package = TestPackage::new();
    let counter = package.graph(ScriptContext::Shared, "Count Up", GraphKind::Macro);
    update_function_signature(
        &counter,
        FunctionSignature {
            parameters: vec![pin("run", "Run", "exec")],
            returns: vec![pin("done", "Done", "exec")],
            ..FunctionSignature::default()
        },
    )
    .unwrap();
    let count = VariableDocument {
        id: Id::generate(),
        name: "Count".to_owned(),
        type_name: "integer".to_owned(),
        default: Some(toml::Value::Integer(0)),
        description: String::new(),
        sync: false,
    };
    apply_variable_changes(&counter, std::slice::from_ref(&count), &[]).unwrap();
    let counter_id = GraphDocument::load(&counter).unwrap().manifest.id;
    let mut body = Nodes::loaded(&counter);
    let inputs = body.nodes.iter().find(|node| node.node_type == "macro/inputs").unwrap().id;
    let outputs = body.nodes.iter().find(|node| node.node_type == "macro/outputs").unwrap().id;
    let get = body.add(&format!("variable/get/{}", count.id));
    let add = body.add("math/add");
    body.wire(get, "value", add, "a");
    body.literal(add, "b", 1);
    let set = body.add(&format!("variable/set/{}", count.id));
    body.wire(add, "result", set, "value");
    let print = body.add("debug/print");
    body.wire(set, "value", print, "value");
    body.node(inputs).exec.insert("run".to_owned(), PinLink { node: set, pin: "exec".to_owned() });
    body.then(set, "then", print);
    body.node(print).exec.insert("then".to_owned(), PinLink { node: outputs, pin: "done".to_owned() });
    body.save(&counter);

    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let first = nodes.add(&format!("macro/{counter_id}"));
    let second = nodes.add(&format!("macro/{counter_id}"));
    nodes.node(load).exec.insert("then".to_owned(), PinLink { node: first, pin: "run".to_owned() });
    nodes.node(first).exec.insert("done".to_owned(), PinLink { node: second, pin: "run".to_owned() });
    nodes.save(&main);

    let zombie = create_class(&package.root(ScriptContext::Shared), "Zombie", "Character").unwrap();
    let mut class_graph = Nodes::default();
    let spawn = class_graph.add("class-event/Spawn");
    let inside = class_graph.add(&format!("macro/{counter_id}"));
    class_graph.node(spawn).exec.insert("then".to_owned(), PinLink { node: inside, pin: "run".to_owned() });
    class_graph.save(&zombie);

    let compiled = package.compile();
    assert_clean(&compiled);
    let (lua, log) = run(&compiled, "Server", "fire('Package.Load')");
    assert_eq!(log, ["1", "1"]);
    lua.load("fire('Package.Load')").exec().unwrap();
    let log: Vec<String> = lua.globals().get("log").unwrap();
    assert_eq!(log, ["1", "1", "2", "2"]);

    let (_, log) = run(&compiled, "Server", "Zombie() Zombie()");
    assert_eq!(log, ["1", "2"], "{}", compiled.files["Shared/Zombie.lua"]);
}

#[test]
fn event_dispatchers_bind_call_and_unbind_per_instance() {
    let package = TestPackage::new();
    let zombie = create_class(&package.root(ScriptContext::Shared), "Zombie", "Character").unwrap();
    let zombie_id = GraphDocument::load(&zombie).unwrap().manifest.id;
    let died = create_dispatcher(&zombie, "On Died").unwrap();
    update_function_signature(
        &died,
        FunctionSignature {
            parameters: vec![pin("damage", "Damage", "number")],
            ..FunctionSignature::default()
        },
    )
    .unwrap();
    let died_id = GraphDocument::load(&died).unwrap().manifest.id;

    let mut class_graph = Nodes::default();
    let spawn_event = class_graph.add("class-event/Spawn");
    let inner_bind = class_graph.add(&format!("dispatcher/bind/{died_id}"));
    let inner = class_graph.print("inner");
    class_graph.then(spawn_event, "then", inner_bind);
    class_graph.then(inner_bind, "event", inner);
    class_graph.save(&zombie);

    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let spawn = nodes.add(&format!("class/spawn/{zombie_id}"));
    let cast = nodes.add("cast/Zombie");
    nodes.wire(spawn, "return_value", cast, "object");
    let bind = nodes.add(&format!("dispatcher/bind/{died_id}"));
    nodes.wire(cast, "result", bind, "self");
    let outer = nodes.add("debug/print");
    nodes.wire(bind, "event_damage", outer, "value");
    let call = |nodes: &mut Nodes, damage: i64| {
        let call = nodes.add(&format!("dispatcher/call/{died_id}"));
        nodes.wire(cast, "result", call, "self");
        nodes.literal(call, "damage", damage);
        call
    };
    let first = call(&mut nodes, 5);
    let second = call(&mut nodes, 7);
    let unbind = nodes.add(&format!("dispatcher/unbind/{died_id}"));
    nodes.wire(cast, "result", unbind, "self");
    nodes.wire(bind, "binding", unbind, "binding");
    let third = call(&mut nodes, 9);
    let clear = nodes.add(&format!("dispatcher/unbind-all/{died_id}"));
    nodes.wire(cast, "result", clear, "self");
    let fourth = call(&mut nodes, 11);
    nodes.then(load, "then", spawn);
    nodes.then(spawn, "then", cast);
    nodes.then(cast, "then", bind);
    nodes.then(bind, "event", outer);
    nodes.then(bind, "then", first);
    nodes.then(first, "then", second);
    nodes.then(second, "then", unbind);
    nodes.then(unbind, "then", third);
    nodes.then(third, "then", clear);
    nodes.then(clear, "then", fourth);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    let class_code = &compiled.files["Shared/Zombie.lua"];
    assert!(class_code.contains("function Zombie:BindOnDied(callback)"), "{class_code}");
    assert!(class_code.contains("self:BindOnDied(function(damage)") || class_code.contains("self:BindOnDied(function("), "{class_code}");
    let main_code = &compiled.files["Server/Main.lua"];
    assert!(main_code.contains(":CallOnDied(5)"), "{main_code}");
    let (_, log) = run(&compiled, "Server", "fire('Package.Load')");
    assert_eq!(log, ["inner", "5", "inner", "7", "inner"], "{main_code}");
}

type Pause = (Option<Id>, Vec<Option<Id>>);

fn pauses_for(code: &str, modes: Vec<crate::ResumeMode>) -> (Vec<String>, Vec<Pause>) {
    let (sender, receiver) = std::sync::mpsc::channel();
    let debugger = crate::Debugger::start(0, move |event| {
        let _ = sender.send(event);
    })
    .unwrap();
    let code = code.replace(&format!(":{}", crate::DEBUGGER_PORT), &format!(":{}", debugger.port()));
    let controller = debugger.clone();
    let watcher = std::thread::spawn(move || {
        let mut pauses = Vec::new();
        let next = || receiver.recv_timeout(std::time::Duration::from_secs(10)).unwrap();
        for mode in modes {
            let crate::DebuggerEvent::Paused(hit) = next() else {
                panic!("expected a pause");
            };
            pauses.push((hit.node, hit.stack.iter().map(|frame| frame.node).collect()));
            controller.resume(mode);
            assert_eq!(next(), crate::DebuggerEvent::Resumed);
        }
        pauses
    });
    let (log, _) = run_with_debugger(&code);
    (log, watcher.join().unwrap())
}

#[test]
fn step_into_over_and_out_follow_the_call_depth() {
    use crate::ResumeMode::{Continue, Into, Out, Over};
    let package = TestPackage::new();
    let inner = package.graph(ScriptContext::Shared, "Inner", GraphKind::Function);
    let inner_id = GraphDocument::load(&inner).unwrap().manifest.id;
    let entry = entry_of(&inner);
    let mut body = Nodes::loaded(&inner);
    let inner_first = body.print("inner 1");
    let inner_second = body.print("inner 2");
    body.then(entry, "then", inner_first);
    body.then(inner_first, "then", inner_second);
    body.save(&inner);

    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let first = nodes.print("a");
    nodes.node(first).properties.insert("breakpoint".to_owned(), toml::Value::Boolean(true));
    let call = nodes.add(&format!("function/{inner_id}"));
    let after = nodes.print("b");
    let last = nodes.print("c");
    nodes.then(load, "then", first);
    nodes.then(first, "then", call);
    nodes.then(call, "then", after);
    nodes.then(after, "then", last);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    let function_code = &compiled.files["Shared/Inner.lua"];
    assert!(function_code.contains("\tdebug_enter(\""), "{function_code}");
    assert!(function_code.trim_end().ends_with("\tdebug_leave()\nend"), "{function_code}");
    let code = format!("{function_code}\n{}", compiled.files["Server/Main.lua"]);

    let (log, pauses) = pauses_for(&code, vec![Into, Into, Out, Continue]);
    assert_eq!(log, ["a", "inner 1", "inner 2", "b", "c"]);
    let nodes_paused: Vec<Option<Id>> = pauses.iter().map(|(node, _)| *node).collect();
    assert_eq!(nodes_paused, [Some(first), Some(call), Some(inner_first), Some(after)]);
    assert_eq!(pauses[2].1, [Some(load), Some(entry)]);
    assert_eq!(pauses[3].1, [Some(load)]);

    let (log, pauses) = pauses_for(&code, vec![Over, Over, Continue]);
    assert_eq!(log, ["a", "inner 1", "inner 2", "b", "c"]);
    let nodes_paused: Vec<Option<Id>> = pauses.iter().map(|(node, _)| *node).collect();
    assert_eq!(nodes_paused, [Some(first), Some(call), Some(after)]);
}

#[test]
fn interfaces_call_methods_on_every_class_that_implements_them() {
    let package = TestPackage::new();
    let shared = package.root(ScriptContext::Shared);
    let damageable = create_graph(&shared, "Damageable", GraphKind::Interface).unwrap();
    let take_damage = nf_project::InterfaceFunction {
        id: "take".to_owned(),
        name: "Take Damage".to_owned(),
        description: String::new(),
        parameters: vec![pin("amount", "Amount", "number")],
        returns: vec![pin("left", "Left", "number")],
    };
    nf_project::update_interface(&damageable, "", vec![take_damage.clone()]).unwrap();
    let interface_id = GraphDocument::load(&damageable).unwrap().manifest.id;

    let zombie = create_class(&shared, "Zombie", "Character").unwrap();
    let zombie_id = GraphDocument::load(&zombie).unwrap().manifest.id;
    let created = nf_project::add_interface_methods(&zombie, std::slice::from_ref(&take_damage)).unwrap();
    assert_eq!(created.len(), 1);
    assert!(nf_project::add_interface_methods(&zombie, std::slice::from_ref(&take_damage)).unwrap().is_empty());
    nf_project::update_class(
        &zombie,
        nf_project::ClassSettings {
            parent: "Character",
            description: "",
            interfaces: &["Damageable".to_owned()],
            exported: false,
        },
    )
    .unwrap();
    let method = &created[0];
    let entry = entry_of(method);
    let mut body = Nodes::loaded(method);
    let double = body.add("math/multiply");
    body.wire(entry, "amount", double, "a");
    body.literal(double, "b", 2);
    let result = body.add("function/return");
    body.wire(double, "result", result, "left");
    body.then(entry, "then", result);
    body.save(method);

    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let spawn = nodes.add(&format!("class/spawn/{zombie_id}"));
    let call = nodes.add(&format!("interface/call/{interface_id}/take"));
    nodes.wire(spawn, "return_value", call, "self");
    nodes.literal(call, "amount", 5);
    let print_left = nodes.add("debug/print");
    nodes.wire(call, "left", print_left, "value");
    let other = nodes.add(&format!("interface/call/{interface_id}/take"));
    nodes.literal(other, "self", "not an entity");
    nodes.literal(other, "amount", 1);
    let print_other = nodes.add("debug/print");
    nodes.wire(other, "left", print_other, "value");
    let implements = nodes.add(&format!("interface/implements/{interface_id}"));
    nodes.wire(spawn, "return_value", implements, "self");
    let print_implements = nodes.add("debug/print");
    nodes.wire(implements, "result", print_implements, "value");
    nodes.then(load, "then", spawn);
    nodes.then(spawn, "then", call);
    nodes.then(call, "then", print_left);
    nodes.then(print_left, "then", other);
    nodes.then(other, "then", print_other);
    nodes.then(print_other, "then", print_implements);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    assert!(compiled.files["Shared/Zombie.lua"].contains("Zombie.ImplementsDamageable = true"));
    let (_, log) = run(&compiled, "Server", "fire('Package.Load')");
    assert_eq!(log, ["10", "nil", "true"], "{}", compiled.files["Server/Main.lua"]);

    let heal = nf_project::InterfaceFunction {
        id: "heal".to_owned(),
        name: "Heal".to_owned(),
        description: String::new(),
        parameters: Vec::new(),
        returns: Vec::new(),
    };
    nf_project::update_interface(&damageable, "", vec![take_damage, heal]).unwrap();
    let missing = compile_package(catalog(), &package.path).unwrap();
    assert!(codes(&missing).contains(&"interface-method-missing"), "{:?}", codes(&missing));
}

#[test]
fn exported_classes_can_be_spawned_used_and_inherited_by_other_packages() {
    let package = TestPackage::new();
    let core = package.sibling("core");
    let core_shared = context_root(&core, ScriptContext::Shared);
    std::fs::create_dir_all(&core_shared).unwrap();
    let zombie = create_class(&core_shared, "Zombie", "Character").unwrap();
    let zombie_id = GraphDocument::load(&zombie).unwrap().manifest.id;
    let rage = VariableDocument {
        id: Id::generate(),
        name: "Rage".to_owned(),
        type_name: "number".to_owned(),
        default: Some(toml::Value::Integer(3)),
        description: String::new(),
        sync: false,
    };
    apply_variable_changes(&zombie, std::slice::from_ref(&rage), &[]).unwrap();
    let roar = create_method(&zombie, "Roar").unwrap();
    update_function_signature(
        &roar,
        FunctionSignature {
            returns: vec![pin("sound", "Sound", "string")],
            ..FunctionSignature::default()
        },
    )
    .unwrap();
    let roar_id = GraphDocument::load(&roar).unwrap().manifest.id;
    let entry = entry_of(&roar);
    let mut body = Nodes::loaded(&roar);
    let result = body.add("function/return");
    body.literal(result, "sound", "grr");
    body.then(entry, "then", result);
    body.save(&roar);
    let export = |exported: bool| {
        nf_project::update_class(
            &zombie,
            nf_project::ClassSettings {
                parent: "Character",
                description: "",
                interfaces: &[],
                exported,
            },
        )
        .unwrap();
    };
    export(true);

    package.require(&["core"]);
    let shared = package.root(ScriptContext::Shared);
    let boss = create_class(&shared, "ZombieBoss", "Zombie").unwrap();
    let boss_id = GraphDocument::load(&boss).unwrap().manifest.id;
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let spawn = nodes.add(&format!("class/spawn/{zombie_id}"));
    let call = nodes.add(&format!("method/{roar_id}"));
    nodes.wire(spawn, "return_value", call, "self");
    let print_roar = nodes.add("debug/print");
    nodes.wire(call, "sound", print_roar, "value");
    let read = nodes.add(&format!("variable/get/{}", rage.id));
    nodes.wire(spawn, "return_value", read, "self");
    let print_rage = nodes.add("debug/print");
    nodes.wire(read, "value", print_rage, "value");
    let spawn_boss = nodes.add(&format!("class/spawn/{boss_id}"));
    let boss_roar = nodes.add(&format!("method/{roar_id}"));
    nodes.wire(spawn_boss, "return_value", boss_roar, "self");
    let print_boss = nodes.add("debug/print");
    nodes.wire(boss_roar, "sound", print_boss, "value");
    nodes.then(load, "then", spawn);
    nodes.then(spawn, "then", call);
    nodes.then(call, "then", print_roar);
    nodes.then(print_roar, "then", print_rage);
    nodes.then(print_rage, "then", spawn_boss);
    nodes.then(spawn_boss, "then", boss_roar);
    nodes.then(boss_roar, "then", print_boss);
    nodes.save(&main);

    let library = compile_package(catalog(), &core).unwrap();
    assert_clean(&library);
    let game = package.compile();
    assert_clean(&game);
    assert!(game.files["Shared/ZombieBoss.lua"].starts_with("ZombieBoss = Zombie.Inherit(\"ZombieBoss\")"), "{}", game.files["Shared/ZombieBoss.lua"]);
    let (_, log) = run_packages(&[&library, &game], "Server", "fire('Package.Load')");
    assert_eq!(log, ["grr", "3", "grr"], "{}", game.files["Server/Main.lua"]);

    export(false);
    let hidden = compile_package(catalog(), &package.path).unwrap();
    assert!(codes(&hidden).contains(&"missing-class"), "{:?}", codes(&hidden));
}

#[test]
fn timelines_animate_tracks_and_finish() {
    let package = TestPackage::new();
    let main = package.graph(ScriptContext::Server, "Main", GraphKind::EventGraph);
    let mut nodes = Nodes::default();
    let load = nodes.add("event/Package/Load");
    let timeline = nodes.add("flow/timeline");
    let mut track = toml::map::Map::new();
    track.insert("id".to_owned(), "alpha".into());
    track.insert("name".to_owned(), "Alpha".into());
    track.insert(
        "keys".to_owned(),
        toml::Value::Array(vec![
            toml::Value::Array(vec![0.0.into(), 0.into()]),
            toml::Value::Array(vec![0.1.into(), 30.into()]),
        ]),
    );
    let properties = &mut nodes.node(timeline).properties;
    properties.insert("length".to_owned(), 0.1.into());
    properties.insert("tracks".to_owned(), toml::Value::Array(vec![toml::Value::Table(track)]));
    let print_value = nodes.add("debug/print");
    nodes.wire(timeline, "track_alpha", print_value, "value");
    let done = nodes.print("done");
    nodes.node(load).exec.insert("then".to_owned(), PinLink { node: timeline, pin: "play_from_start".to_owned() });
    nodes.then(timeline, "update", print_value);
    nodes.then(timeline, "finished", done);
    nodes.save(&main);

    let compiled = package.compile();
    assert_clean(&compiled);
    let code = &compiled.files["Server/Main.lua"];
    assert!(code.contains("timeline_run(timeline, 1, \"start\", 0.1, false, timeline_update, timeline_finished)"), "{code}");
    let ticks = "for _ = 1, 10 do for _, timer in ipairs(timers) do if not timer.cleared then timer.callback() end end end";
    let (_, log) = run(&compiled, "Server", &format!("fire('Package.Load') {ticks}"));
    assert_eq!(log.last().map(String::as_str), Some("done"), "{code}");
    let values: Vec<f64> = log[..log.len() - 1].iter().map(|value| value.parse().unwrap()).collect();
    assert_eq!(values.len(), 3, "{log:?}");
    for (value, expected) in values.iter().zip([10.0, 20.0, 30.0]) {
        assert!((value - expected).abs() < 0.001, "{log:?}");
    }
}
