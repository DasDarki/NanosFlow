use nf_api::TypeExpr;

use crate::catalog::{DynamicPins, NodeDefinition, NodeKind, Side};
use crate::generics::type_parameter;
use crate::pin::PinDefinition;

fn named(name: &str) -> TypeExpr {
    TypeExpr::named(name)
}

fn exec_in() -> PinDefinition {
    PinDefinition::exec("exec", "")
}

fn exec_out(name: &str, label: &str) -> PinDefinition {
    PinDefinition::exec(name, label)
}

fn data(name: &str, label: &str, type_name: &str) -> PinDefinition {
    PinDefinition::data(name, label, named(type_name))
}

struct Builtin {
    id: &'static str,
    title: &'static str,
    category: &'static str,
    kind: NodeKind,
    description: &'static str,
    keywords: &'static [&'static str],
}

impl Builtin {
    fn with_pins(self, inputs: Vec<PinDefinition>, outputs: Vec<PinDefinition>) -> NodeDefinition {
        NodeDefinition {
            id: self.id.to_owned(),
            title: self.title.to_owned(),
            subtitle: None,
            category: self.category.to_owned(),
            kind: self.kind,
            side: Side::Both,
            description: self.description.to_owned(),
            inputs,
            outputs,
            keywords: self.keywords.iter().map(|keyword| (*keyword).to_owned()).collect(),
            api: None,
            context_warnings: Vec::new(),
            callbacks: Vec::new(),
            dynamic: None,
        }
    }
}

fn binary(
    id: &'static str,
    title: &'static str,
    category: &'static str,
    operand: &str,
    result: &str,
    keywords: &'static [&'static str],
) -> NodeDefinition {
    Builtin {
        id,
        title,
        category,
        kind: NodeKind::Operator,
        description: "",
        keywords,
    }
    .with_pins(
        vec![data("a", "", operand), data("b", "", operand)],
        vec![data("result", "", result)],
    )
}

fn with_dynamic(mut definition: NodeDefinition, dynamic: DynamicPins) -> NodeDefinition {
    definition.dynamic = Some(dynamic);
    definition
}

fn switch_node(id: &'static str, title: &'static str, value_type: &str, description: &'static str) -> NodeDefinition {
    with_dynamic(
        Builtin {
            id,
            title,
            category: "Flow Control",
            kind: NodeKind::FlowControl,
            description,
            keywords: &["switch", "case", "match", "select"],
        }
        .with_pins(
            vec![exec_in(), data("value", "Value", value_type)],
            vec![exec_out("default", "Default")],
        ),
        DynamicPins::Cases,
    )
}

fn array_pin(name: &str, label: &str) -> PinDefinition {
    PinDefinition::data(name, label, TypeExpr::array(named("any")))
}

fn generic_array(name: &str, label: &str) -> PinDefinition {
    PinDefinition::data(name, label, TypeExpr::array(type_parameter()))
}

fn wildcard(name: &str, label: &str) -> PinDefinition {
    PinDefinition::data(name, label, type_parameter())
}

fn pure(
    id: &'static str,
    title: &'static str,
    category: &'static str,
    description: &'static str,
    keywords: &'static [&'static str],
    inputs: Vec<PinDefinition>,
    outputs: Vec<PinDefinition>,
) -> NodeDefinition {
    Builtin {
        id,
        title,
        category,
        kind: NodeKind::PureFunction,
        description,
        keywords,
    }
    .with_pins(inputs, outputs)
}

fn exec_node(
    id: &'static str,
    title: &'static str,
    category: &'static str,
    description: &'static str,
    keywords: &'static [&'static str],
    inputs: Vec<PinDefinition>,
    outputs: Vec<PinDefinition>,
) -> NodeDefinition {
    let mut all_inputs = vec![exec_in()];
    all_inputs.extend(inputs);
    let mut all_outputs = vec![exec_out("then", "")];
    all_outputs.extend(outputs);
    Builtin {
        id,
        title,
        category,
        kind: NodeKind::Function,
        description,
        keywords,
    }
    .with_pins(all_inputs, all_outputs)
}

fn literal(id: &'static str, title: &'static str, type_name: &str, default: &str) -> NodeDefinition {
    Builtin {
        id,
        title,
        category: "Literals",
        kind: NodeKind::Literal,
        description: "A constant value that can be connected to several inputs.",
        keywords: &["constant", "value"],
    }
    .with_pins(
        vec![data("value", "", type_name).with_default(default)],
        vec![data("result", "", type_name)],
    )
}

pub const COMMENT: &str = "comment";
pub const REROUTE: &str = "flow/reroute";
pub const EXEC_REROUTE: &str = "flow/reroute-exec";

pub fn builtin_definitions() -> Vec<NodeDefinition> {
    vec![
        Builtin {
            id: COMMENT,
            title: "Comment",
            category: "Utilities",
            kind: NodeKind::Comment,
            description: "A resizable box with a note. Moving it also moves the nodes inside. Select nodes and press C to wrap them.",
            keywords: &["note", "group", "box", "frame", "comment"],
        }
        .with_pins(Vec::new(), Vec::new()),
        Builtin {
            id: REROUTE,
            title: "Reroute",
            category: "Utilities",
            kind: NodeKind::Reroute,
            description: "Passes a value through unchanged, so wires can be routed around other nodes. Double-click a wire to add one.",
            keywords: &["reroute", "knot", "wire", "route"],
        }
        .with_pins(vec![wildcard("value", "")], vec![wildcard("value", "")]),
        Builtin {
            id: EXEC_REROUTE,
            title: "Reroute (Execution)",
            category: "Utilities",
            kind: NodeKind::Reroute,
            description: "Passes execution through unchanged, so execution wires can be routed around other nodes.",
            keywords: &["reroute", "knot", "wire", "route"],
        }
        .with_pins(vec![exec_in()], vec![exec_out("then", "")]),
        Builtin {
            id: "flow/branch",
            title: "Branch",
            category: "Flow Control",
            kind: NodeKind::FlowControl,
            description: "Continues with True or False depending on the condition.",
            keywords: &["if", "else", "condition"],
        }
        .with_pins(
            vec![exec_in(), data("condition", "Condition", "boolean")],
            vec![exec_out("true", "True"), exec_out("false", "False")],
        ),
        with_dynamic(
            Builtin {
                id: "flow/timeline",
                title: "Timeline",
                category: "Flow Control",
                kind: NodeKind::FlowControl,
                description: "Animates values over time. Update runs about 30 times per second while it plays, Finished runs at the end. Edit its length and curves in the details panel.",
                keywords: &["timeline", "animate", "tween", "curve", "lerp", "over time"],
            }
            .with_pins(
                vec![
                    PinDefinition::exec("play", "Play"),
                    PinDefinition::exec("play_from_start", "Play from Start"),
                    PinDefinition::exec("stop", "Stop"),
                    PinDefinition::exec("reverse", "Reverse"),
                ],
                vec![exec_out("update", "Update"), exec_out("finished", "Finished"), data("time", "Time", "number")],
            ),
            DynamicPins::Tracks,
        ),
        with_dynamic(
            Builtin {
                id: "flow/sequence",
                title: "Sequence",
                category: "Flow Control",
                kind: NodeKind::FlowControl,
                description: "Runs every output one after another. Use the plus button on the node to add outputs.",
                keywords: &["then", "order"],
            }
            .with_pins(vec![exec_in()], Vec::new()),
            DynamicPins::ExecOutputs {
                prefix: "then_".to_owned(),
                label: "Then".to_owned(),
                minimum: 1,
                initial: 2,
                fallback: 3,
            },
        ),
        Builtin {
            id: "flow/for-loop",
            title: "For Loop",
            category: "Flow Control",
            kind: NodeKind::FlowControl,
            description: "Runs the loop body once for every index from first to last, both included.",
            keywords: &["for", "repeat", "loop"],
        }
        .with_pins(
            vec![
                exec_in(),
                data("first_index", "First Index", "integer").with_default("1"),
                data("last_index", "Last Index", "integer").with_default("10"),
            ],
            vec![
                exec_out("loop_body", "Loop Body"),
                data("index", "Index", "integer"),
                exec_out("completed", "Completed"),
            ],
        ),
        Builtin {
            id: "flow/for-each",
            title: "For Each",
            category: "Flow Control",
            kind: NodeKind::FlowControl,
            description: "Runs the loop body once for every element of an array.",
            keywords: &["foreach", "ipairs", "loop", "array"],
        }
        .with_pins(
            vec![exec_in(), generic_array("array", "Array")],
            vec![
                exec_out("loop_body", "Loop Body"),
                wildcard("element", "Element"),
                data("index", "Index", "integer"),
                exec_out("completed", "Completed"),
            ],
        ),
        Builtin {
            id: "flow/while",
            title: "While Loop",
            category: "Flow Control",
            kind: NodeKind::FlowControl,
            description: "Runs the loop body as long as the condition is true. The condition is checked before every iteration.",
            keywords: &["while", "loop", "repeat"],
        }
        .with_pins(
            vec![exec_in(), data("condition", "Condition", "boolean")],
            vec![exec_out("loop_body", "Loop Body"), exec_out("completed", "Completed")],
        ),
        Builtin {
            id: "flow/delay",
            title: "Delay",
            category: "Flow Control",
            kind: NodeKind::FlowControl,
            description: "Waits the given number of seconds before continuing. Uses Timer.SetTimeout.",
            keywords: &["wait", "timer", "sleep", "timeout"],
        }
        .with_pins(
            vec![exec_in(), data("seconds", "Seconds", "number").with_default("1")],
            vec![exec_out("completed", "Completed")],
        ),
        Builtin {
            id: "flow/return",
            title: "Return",
            category: "Flow Control",
            kind: NodeKind::FlowControl,
            description: "Stops the current event. Returning false from some events cancels them.",
            keywords: &["exit", "stop", "cancel"],
        }
        .with_pins(
            vec![exec_in(), data("value", "Value", "any").with_default("nil")],
            Vec::new(),
        ),
        Builtin {
            id: "debug/print",
            title: "Print",
            category: "Debug",
            kind: NodeKind::Function,
            description: "Writes a value to the server or client console. Uses Console.Log.",
            keywords: &["log", "console", "debug", "print string", "output"],
        }
        .with_pins(
            vec![exec_in(), data("value", "Value", "any").with_default("\"Hello\"")],
            vec![exec_out("then", "")],
        ),
        binary("logic/and", "AND", "Logic", "boolean", "boolean", &["and", "&&", "both"]),
        binary("logic/or", "OR", "Logic", "boolean", "boolean", &["or", "||", "either"]),
        Builtin {
            id: "logic/not",
            title: "NOT",
            category: "Logic",
            kind: NodeKind::Operator,
            description: "Inverts a boolean.",
            keywords: &["not", "!", "invert", "negate"],
        }
        .with_pins(vec![data("a", "", "boolean")], vec![data("result", "", "boolean")]),
        binary("compare/equal", "Equal (==)", "Comparison", "any", "boolean", &["==", "equals", "same"]),
        binary("compare/not-equal", "Not Equal (~=)", "Comparison", "any", "boolean", &["~=", "!=", "different"]),
        binary("compare/less", "Less (<)", "Comparison", "number", "boolean", &["<", "smaller"]),
        binary("compare/less-equal", "Less or Equal (<=)", "Comparison", "number", "boolean", &["<="]),
        binary("compare/greater", "Greater (>)", "Comparison", "number", "boolean", &[">", "bigger"]),
        binary("compare/greater-equal", "Greater or Equal (>=)", "Comparison", "number", "boolean", &[">="]),
        binary("math/add", "Add (+)", "Math", "number", "number", &["+", "plus", "sum"]),
        binary("math/subtract", "Subtract (-)", "Math", "number", "number", &["-", "minus"]),
        binary("math/multiply", "Multiply (*)", "Math", "number", "number", &["*", "times"]),
        binary("math/divide", "Divide (/)", "Math", "number", "number", &["/"]),
        binary("math/modulo", "Modulo (%)", "Math", "number", "number", &["%", "remainder", "mod"]),
        binary("math/power", "Power (^)", "Math", "number", "number", &["^", "exponent", "pow"]),
        Builtin {
            id: "string/concat",
            title: "Append",
            category: "String",
            kind: NodeKind::Operator,
            description: "Joins two strings with Lua's .. operator.",
            keywords: &["..", "concat", "join", "combine"],
        }
        .with_pins(
            vec![data("a", "", "string"), data("b", "", "string")],
            vec![data("result", "", "string")],
        ),
        Builtin {
            id: "string/to-string",
            title: "To String",
            category: "String",
            kind: NodeKind::PureFunction,
            description: "Converts any value to its text representation with tostring.",
            keywords: &["tostring", "convert", "text"],
        }
        .with_pins(vec![data("value", "Value", "any")], vec![data("result", "", "string")]),
        Builtin {
            id: "string/to-number",
            title: "To Number",
            category: "String",
            kind: NodeKind::PureFunction,
            description: "Parses a number from text with tonumber. Returns nil when the text is not a number.",
            keywords: &["tonumber", "parse", "convert"],
        }
        .with_pins(
            vec![data("value", "Value", "string")],
            vec![PinDefinition::data("result", "", TypeExpr::Optional(Box::new(named("number"))))],
        ),
        literal("literal/boolean", "Boolean", "boolean", "false"),
        switch_node("flow/switch-integer", "Switch on Integer", "integer", "Continues with the output whose value matches, or with Default. Add cases with the plus button on the node."),
        switch_node("flow/switch-string", "Switch on String", "string", "Continues with the output whose text matches exactly, or with Default. Add cases with the plus button on the node."),
        Builtin {
            id: "flow/select",
            title: "Select",
            category: "Flow Control",
            kind: NodeKind::PureFunction,
            description: "Picks one of two values depending on a condition.",
            keywords: &["select", "ternary", "if", "choose", "pick"],
        }
        .with_pins(
            vec![data("condition", "Condition", "boolean"), wildcard("if_true", "If True"), wildcard("if_false", "If False")],
            vec![wildcard("value", "Value")],
        ),
        Builtin {
            id: "flow/do-once",
            title: "Do Once",
            category: "Flow Control",
            kind: NodeKind::FlowControl,
            description: "Lets execution through only the first time it is reached while the package runs.",
            keywords: &["once", "first", "single"],
        }
        .with_pins(vec![exec_in()], vec![exec_out("completed", "Completed")]),
        Builtin {
            id: "flow/flip-flop",
            title: "Flip Flop",
            category: "Flow Control",
            kind: NodeKind::FlowControl,
            description: "Alternates between A and B every time it runs, starting with A.",
            keywords: &["toggle", "alternate", "switch"],
        }
        .with_pins(
            vec![exec_in()],
            vec![exec_out("a", "A"), exec_out("b", "B"), data("is_a", "Is A", "boolean")],
        ),
        Builtin {
            id: "flow/break",
            title: "Break Loop",
            category: "Flow Control",
            kind: NodeKind::FlowControl,
            description: "Stops the loop it is in and continues with its Completed output.",
            keywords: &["break", "stop", "exit", "loop"],
        }
        .with_pins(vec![exec_in()], Vec::new()),
        Builtin {
            id: "flow/is-valid",
            title: "Is Valid",
            category: "Flow Control",
            kind: NodeKind::FlowControl,
            description: "Continues with Is Valid when the entity exists and was not destroyed, otherwise with Is Not Valid.",
            keywords: &["valid", "exists", "nil", "destroyed", "isvalid"],
        }
        .with_pins(
            vec![exec_in(), data("object", "Object", "Entity")],
            vec![exec_out("valid", "Is Valid"), exec_out("invalid", "Is Not Valid")],
        ),
        with_dynamic(
            pure(
                "array/of",
                "Make Array",
                "Array",
                "A new array with the connected elements. Use the plus button on the node to add elements.",
                &["make", "list", "create", "new", "{}"],
                vec![],
                vec![generic_array("result", "Array")],
            ),
            DynamicPins::DataInputs {
                prefix: "element_".to_owned(),
                label: "Element".to_owned(),
                minimum: 1,
                initial: 2,
                fallback: 2,
            },
        ),
        pure("array/make", "Empty Array", "Array", "A new array without elements.", &["new", "list", "create", "{}"], vec![], vec![array_pin("result", "Array")]),
        exec_node(
            "array/add",
            "Add",
            "Array",
            "Appends an element to the end of the array.",
            &["append", "push", "insert", "table.insert"],
            vec![generic_array("array", "Array"), wildcard("element", "Element")],
            vec![data("index", "Index", "integer").with_description("The position of the new element.")],
        ),
        exec_node(
            "array/insert",
            "Insert At",
            "Array",
            "Inserts an element at a position and moves the following elements back.",
            &["insert", "table.insert"],
            vec![generic_array("array", "Array"), data("index", "Index", "integer").with_default("1"), wildcard("element", "Element")],
            vec![],
        ),
        exec_node(
            "array/remove-at",
            "Remove At",
            "Array",
            "Removes the element at a position and moves the following elements forward.",
            &["remove", "delete", "table.remove"],
            vec![generic_array("array", "Array"), data("index", "Index", "integer")],
            vec![wildcard("removed", "Removed")],
        ),
        exec_node(
            "array/remove-element",
            "Remove Element",
            "Array",
            "Removes every occurrence of an element from the array.",
            &["remove", "delete"],
            vec![generic_array("array", "Array"), wildcard("element", "Element")],
            vec![],
        ),
        exec_node("array/clear", "Clear", "Array", "Removes all elements from the array.", &["empty", "reset"], vec![array_pin("array", "Array")], vec![]),
        exec_node(
            "array/set",
            "Set Element",
            "Array",
            "Replaces the element at a position.",
            &["set", "assign", "replace"],
            vec![generic_array("array", "Array"), data("index", "Index", "integer"), wildcard("element", "Element")],
            vec![],
        ),
        pure("array/get", "Get Element", "Array", "The element at a position, or nil if there is none.", &["get", "index", "[]"], vec![generic_array("array", "Array"), data("index", "Index", "integer").with_default("1")], vec![wildcard("element", "Element")]),
        pure("array/length", "Length", "Array", "How many elements the array has.", &["length", "count", "size", "#"], vec![array_pin("array", "Array")], vec![data("length", "Length", "integer")]),
        pure("array/is-empty", "Is Empty", "Array", "True when the array has no elements.", &["empty"], vec![array_pin("array", "Array")], vec![data("result", "", "boolean")]),
        pure("array/contains", "Contains", "Array", "True when the array contains the element.", &["contains", "has", "includes"], vec![generic_array("array", "Array"), wildcard("element", "Element")], vec![data("result", "", "boolean")]),
        pure(
            "array/find",
            "Find",
            "Array",
            "The position of the first matching element, or nil when it is not in the array.",
            &["find", "index of", "search"],
            vec![generic_array("array", "Array"), wildcard("element", "Element")],
            vec![PinDefinition::data("index", "Index", TypeExpr::Optional(Box::new(named("integer"))))],
        ),
        pure("array/last", "Last Element", "Array", "The last element, or nil when the array is empty.", &["last", "end", "top"], vec![generic_array("array", "Array")], vec![wildcard("element", "Element")]),
        pure("array/random", "Random Element", "Array", "A random element of the array. The array must not be empty.", &["random", "pick"], vec![generic_array("array", "Array")], vec![wildcard("element", "Element")]),
        pure("array/copy", "Copy Array", "Array", "A new array with the same elements. Changing the copy does not change the original.", &["copy", "clone", "duplicate"], vec![generic_array("array", "Array")], vec![generic_array("result", "Copy")]),
        pure(
            "array/join",
            "Join Text",
            "Array",
            "Joins all elements into one text with table.concat. Elements must be text or numbers.",
            &["join", "concat", "implode", "table.concat"],
            vec![array_pin("array", "Array"), data("separator", "Separator", "string").with_default("\", \"")],
            vec![data("result", "", "string")],
        ),
        Builtin {
            id: "flow/for-each-pair",
            title: "For Each Pair",
            category: "Map",
            kind: NodeKind::FlowControl,
            description: "Runs the loop body once for every key and value of a map. The order is not defined.",
            keywords: &["pairs", "foreach", "loop", "map", "dictionary"],
        }
        .with_pins(
            vec![exec_in(), data("map", "Map", "table")],
            vec![
                exec_out("loop_body", "Loop Body"),
                data("key", "Key", "any"),
                data("value", "Value", "any"),
                exec_out("completed", "Completed"),
            ],
        ),
        pure("map/make", "Empty Map", "Map", "A new map without entries. Maps store values under keys like names or ids.", &["new", "dictionary", "table", "create", "{}"], vec![], vec![data("result", "Map", "table")]),
        pure("map/get", "Get Value", "Map", "The value stored under a key, or nil if there is none.", &["get", "lookup", "[]"], vec![data("map", "Map", "table"), data("key", "Key", "any")], vec![data("value", "Value", "any")]),
        exec_node("map/set", "Set Value", "Map", "Stores a value under a key and replaces the previous one.", &["set", "put", "assign"], vec![data("map", "Map", "table"), data("key", "Key", "any"), data("value", "Value", "any")], vec![]),
        exec_node("map/remove", "Remove Key", "Map", "Removes a key and its value from the map.", &["remove", "delete"], vec![data("map", "Map", "table"), data("key", "Key", "any")], vec![]),
        pure("map/has", "Has Key", "Map", "True when the map has a value stored under the key.", &["has", "contains", "exists"], vec![data("map", "Map", "table"), data("key", "Key", "any")], vec![data("result", "", "boolean")]),
        pure("map/count", "Count Entries", "Map", "How many keys the map has.", &["count", "size", "length"], vec![data("map", "Map", "table")], vec![data("count", "Count", "integer")]),
        literal("literal/number", "Number", "number", "0"),
        literal("literal/integer", "Integer", "integer", "0"),
        literal("literal/string", "String", "string", ""),
    ]
}
