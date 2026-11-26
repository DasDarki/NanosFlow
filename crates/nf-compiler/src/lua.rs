use std::fmt::Write;

pub const KEYWORDS: [&str; 22] = [
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in", "local", "nil", "not",
    "or", "repeat", "return", "then", "true", "until", "while",
];

pub const STANDARD_GLOBALS: [&str; 37] = [
    "assert", "collectgarbage", "dofile", "error", "getmetatable", "ipairs", "load", "loadfile", "next", "pairs", "pcall",
    "print", "rawequal", "rawget", "rawlen", "rawset", "require", "select", "setmetatable", "tonumber", "tostring", "type",
    "xpcall", "_G", "_ENV", "_VERSION", "coroutine", "debug", "io", "math", "os", "package", "string", "table", "utf8",
    "self", "unpack",
];

const OR: u8 = 1;
const AND: u8 = 2;
const COMPARISON: u8 = 3;
const CONCAT: u8 = 4;
const ADDITIVE: u8 = 5;
const MULTIPLICATIVE: u8 = 6;
const UNARY: u8 = 7;
const POWER: u8 = 8;
const ATOM: u8 = 9;
const PREFIX: u8 = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expr {
    text: String,
    precedence: u8,
}

fn binary_precedence(operator: &str) -> (u8, bool) {
    match operator {
        "or" => (OR, false),
        "and" => (AND, false),
        "==" | "~=" | "<" | "<=" | ">" | ">=" => (COMPARISON, false),
        ".." => (CONCAT, true),
        "+" | "-" => (ADDITIVE, false),
        "*" | "/" | "//" | "%" => (MULTIPLICATIVE, false),
        "^" => (POWER, true),
        _ => (COMPARISON, false),
    }
}

pub fn is_identifier(text: &str) -> bool {
    text.starts_with(|character: char| character.is_ascii_alphabetic() || character == '_')
        && text.chars().all(|character| character.is_ascii_alphanumeric() || character == '_')
        && !KEYWORDS.contains(&text)
}

impl Expr {
    pub fn name(name: impl Into<String>) -> Expr {
        Expr {
            text: name.into(),
            precedence: PREFIX,
        }
    }

    pub fn nil() -> Expr {
        Expr::atom("nil")
    }

    pub fn boolean(value: bool) -> Expr {
        Expr::atom(if value { "true" } else { "false" })
    }

    pub fn integer(value: i64) -> Expr {
        if value < 0 {
            Expr {
                text: value.to_string(),
                precedence: UNARY,
            }
        } else {
            Expr::atom(value.to_string())
        }
    }

    pub fn number(value: f64) -> Expr {
        if value.is_nan() {
            return Expr {
                text: "(0 / 0)".to_owned(),
                precedence: PREFIX,
            };
        }
        if value.is_infinite() {
            return if value > 0.0 {
                Expr::name("math.huge")
            } else {
                Expr::unary("-", Expr::name("math.huge"))
            };
        }
        if value.fract() == 0.0 && value.abs() < 1e15 {
            return Expr::integer(value as i64);
        }
        let text = value.to_string();
        if value < 0.0 {
            Expr { text, precedence: UNARY }
        } else {
            Expr::atom(text)
        }
    }

    pub fn string(value: &str) -> Expr {
        let mut text = String::with_capacity(value.len() + 2);
        text.push('"');
        for character in value.chars() {
            match character {
                '"' => text.push_str("\\\""),
                '\\' => text.push_str("\\\\"),
                '\n' => text.push_str("\\n"),
                '\r' => text.push_str("\\r"),
                '\t' => text.push_str("\\t"),
                character if (character as u32) < 0x20 || character as u32 == 0x7f => {
                    let _ = write!(text, "\\{:03}", character as u32);
                }
                character => text.push(character),
            }
        }
        text.push('"');
        Expr::atom(text)
    }

    fn atom(text: impl Into<String>) -> Expr {
        Expr {
            text: text.into(),
            precedence: ATOM,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn is_name(&self) -> bool {
        is_identifier(&self.text)
    }

    pub fn is_nil(&self) -> bool {
        self.text == "nil"
    }

    pub fn is_string_literal(&self) -> bool {
        self.precedence == ATOM && self.text.starts_with('"')
    }

    pub fn number_value(&self) -> Option<f64> {
        if self.precedence == PREFIX {
            return None;
        }
        self.text.parse::<f64>().ok()
    }

    pub fn is_cheap(&self) -> bool {
        let is_path = self.precedence == PREFIX
            && !self.text.is_empty()
            && self.text.split('.').all(is_identifier);
        let is_short_literal = (self.precedence == ATOM || self.precedence == UNARY)
            && self.text.len() <= 32
            && (self.text.starts_with('"') || self.number_value().is_some() || ["nil", "true", "false"].contains(&self.text.as_str()));
        is_path || is_short_literal
    }

    fn wrapped(&self, minimum: u8) -> String {
        if self.precedence < minimum {
            format!("({})", self.text)
        } else {
            self.text.clone()
        }
    }

    fn prefix_text(&self) -> String {
        self.wrapped(PREFIX)
    }

    pub fn binary(operator: &str, left: Expr, right: Expr) -> Expr {
        let (precedence, right_associative) = binary_precedence(operator);
        let left_text = if left.precedence < precedence || (left.precedence == precedence && right_associative) {
            format!("({})", left.text)
        } else {
            left.text
        };
        let right_text = if right.precedence < precedence || (right.precedence == precedence && !right_associative) {
            format!("({})", right.text)
        } else {
            right.text
        };
        Expr {
            text: format!("{left_text} {operator} {right_text}"),
            precedence,
        }
    }

    pub fn unary(operator: &str, operand: Expr) -> Expr {
        let mut operand_text = operand.wrapped(UNARY);
        if operator == "-" && operand_text.starts_with('-') {
            operand_text = format!("({operand_text})");
        }
        let separator = if operator == "not" { " " } else { "" };
        Expr {
            text: format!("{operator}{separator}{operand_text}"),
            precedence: UNARY,
        }
    }

    pub fn not(condition: Expr) -> Expr {
        if let Some(inner) = condition.text.strip_prefix("not ")
            && condition.precedence == UNARY
            && is_identifier(inner)
        {
            return Expr::name(inner);
        }
        Expr::unary("not", condition)
    }

    pub fn call(function: &str, arguments: Vec<Expr>) -> Expr {
        Expr::name(format!("{function}({})", join(&arguments)))
    }

    pub fn method_call(target: &Expr, method: &str, arguments: Vec<Expr>) -> Expr {
        Expr::name(format!("{}:{method}({})", target.prefix_text(), join(&arguments)))
    }

    pub fn table(text: impl Into<String>) -> Expr {
        Expr::atom(text)
    }

    pub fn index(target: &Expr, key: Expr) -> Expr {
        Expr::name(format!("{}[{}]", target.prefix_text(), key.text))
    }

    pub fn field(target: &Expr, field: &str) -> Expr {
        if is_identifier(field) {
            Expr::name(format!("{}.{field}", target.prefix_text()))
        } else {
            Expr::name(format!("{}[{}]", target.prefix_text(), Expr::string(field).text))
        }
    }
}

fn join(expressions: &[Expr]) -> String {
    expressions.iter().map(|expression| expression.text.as_str()).collect::<Vec<_>>().join(", ")
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Local { names: Vec<String>, values: Vec<Expr> },
    Assign { targets: Vec<String>, values: Vec<Expr> },
    Expression(Expr),
    If { condition: Expr, then_block: Block, else_block: Block },
    NumericFor { variable: String, start: Expr, limit: Expr, body: Block },
    GenericFor { variables: Vec<String>, iterator: Expr, body: Block },
    While { condition: Expr, body: Block },
    Break,
    Return(Vec<Expr>),
    LocalFunction { name: String, parameters: Vec<String>, body: Block },
    Function { name: String, parameters: Vec<String>, body: Block },
    Callback { prefix: String, parameters: Vec<String>, body: Block, suffix: String },
    Raw(String),
    Marker(String),
}

pub type Block = Vec<Statement>;

const MARKER_PREFIX: &str = "--@nanosflow ";

pub fn extract_markers(rendered: &str) -> (String, Vec<(usize, String)>) {
    let mut clean = String::with_capacity(rendered.len());
    let mut markers = Vec::new();
    let mut line_number = 1;
    for line in rendered.split_inclusive('\n') {
        if let Some(payload) = line.trim_start().strip_prefix(MARKER_PREFIX) {
            markers.push((line_number, payload.trim_end().to_owned()));
            continue;
        }
        clean.push_str(line);
        line_number += 1;
    }
    (clean, markers)
}

fn indent(depth: usize, output: &mut String) {
    for _ in 0..depth {
        output.push('\t');
    }
}

fn line(depth: usize, text: &str, output: &mut String) {
    indent(depth, output);
    output.push_str(text);
    output.push('\n');
}

pub fn render_block(block: &[Statement], depth: usize, output: &mut String) {
    let last = block.iter().rposition(|statement| !matches!(statement, Statement::Marker(_))).unwrap_or(0);
    let mut previous: Option<&Statement> = None;
    for (index, statement) in block.iter().enumerate() {
        if let Statement::Marker(payload) = statement {
            line(depth, &format!("{MARKER_PREFIX}{payload}"), output);
            continue;
        }
        let separate = depth == 0
            && previous.is_some_and(|previous| {
                (is_definition(statement) || is_definition(previous))
                    && !matches!((previous, statement), (Statement::Local { .. }, Statement::Local { .. }))
            });
        if separate {
            output.push('\n');
        }
        render_statement(statement, depth, index == last, output);
        previous = Some(statement);
    }
}

fn is_definition(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Function { .. } | Statement::LocalFunction { .. } | Statement::Callback { .. } | Statement::Raw(_)
    )
}

fn render_statement(statement: &Statement, depth: usize, is_last: bool, output: &mut String) {
    match statement {
        Statement::Local { names, values } => {
            if values.is_empty() {
                line(depth, &format!("local {}", names.join(", ")), output);
            } else {
                line(depth, &format!("local {} = {}", names.join(", "), join(values)), output);
            }
        }
        Statement::Assign { targets, values } => {
            line(depth, &format!("{} = {}", targets.join(", "), join(values)), output);
        }
        Statement::Expression(expression) => line(depth, &expression.text, output),
        Statement::If { condition, then_block, else_block } => render_if(condition, then_block, else_block, depth, output),
        Statement::NumericFor { variable, start, limit, body } => {
            line(depth, &format!("for {variable} = {}, {} do", start.text, limit.text), output);
            render_block(body, depth + 1, output);
            line(depth, "end", output);
        }
        Statement::GenericFor { variables, iterator, body } => {
            line(depth, &format!("for {} in {} do", variables.join(", "), iterator.text), output);
            render_block(body, depth + 1, output);
            line(depth, "end", output);
        }
        Statement::While { condition, body } => {
            line(depth, &format!("while {} do", condition.text), output);
            render_block(body, depth + 1, output);
            line(depth, "end", output);
        }
        Statement::Break => {
            if is_last {
                line(depth, "break", output);
            } else {
                line(depth, "do break end", output);
            }
        }
        Statement::Return(values) => {
            let text = if values.is_empty() {
                "return".to_owned()
            } else {
                format!("return {}", join(values))
            };
            if is_last {
                line(depth, &text, output);
            } else {
                line(depth, &format!("do {text} end"), output);
            }
        }
        Statement::LocalFunction { name, parameters, body } => {
            line(depth, &format!("local function {name}({})", parameters.join(", ")), output);
            render_block(body, depth + 1, output);
            line(depth, "end", output);
        }
        Statement::Function { name, parameters, body } => {
            line(depth, &format!("function {name}({})", parameters.join(", ")), output);
            render_block(body, depth + 1, output);
            line(depth, "end", output);
        }
        Statement::Marker(payload) => line(depth, &format!("{MARKER_PREFIX}{payload}"), output),
        Statement::Raw(text) => {
            for text_line in text.lines() {
                line(depth, text_line, output);
            }
        }
        Statement::Callback { prefix, parameters, body, suffix } => {
            line(depth, &format!("{prefix}function({})", parameters.join(", ")), output);
            render_block(body, depth + 1, output);
            line(depth, &format!("end{suffix}"), output);
        }
    }
}

fn render_if(condition: &Expr, then_block: &Block, else_block: &Block, depth: usize, output: &mut String) {
    line(depth, &format!("if {} then", condition.text), output);
    render_block(then_block, depth + 1, output);
    let mut remaining = else_block;
    loop {
        match remaining.as_slice() {
            [] => break,
            [Statement::If { condition, then_block, else_block }] if !then_block.is_empty() => {
                line(depth, &format!("elseif {} then", condition.text), output);
                render_block(then_block, depth + 1, output);
                remaining = else_block;
            }
            _ => {
                line(depth, "else", output);
                render_block(remaining, depth + 1, output);
                break;
            }
        }
    }
    line(depth, "end", output);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> Expr {
        Expr::name(text)
    }

    #[test]
    fn parenthesizes_only_where_precedence_requires_it() {
        let sum = Expr::binary("+", name("a"), name("b"));
        assert_eq!(Expr::binary("*", sum.clone(), name("c")).text(), "(a + b) * c");
        assert_eq!(Expr::binary("+", name("c"), sum.clone()).text(), "c + (a + b)");
        assert_eq!(Expr::binary("-", sum.clone(), name("c")).text(), "a + b - c");
        let power = Expr::binary("^", name("a"), Expr::binary("^", name("b"), name("c")));
        assert_eq!(power.text(), "a ^ b ^ c");
        assert_eq!(Expr::unary("-", Expr::integer(-5)).text(), "-(-5)");
        assert_eq!(Expr::method_call(&Expr::string("x"), "len", Vec::new()).text(), "(\"x\"):len()");
        assert_eq!(Expr::not(Expr::binary("and", name("a"), name("b"))).text(), "not (a and b)");
    }

    #[test]
    fn escapes_strings_and_formats_numbers() {
        assert_eq!(Expr::string("say \"hi\"\n").text(), "\"say \\\"hi\\\"\\n\"");
        assert_eq!(Expr::number(2.0).text(), "2");
        assert_eq!(Expr::number(0.25).text(), "0.25");
        assert_eq!(Expr::number(f64::INFINITY).text(), "math.huge");
    }

    #[test]
    fn renders_else_if_chains_and_early_returns() {
        let block = vec![
            Statement::If {
                condition: name("a"),
                then_block: vec![Statement::Return(Vec::new()), Statement::Expression(Expr::call("print", vec![]))],
                else_block: vec![Statement::If {
                    condition: name("b"),
                    then_block: vec![Statement::Break],
                    else_block: vec![Statement::Expression(Expr::call("print", vec![]))],
                }],
            },
        ];
        let mut output = String::new();
        render_block(&block, 0, &mut output);
        assert_eq!(output, "if a then\n\tdo return end\n\tprint()\nelseif b then\n\tbreak\nelse\n\tprint()\nend\n");
    }
}
