//! Static constant propagation over a bounded syntax subset, without host APIs.
use crate::{math::number, re, require, static_data::unquote, Error, Result};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
};
use tree_sitter::Node;

#[derive(Clone, Debug)]
pub enum Constant<'a> {
    Number(f64),
    Text(String),
    Bool(bool),
    Null,
    Array(Vec<Constant<'a>>),
    Object(HashMap<String, Constant<'a>>),
    Function(Node<'a>),
    Decoder,
    Dynamic,
}
pub type Env<'a> = HashMap<String, Constant<'a>>;
impl<'a> Constant<'a> {
    pub fn number(&self) -> Result<f64> {
        match self {
            Self::Number(n) => Ok(*n),
            Self::Bool(n) => Ok(u8::from(*n) as f64),
            Self::Null => Ok(0.0),
            _ => Err(unknown("Not numeric data")),
        }
    }
    pub fn integer(&self) -> Result<i64> {
        let n = self.number()?;
        require(
            n.is_finite() && n.trunc() == n && n.abs() < (1u64 << 53) as f64,
            "Constant integer bounds",
        )?;
        Ok(n as i64)
    }
    pub fn text(&self) -> Result<String> {
        match self {
            Self::Text(s) => Ok(s.clone()),
            Self::Number(n) => Ok(number(*n)),
            Self::Bool(b) => Ok(b.to_string()),
            Self::Null => Ok("None".into()),
            _ => Err(unknown("Not scalar text")),
        }
    }
    pub fn truth(&self) -> Result<bool> {
        Ok(match self {
            Self::Dynamic => return Err(unknown("Dynamic truth")),
            Self::Null => false,
            Self::Bool(b) => *b,
            Self::Number(n) => *n != 0.0 && !n.is_nan(),
            Self::Text(s) => !s.is_empty(),
            _ => true,
        })
    }
    pub fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Bool(a), Self::Bool(b)) => a == b,
            (Self::Null, Self::Null) => true,
            (Self::Function(a), Self::Function(b)) => a.id() == b.id(),
            (Self::Decoder, Self::Decoder) => true,
            _ => false,
        }
    }
}
fn unknown(message: &str) -> Error {
    Error::new("UNKNOWN_CONSTANT", message)
}
pub fn unwrap(mut n: Node<'_>) -> Node<'_> {
    while n.kind() == "parenthesized_expression" {
        if let Some(child) = n.named_child(0) {
            n = child
        } else {
            break;
        }
    }
    n
}
pub fn child<'a>(n: Node<'a>, name: &str) -> Result<Node<'a>> {
    n.child_by_field_name(name)
        .ok_or_else(|| unknown("Missing syntax field"))
}
pub fn children(n: Node<'_>) -> Vec<Node<'_>> {
    let mut c = n.walk();
    n.named_children(&mut c)
        .filter(|n| n.kind() != "comment")
        .collect()
}
pub fn walk(n: Node<'_>, descend: bool) -> Vec<Node<'_>> {
    let mut pending = vec![n];
    let mut out = Vec::new();
    while let Some(node) = pending.pop() {
        out.push(node);
        if !descend
            && node != n
            && matches!(
                node.kind(),
                "function_declaration"
                    | "function_expression"
                    | "arrow_function"
                    | "method_definition"
            )
        {
            continue;
        }
        pending.extend(children(node).into_iter().rev());
    }
    out
}
pub struct Tables<'a> {
    pub root: Node<'a>,
    pub source: &'a str,
    pub globals: RefCell<Env<'a>>,
    pool: Vec<String>,
    offset: i64,
    rotation: Cell<usize>,
    steps: Cell<usize>,
    scopes: RefCell<HashMap<usize, Env<'a>>>,
    overrides: RefCell<HashMap<usize, Constant<'a>>>,
    nodes: Vec<Node<'a>>,
    positions: HashMap<usize, usize>,
    decoder_node: Option<usize>,
    recovered_bindings: HashMap<usize, Constant<'a>>,
}
impl<'a> Tables<'a> {
    pub fn text(&self, n: Node<'_>) -> &str {
        &self.source[n.byte_range()]
    }
    pub fn new(root: Node<'a>, source: &'a str) -> Result<Self> {
        let nodes = walk(root, true);
        let positions = nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id(), index))
            .collect();
        let mut t = Self {
            root,
            source,
            globals: RefCell::new(HashMap::new()),
            pool: vec![],
            offset: 0,
            rotation: Cell::new(0),
            steps: Cell::new(0),
            scopes: RefCell::new(HashMap::new()),
            overrides: RefCell::new(HashMap::new()),
            nodes,
            positions,
            decoder_node: None,
            recovered_bindings: HashMap::new(),
        };
        // Bind declarations as inert function syntax even in small scripts
        // without an obfuscated pool. Only the bounded evaluator may use them.
        for n in children(root) {
            if n.kind() == "function_declaration" {
                if let Some(name) = n.child_by_field_name("name") {
                    t.globals
                        .borrow_mut()
                        .insert(t.text(name).to_string(), Constant::Function(n));
                }
            }
        }
        t.recover()?;
        t.scopes.borrow_mut().clear();
        Ok(t)
    }
    pub fn all(&self) -> &[Node<'a>] {
        &self.nodes
    }
    /// Reuse this parse's preorder index. A subtree starts at its own ID, so
    /// an ancestor with identical byte bounds cannot enter the result.
    pub fn walk(&self, node: Node<'a>, descend: bool) -> Vec<Node<'a>> {
        let Some(&start) = self.positions.get(&node.id()) else {
            return walk(node, descend);
        };
        let end = self
            .nodes
            .partition_point(|n| n.start_byte() < node.end_byte())
            .max(start + 1);
        if descend {
            return self.nodes[start..end].to_vec();
        }
        let mut out = Vec::new();
        let mut index = start;
        while index < end {
            let current = self.nodes[index];
            out.push(current);
            if index != start
                && matches!(
                    current.kind(),
                    "function_declaration"
                        | "function_expression"
                        | "arrow_function"
                        | "method_definition"
                )
            {
                index = self
                    .nodes
                    .partition_point(|n| n.start_byte() < current.end_byte())
                    .max(index + 1);
            } else {
                index += 1;
            }
        }
        out
    }
    pub fn bound(
        &self,
        node: Node<'a>,
        env: &mut Env<'a>,
        overrides: HashMap<usize, Constant<'a>>,
    ) -> Result<Constant<'a>> {
        let previous = self.overrides.replace(overrides);
        let result = self.eval(node, env);
        self.overrides.replace(previous);
        result
    }
    pub fn invoke_bound(
        &self,
        function: Node<'a>,
        args: Vec<Constant<'a>>,
        overrides: HashMap<usize, Constant<'a>>,
    ) -> Result<Constant<'a>> {
        self.steps.set(0);
        let previous = self.overrides.replace(overrides);
        let result = self.invoke(Constant::Function(function), args, 0);
        self.overrides.replace(previous);
        result
    }
    pub fn strings(&self) -> Vec<String> {
        let mut values = self
            .pool
            .iter()
            .map(|s| s.chars().rev().collect())
            .collect::<Vec<_>>();
        for v in self.globals.borrow().values() {
            if let Constant::Array(items) = v {
                values.extend(items.iter().filter_map(|v| {
                    if let Constant::Text(s) = v {
                        Some(s.clone())
                    } else {
                        None
                    }
                }));
            }
        }
        for n in self.all().iter().copied() {
            if matches!(n.kind(), "string" | "template_string") {
                if let Ok(s) = unquote(self.text(n)) {
                    values.push(s);
                }
            }
        }
        values
    }
    pub fn eval(&self, n: Node<'a>, env: &mut Env<'a>) -> Result<Constant<'a>> {
        self.steps.set(0);
        self.evaluate(n, env, 0)
    }
    pub fn value(&self, n: Node<'a>) -> Result<Constant<'a>> {
        self.eval(n, &mut self.scope(n))
    }
    pub fn evaluate(&self, n: Node<'a>, env: &mut Env<'a>, depth: usize) -> Result<Constant<'a>> {
        let count = self.steps.get() + 1;
        self.steps.set(count);
        require(count <= 2000 && depth <= 40, "Static expression budget")?;
        let n = unwrap(n);
        if let Some(value) = self.overrides.borrow().get(&n.id()) {
            return Ok(value.clone());
        }
        let text = self.text(n);
        let eval = |n, env: &mut Env<'a>| self.evaluate(n, env, depth + 1);
        Ok(match n.kind() {
            "number" => {
                let text = text.replace('_', "");
                require(!text.ends_with('n'), "No BigInt table indices")?;
                let v = if let Some(s) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X"))
                {
                    i64::from_str_radix(s, 16).map(|v| v as f64).ok()
                } else if let Some(s) = text.strip_prefix("0o") {
                    i64::from_str_radix(s, 8).map(|v| v as f64).ok()
                } else if let Some(s) = text.strip_prefix("0b") {
                    i64::from_str_radix(s, 2).map(|v| v as f64).ok()
                } else {
                    text.parse().ok()
                };
                Constant::Number(v.ok_or_else(|| unknown("Numeric literal"))?)
            }
            "string" | "template_string" => Constant::Text(unquote(text)?),
            "true" => Constant::Bool(true),
            "false" => Constant::Bool(false),
            "null" => Constant::Null,
            "identifier" | "property_identifier" | "shorthand_property_identifier" => {
                let v = env
                    .get(text)
                    .cloned()
                    .or_else(|| self.globals.borrow().get(text).cloned())
                    .ok_or_else(|| unknown(text))?;
                require(!matches!(v, Constant::Dynamic), "Dynamic binding")?;
                v
            }
            "array" => Constant::Array(
                children(n)
                    .into_iter()
                    .map(|c| eval(c, env))
                    .collect::<Result<_>>()?,
            ),
            "object" => {
                let mut out = HashMap::new();
                for pair in children(n) {
                    require(pair.kind() == "pair", "Object expression")?;
                    let key = child(pair, "key")?;
                    let name = if matches!(key.kind(), "string" | "template_string") {
                        unquote(self.text(key))?
                    } else if key.kind() == "computed_property_name" {
                        eval(
                            key.named_child(0)
                                .ok_or_else(|| unknown("Computed data key"))?,
                            env,
                        )?
                        .text()?
                    } else {
                        self.text(key).into()
                    };
                    out.insert(name, eval(child(pair, "value")?, env)?);
                }
                Constant::Object(out)
            }
            "function_expression" | "function_declaration" | "arrow_function" => {
                Constant::Function(n)
            }
            "unary_expression" => {
                let op = self.text(child(n, "operator")?);
                let value = eval(child(n, "argument")?, env)?;
                match op {
                    "~" => Constant::Number((!(value.number()?.trunc() as i64 as i32)) as f64),
                    "+" => Constant::Number(if let Constant::Text(s) = value {
                        s.parse().map_err(|_| unknown("Numeric string"))?
                    } else {
                        value.number()?
                    }),
                    "-" => Constant::Number(-value.number()?),
                    "!" => Constant::Bool(!value.truth()?),
                    "void" => Constant::Null,
                    _ => return Err(unknown(op)),
                }
            }
            "ternary_expression" => {
                let yes = eval(child(n, "condition")?, env)?.truth()?;
                eval(
                    child(n, if yes { "consequence" } else { "alternative" })?,
                    env,
                )?
            }
            "binary_expression" => {
                let op = self.text(child(n, "operator")?);
                let a = eval(child(n, "left")?, env)?;
                if op == "&&" && !a.truth()? || op == "||" && a.truth()? {
                    return Ok(a);
                }
                let b = eval(child(n, "right")?, env)?;
                match op {
                    "&&" | "||" => b,
                    "+" => {
                        if matches!(a, Constant::Text(_)) || matches!(b, Constant::Text(_)) {
                            Constant::Text(a.text()? + &b.text()?)
                        } else {
                            Constant::Number(a.number()? + b.number()?)
                        }
                    }
                    "-" => Constant::Number(a.number()? - b.number()?),
                    "*" => Constant::Number(a.number()? * b.number()?),
                    "/" => {
                        let b = b.number()?;
                        require(b != 0.0, "Constant division by zero")?;
                        Constant::Number(a.number()? / b)
                    }
                    "%" => Constant::Number(a.number()? % b.number()?),
                    "^" | "&" | "|" | "<<" | ">>" | ">>>" => {
                        let (a, b) = (
                            a.number()?.trunc() as i64 as i32,
                            b.number()?.trunc() as i64 as i32,
                        );
                        Constant::Number(match op {
                            "^" => (a ^ b) as f64,
                            "&" => (a & b) as f64,
                            "|" => (a | b) as f64,
                            "<<" => a.wrapping_shl(b as u32 & 31) as f64,
                            ">>" => a.wrapping_shr(b as u32 & 31) as f64,
                            _ => (a as u32).wrapping_shr(b as u32 & 31) as f64,
                        })
                    }
                    "==" | "===" => Constant::Bool(a.same(&b)),
                    "!=" | "!==" => Constant::Bool(!a.same(&b)),
                    "<" => Constant::Bool(a.number()? < b.number()?),
                    ">" => Constant::Bool(a.number()? > b.number()?),
                    "<=" => Constant::Bool(a.number()? <= b.number()?),
                    ">=" => Constant::Bool(a.number()? >= b.number()?),
                    _ => return Err(unknown(op)),
                }
            }
            "member_expression" | "subscript_expression" => {
                let value = eval(child(n, "object")?, env)?;
                let key = if let Some(prop) = n.child_by_field_name("property") {
                    Constant::Text(self.text(prop).into())
                } else {
                    eval(child(n, "index")?, env)?
                };
                match value {
                    Constant::Object(map) => map
                        .get(&key.text()?)
                        .cloned()
                        .ok_or_else(|| unknown("Data member absent"))?,
                    Constant::Array(a) => {
                        if key.text()? == "length" {
                            Constant::Number(a.len() as f64)
                        } else {
                            a.get(key.integer()? as usize)
                                .cloned()
                                .ok_or_else(|| unknown("Constant index"))?
                        }
                    }
                    Constant::Text(s) => {
                        if key.text()? == "length" {
                            Constant::Number(s.chars().count() as f64)
                        } else {
                            Constant::Text(
                                s.chars()
                                    .nth(key.integer()? as usize)
                                    .ok_or_else(|| unknown("String index"))?
                                    .to_string(),
                            )
                        }
                    }
                    Constant::Function(function) if key.text()? == "length" => {
                        let count = function
                            .child_by_field_name("parameters")
                            .map(|parameters| {
                                children(parameters)
                                    .into_iter()
                                    .take_while(|p| p.kind() == "identifier")
                                    .count()
                            })
                            .unwrap_or_else(|| {
                                usize::from(function.child_by_field_name("parameter").is_some())
                            });
                        Constant::Number(count as f64)
                    }
                    _ => return Err(unknown("Non-data member access")),
                }
            }
            "augmented_assignment_expression" => {
                let left = child(n, "left")?;
                require(left.kind() == "identifier", "Static compound binding")?;
                let a = eval(left, env)?.integer()? as i32;
                let b = eval(child(n, "right")?, env)?.integer()? as i32;
                let value = match self.text(child(n, "operator")?) {
                    "^=" => a ^ b,
                    "&=" => a & b,
                    "|=" => a | b,
                    "+=" => a.wrapping_add(b),
                    "-=" => a.wrapping_sub(b),
                    _ => return Err(unknown("Static compound operator")),
                };
                let value = Constant::Number(value as f64);
                env.insert(self.text(left).into(), value.clone());
                value
            }
            "assignment_expression" => {
                let left = child(n, "left")?;
                let value = eval(child(n, "right")?, env)?;
                if left.kind() == "identifier" {
                    env.insert(self.text(left).into(), value.clone());
                } else if left.kind() == "subscript_expression" {
                    let object = child(left, "object")?;
                    require(object.kind() == "identifier", "Nonlocal assignment")?;
                    let index = eval(child(left, "index")?, env)?.integer()?;
                    require((0..65536).contains(&index), "Static array write bounds")?;
                    let Some(Constant::Array(values)) = env.get_mut(self.text(object)) else {
                        return Err(unknown("Nonlocal array assignment"));
                    };
                    let target = values
                        .get_mut(index as usize)
                        .ok_or_else(|| unknown("Static array write bounds"))?;
                    *target = value.clone();
                } else {
                    return Err(unknown("Nonlocal assignment"));
                }
                value
            }
            "sequence_expression" => {
                let mut v = Constant::Null;
                for c in children(n) {
                    v = eval(c, env)?;
                }
                v
            }
            "call_expression" => {
                let callee = child(n, "function")?;
                let args = children(child(n, "arguments")?);
                if matches!(callee.kind(), "member_expression" | "subscript_expression")
                    && self.text(child(callee, "object")?) == "Reflect"
                {
                    let prop = if let Some(prop) = callee.child_by_field_name("property") {
                        self.text(prop).into()
                    } else {
                        eval(child(callee, "index")?, env)?.text()?
                    };
                    require(
                        prop == "apply" && args.len() == 3,
                        "Static forwarding wrapper",
                    )?;
                    let f = eval(args[0], env)?;
                    let Constant::Array(args) = eval(args[2], env)? else {
                        return Err(unknown("Wrapper argument array"));
                    };
                    self.invoke(f, args, depth + 1)?
                } else {
                    let f = if self.text(callee) == "parseInt" {
                        None
                    } else {
                        Some(eval(callee, env)?)
                    };
                    let args = args
                        .into_iter()
                        .map(|a| eval(a, env))
                        .collect::<Result<Vec<_>>>()?;
                    if let Some(f) = f {
                        self.invoke(f, args, depth + 1)?
                    } else {
                        let text = args
                            .first()
                            .ok_or_else(|| unknown("parseInt argument"))?
                            .text()?;
                        let m = re(r"^\s*([+-]?\d+)")
                            .captures(&text)
                            .ok_or_else(|| unknown("Not decimal input"))?;
                        Constant::Number(m[1].parse().map_err(|_| unknown("Decimal bounds"))?)
                    }
                }
            }
            _ => return Err(unknown(n.kind())),
        })
    }
    pub fn invoke(
        &self,
        f: Constant<'a>,
        args: Vec<Constant<'a>>,
        depth: usize,
    ) -> Result<Constant<'a>> {
        require(depth <= 40, "Static wrapper depth")?;
        if matches!(f, Constant::Decoder) {
            let index = args
                .first()
                .ok_or_else(|| unknown("Decoder index"))?
                .integer()?;
            require(!self.pool.is_empty(), "Empty string pool")?;
            let index = (index - self.offset + self.rotation.get() as i64)
                .rem_euclid(self.pool.len() as i64) as usize;
            return Ok(Constant::Text(self.pool[index].chars().rev().collect()));
        }
        let Constant::Function(node) = f else {
            return Err(unknown("Not a constant wrapper"));
        };
        let params = children(child(node, "parameters")?);
        let mut local = self.scope(node);
        for (i, p) in params.into_iter().enumerate() {
            if p.kind() == "identifier" {
                if let Some(v) = args.get(i) {
                    local.insert(self.text(p).into(), v.clone());
                }
            }
        }
        for statement in children(child(node, "body")?) {
            match statement.kind() {
                "empty_statement" => (),
                "variable_declaration" | "lexical_declaration" => {
                    for declaration in children(statement) {
                        if let (Some(name), Some(value)) = (
                            declaration.child_by_field_name("name"),
                            declaration.child_by_field_name("value"),
                        ) {
                            require(name.kind() == "identifier", "Static wrapper binding")?;
                            let value = self.evaluate(value, &mut local, depth + 1)?;
                            local.insert(self.text(name).to_string(), value);
                        }
                    }
                }
                "return_statement" => {
                    return self.evaluate(
                        statement
                            .named_child(0)
                            .ok_or_else(|| unknown("Missing constant return"))?,
                        &mut local,
                        depth + 1,
                    )
                }
                "expression_statement" => {
                    self.evaluate(statement.named_child(0).unwrap(), &mut local, depth + 1)?;
                }
                "switch_statement" => {
                    let value = self.evaluate(child(statement, "value")?, &mut local, depth + 1)?;
                    let mut chosen = None;
                    let mut fallback = None;
                    for case in children(child(statement, "body")?) {
                        if case.kind() == "switch_default" {
                            fallback = Some(case)
                        } else if self
                            .evaluate(child(case, "value")?, &mut local, depth + 1)?
                            .same(&value)
                        {
                            chosen = Some(case);
                            break;
                        }
                    }
                    if let Some(case) = chosen.or(fallback) {
                        for statement in children(case) {
                            if statement.kind() == "return_statement" {
                                return self.evaluate(
                                    statement
                                        .named_child(0)
                                        .ok_or_else(|| unknown("Empty constant return"))?,
                                    &mut local,
                                    depth + 1,
                                );
                            }
                        }
                    }
                    return Err(unknown("Unsupported constant switch"));
                }
                _ => return Err(unknown("Nonconstant wrapper statement")),
            }
        }
        Err(unknown("Wrapper has no return"))
    }
    fn recover(&mut self) -> Result<()> {
        let all = self.nodes.clone();
        let mut factories = Vec::new();
        for node in &all {
            if node.kind() != "call_expression" {
                continue;
            }
            let callee = child(*node, "function")?;
            if callee.kind() != "member_expression"
                || self.text(child(callee, "property")?) != "split"
            {
                continue;
            }
            let object = child(callee, "object")?;
            let args = children(child(*node, "arguments")?);
            if matches!(object.kind(), "string" | "template_string")
                && self.text(object).len() > 1000
                && args.len() == 1
            {
                if let (Ok(text), Ok(delimiter)) =
                    (unquote(self.text(object)), unquote(self.text(args[0])))
                {
                    if delimiter.chars().count() == 1 {
                        factories.push((
                            *node,
                            text.split(&delimiter).map(String::from).collect::<Vec<_>>(),
                        ))
                    }
                }
            }
        }
        factories.sort_by_key(|(_, p)| std::cmp::Reverse(p.len()));
        let Some((pool_node, pool)) = factories.into_iter().next() else {
            return Ok(());
        };
        self.pool = pool;
        let mut factory = pool_node;
        while factory.kind() != "function_declaration" {
            let Some(p) = factory.parent() else {
                return Ok(());
            };
            factory = p;
        }
        let mut outer = factory.parent();
        while outer
            .is_some_and(|n| !matches!(n.kind(), "function_expression" | "function_declaration"))
        {
            outer = outer.unwrap().parent()
        }
        let Some(outer) = outer else { return Ok(()) };
        let top = self.walk(child(outer, "body")?, false);
        let mut decoder = None;
        for node in &top {
            if node.kind() == "function_declaration" {
                let name = self.text(child(*node, "name")?).to_string();
                self.globals
                    .borrow_mut()
                    .insert(name.clone(), Constant::Function(*node));
                let text = self.text(*node);
                if text.len() < 500 && text.contains(".reverse()") && text.contains(".join(") {
                    let pat = re(r"([\w$]+)\s*=\s*([\w$]+)\s*-\s*(\d+)");
                    if let Some(c) = pat.captures(text) {
                        if c[1] == c[2] {
                            decoder = Some(name);
                            self.offset = c[3].parse().map_err(|_| unknown("Decoder offset"))?;
                            self.decoder_node = Some(node.id());
                        }
                    }
                }
            }
        }
        let Some(decoder) = decoder else {
            return Ok(());
        };
        self.globals.borrow_mut().insert(decoder, Constant::Decoder);
        let mut matched = false;
        for node in &top {
            if node.kind() != "call_expression" {
                continue;
            }
            let function = unwrap(child(*node, "function")?);
            if function.kind() != "function_expression"
                || !self.text(function).contains("parseInt")
                || self.text(function).len() > 4000
            {
                continue;
            }
            let args = children(child(*node, "arguments")?);
            if args.len() < 2 {
                continue;
            }
            let Ok(target) = self.eval(args[1], &mut Env::new()).and_then(|v| v.number()) else {
                continue;
            };
            let mut local = Env::new();
            let mut expression = None;
            for inner in self.walk(child(function, "body")?, false) {
                if inner.kind() == "assignment_expression" {
                    let right = child(inner, "right")?;
                    if self.text(right).contains("parseInt") {
                        expression = Some(right);
                        break;
                    }
                    let _ = self.eval(inner, &mut local);
                }
            }
            let Some(expression) = expression else {
                continue;
            };
            let mut candidates: HashSet<usize> = (0..self.pool.len()).collect();
            let numeric: Vec<_> = self
                .pool
                .iter()
                .enumerate()
                .filter(|(_, v)| re(r"^\s*[+-]?\d+").is_match(&v.chars().rev().collect::<String>()))
                .map(|(i, _)| i)
                .collect();
            for call in self.walk(expression, true) {
                if call.kind() != "call_expression"
                    || self.text(child(call, "function")?) != "parseInt"
                {
                    continue;
                }
                let args = children(child(call, "arguments")?);
                if args.len() != 1 {
                    continue;
                }
                let call = unwrap(args[0]);
                if call.kind() != "call_expression" {
                    continue;
                }
                let callee = child(call, "function")?;
                if !matches!(self.eval(callee, &mut local.clone()), Ok(Constant::Decoder)) {
                    continue;
                }
                let args = children(child(call, "arguments")?);
                if args.len() != 1
                    || self
                        .walk(args[0], true)
                        .iter()
                        .any(|n| n.kind() == "call_expression")
                {
                    continue;
                }
                if let Ok(index) = self
                    .eval(args[0], &mut local.clone())
                    .and_then(|v| v.integer())
                {
                    let index = (index - self.offset).rem_euclid(self.pool.len() as i64) as usize;
                    let eligible: HashSet<_> = numeric
                        .iter()
                        .map(|n| (n + self.pool.len() - index) % self.pool.len())
                        .collect();
                    candidates.retain(|v| eligible.contains(v));
                }
            }
            for rotation in candidates {
                self.rotation.set(rotation);
                if self
                    .eval(expression, &mut local.clone())
                    .and_then(|v| v.number())
                    .is_ok_and(|n| (n - target).abs() < 1e-7)
                {
                    matched = true;
                    break;
                }
            }
            require(matched, "String pool rotation mismatch")?;
            break;
        }
        require(matched, "String pool rotation formula missing")?;
        for _ in 0..3 {
            for node in &top {
                if node.kind() != "assignment_expression" {
                    continue;
                }
                let (left, right) = (child(*node, "left")?, child(*node, "right")?);
                if left.kind() != "identifier" {
                    continue;
                }
                let name = self.text(left).to_string();
                if right.kind() == "identifier" {
                    let value = self.globals.borrow().get(self.text(right)).cloned();
                    if let Some(v) = value {
                        self.globals.borrow_mut().insert(name, v.clone());
                        self.recovered_bindings.insert(node.id(), v);
                    }
                } else if right.kind() == "function_expression" && self.text(right).len() < 1000 {
                    self.globals
                        .borrow_mut()
                        .insert(name, Constant::Function(right));
                } else if right.kind() == "array" && right.named_child_count() > 100 {
                    if let Ok(values) = children(right)
                        .into_iter()
                        .map(|n| self.eval(n, &mut Env::new()))
                        .collect::<Result<Vec<_>>>()
                    {
                        let value = Constant::Array(values);
                        self.globals.borrow_mut().insert(name, value.clone());
                        self.recovered_bindings.insert(node.id(), value);
                    }
                }
            }
        }
        Ok(())
    }
    pub fn scope(&self, node: Node<'a>) -> Env<'a> {
        let mut parent = node.parent();
        while parent.is_some_and(|p| {
            !matches!(
                p.kind(),
                "function_declaration"
                    | "function_expression"
                    | "arrow_function"
                    | "method_definition"
            )
        }) {
            parent = parent.unwrap().parent()
        }
        let Some(parent) = parent else {
            return Env::new();
        };
        if let Some(env) = self.scopes.borrow().get(&parent.id()) {
            return env.clone();
        }
        let Some(body) = parent.child_by_field_name("body") else {
            return Env::new();
        };
        let rows = self.walk(body, false);
        let mut counts = HashMap::new();
        for n in &rows {
            if n.kind() == "assignment_expression"
                || n.kind() == "variable_declarator" && n.child_by_field_name("value").is_some()
            {
                if let Some(left) = n.child_by_field_name(if n.kind() == "assignment_expression" {
                    "left"
                } else {
                    "name"
                }) {
                    if left.kind() == "identifier" {
                        *counts.entry(self.text(left).to_string()).or_insert(0usize) += 1;
                    }
                }
            }
        }
        let mut local = self.scope(parent);
        if let Some(params) = parent.child_by_field_name("parameters") {
            for p in children(params) {
                if p.kind() == "identifier" {
                    local.insert(self.text(p).into(), Constant::Dynamic);
                }
            }
        }
        if let Some(param) = parent.child_by_field_name("parameter") {
            local.insert(self.text(param).into(), Constant::Dynamic);
        }
        for n in &rows {
            if n.kind() == "variable_declarator" {
                if let Some(name) = n
                    .child_by_field_name("name")
                    .filter(|n| n.kind() == "identifier")
                {
                    local.insert(self.text(name).into(), Constant::Dynamic);
                }
            }
        }
        for name in counts.keys() {
            local.insert(name.clone(), Constant::Dynamic);
        }
        let mut owned = HashSet::new();
        for n in &rows {
            if n.kind() == "function_declaration" {
                if let Some(name) = n.child_by_field_name("name") {
                    let value = if self.decoder_node == Some(n.id()) {
                        Constant::Decoder
                    } else {
                        Constant::Function(*n)
                    };
                    local.insert(self.text(name).into(), value);
                }
            }
        }
        // A bounded wrapper may close over this same scope while a binding is
        // being analyzed. Publish only the bindings known so far, rather than
        // recursively rebuilding an unfinished scope.
        self.scopes.borrow_mut().insert(parent.id(), local.clone());
        for n in rows {
            self.scopes.borrow_mut().insert(parent.id(), local.clone());
            if !matches!(n.kind(), "assignment_expression" | "variable_declarator") {
                continue;
            }
            let (Ok(left), Ok(right)) = (
                child(
                    n,
                    if n.kind() == "assignment_expression" {
                        "left"
                    } else {
                        "name"
                    },
                ),
                child(
                    n,
                    if n.kind() == "assignment_expression" {
                        "right"
                    } else {
                        "value"
                    },
                ),
            ) else {
                continue;
            };
            if matches!(left.kind(), "member_expression" | "subscript_expression") {
                let Ok(object) = child(left, "object") else {
                    continue;
                };
                let name = self.text(object);
                if object.kind() == "identifier" && owned.contains(name) {
                    let key = if let Some(prop) = left.child_by_field_name("property") {
                        Ok(self.text(prop).into())
                    } else {
                        child(left, "index")
                            .and_then(|n| self.eval(n, &mut local.clone()))
                            .and_then(|v| v.text())
                    };
                    if let Ok(k) = key {
                        let value = self
                            .eval(right, &mut local.clone())
                            .unwrap_or(Constant::Dynamic);
                        match local.get_mut(name) {
                            Some(Constant::Object(map)) => {
                                map.insert(k, value);
                            }
                            Some(Constant::Array(values)) => {
                                if let Some(value_slot) = k
                                    .parse::<usize>()
                                    .ok()
                                    .and_then(|index| values.get_mut(index))
                                {
                                    *value_slot = value;
                                }
                            }
                            _ => (),
                        }
                    }
                }
                continue;
            }
            if left.kind() != "identifier" || counts.get(self.text(left)) != Some(&1) {
                continue;
            }
            let name = self.text(left);
            if let Some(v) = self
                .recovered_bindings
                .get(&n.id())
                .cloned()
                .or_else(|| self.eval(right, &mut local.clone()).ok())
            {
                if matches!(
                    v,
                    Constant::Text(_)
                        | Constant::Number(_)
                        | Constant::Object(_)
                        | Constant::Array(_)
                        | Constant::Function(_)
                        | Constant::Decoder
                        | Constant::Bool(_)
                        | Constant::Null
                ) {
                    if matches!(v, Constant::Object(_) | Constant::Array(_)) {
                        owned.insert(name.to_string());
                    }
                    local.insert(name.into(), v);
                }
            }
        }
        self.scopes.borrow_mut().insert(parent.id(), local.clone());
        local
    }
}
