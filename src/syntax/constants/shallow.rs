use super::*;

impl<'a> Tables<'a> {
    /// A conservative fast path for immutable scalar/array declarations. It
    /// resolves only referenced lexical bindings; calls or writes use the
    /// complete existing scope evaluator instead.
    pub(crate) fn shallow(&self, node: Node<'a>) -> Result<Constant<'a>> {
        require(
            self.overrides.borrow().is_empty(),
            "Declaration with active proof overrides",
        )?;
        self.shallow_value(node, 0, &mut 2000)
    }
    fn shallow_value(
        &self,
        node: Node<'a>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Constant<'a>> {
        crate::cpu::check()?;
        require(
            depth < 40 && *remaining > 0,
            "Declaration dependency bounds",
        )?;
        *remaining -= 1;
        let node = unwrap(node);
        if let Some(value) = self.shallow_values.borrow().get(&node.id()) {
            return Ok(value.clone());
        }
        let value = self.shallow_expression(node, depth, remaining)?;
        if self.shallow_values.borrow().len() < 4096 {
            self.shallow_values
                .borrow_mut()
                .insert(node.id(), value.clone());
        }
        Ok(value)
    }
    fn shallow_expression(
        &self,
        node: Node<'a>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Constant<'a>> {
        if node.kind() == "identifier" {
            return self.shallow_binding(node, depth + 1, remaining);
        }
        if matches!(node.kind(), "function_expression" | "arrow_function") {
            return Ok(Constant::Function(node));
        }
        let mut env = Env::new();
        for n in self.walk(node, false) {
            require(
                !matches!(
                    n.kind(),
                    "call_expression"
                        | "new_expression"
                        | "assignment_expression"
                        | "augmented_assignment_expression"
                        | "update_expression"
                        | "await_expression"
                ),
                "Declaration requires complete scope",
            )?;
            if n.kind() == "identifier" && !env.contains_key(self.text(n)) {
                let value = self.shallow_binding(n, depth + 1, remaining)?;
                env.insert(self.text(n).to_owned(), value);
            }
        }
        self.eval(node, &mut env)
    }
    fn shallow_binding(
        &self,
        reference: Node<'a>,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Constant<'a>> {
        crate::cpu::check()?;
        require(
            depth < 40 && *remaining > 0,
            "Declaration dependency bounds",
        )?;
        *remaining -= 1;
        let name = self.text(reference);
        let mut ancestor = self.scope_owners.get(&reference.id()).copied();
        while let Some(owner) = ancestor {
            ancestor = self.scope_owners.get(&owner.id()).copied();
            if !matches!(
                owner.kind(),
                "program"
                    | "function_declaration"
                    | "function_expression"
                    | "arrow_function"
                    | "method_definition"
            ) {
                continue;
            }
            for params in [
                owner.child_by_field_name("parameters"),
                owner.child_by_field_name("parameter"),
            ]
            .into_iter()
            .flatten()
            {
                require(
                    !walk(params, true)
                        .iter()
                        .any(|n| n.kind() == "identifier" && self.text(*n) == name),
                    "Declaration parameter shadow",
                )?;
            }
            let body = owner.child_by_field_name("body").unwrap_or(owner);
            let mut declared = false;
            let mut values = Vec::new();
            let mut mutated = false;
            for (index, n) in self.walk(body, false).into_iter().enumerate() {
                if index % 1024 == 0 {
                    crate::cpu::check()?;
                }
                if matches!(n.kind(), "function_declaration" | "class_declaration")
                    && n.child_by_field_name("name")
                        .is_some_and(|n| self.text(n) == name)
                {
                    return Err(unknown("Declaration function shadow"));
                }
                let (target, rhs) = match n.kind() {
                    "variable_declarator" => (
                        n.child_by_field_name("name"),
                        n.child_by_field_name("value"),
                    ),
                    "assignment_expression" => (
                        n.child_by_field_name("left"),
                        n.child_by_field_name("right"),
                    ),
                    "augmented_assignment_expression" => (n.child_by_field_name("left"), None),
                    "update_expression" => (n.child_by_field_name("argument"), None),
                    _ => continue,
                };
                let Some(target) = target else { continue };
                if target.kind() == "identifier" && self.text(target) == name {
                    declared = true;
                    if let Some(rhs) = rhs {
                        values.push(rhs);
                    } else if n.kind() != "variable_declarator" {
                        mutated = true;
                    }
                } else if matches!(target.kind(), "member_expression" | "subscript_expression")
                    && target
                        .child_by_field_name("object")
                        .is_some_and(|n| self.text(n) == name)
                {
                    mutated = true;
                } else if matches!(target.kind(), "array_pattern" | "object_pattern")
                    && walk(target, true)
                        .iter()
                        .any(|n| n.kind() == "identifier" && self.text(*n) == name)
                {
                    return Err(unknown("Declaration destructuring shadow"));
                }
            }
            if declared {
                require(
                    !mutated && values.len() == 1,
                    "Declaration reassigned or dynamic",
                )?;
                require(
                    values[0].start_byte() < reference.start_byte(),
                    "Declaration forward dependency",
                )?;
                return self.shallow_value(values[0], depth + 1, remaining);
            }
            // A write to an inherited array cannot be ignored by this path.
            require(!mutated, "Declaration inherited mutation")?;
        }
        Err(unknown("Declaration binding absent"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn read(source: &str) -> (Result<serde_json::Value>, Result<serde_json::Value>) {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_javascript::LANGUAGE.into())
            .unwrap();
        let tree = parser.parse(source, None).unwrap();
        let tables = Tables::new(tree.root_node(), source).unwrap();
        let rhs = tables
            .all()
            .iter()
            .find(|n| {
                n.kind() == "variable_declarator"
                    && child(**n, "name").is_ok_and(|n| tables.text(n) == "result")
            })
            .and_then(|n| n.child_by_field_name("value"))
            .unwrap();
        let json = |v| crate::syntax::compiler::constant_json(v).ok_or_else(|| unknown("non-data"));
        (
            tables.shallow(rhs).and_then(json),
            tables.value(rhs).and_then(json),
        )
    }
    #[test]
    fn targeted_dependencies_match_complete_scope_and_keep_lexical_shadowing() {
        let source = "function outer(){var root=(a=>a).length+6;var seed=root^3,key=[seed,2,3];function pack(){var result=key;}}";
        let (fast, full) = read(source);
        assert_eq!(fast, full);
        assert_eq!(fast.unwrap(), serde_json::json!([4.0, 2.0, 3.0]));
        for bad in [
            source.replace("function outer()", "function outer(root)"),
            source.replace(
                "var seed=root^3,key=[seed,2,3]",
                "var key=[seed,2,3],seed=root^3",
            ),
            source.replace("function pack()", "seed++;function pack()"),
            source.replace("function pack()", "key[0]=9;function pack()"),
        ] {
            assert!(read(&bad).0.is_err());
        }
    }
    #[test]
    fn calls_and_cycles_require_the_complete_scope_path() {
        let (fast, full) =
            read("function f(){return 7}function outer(){var key=[f(),2];var result=key;}");
        assert!(fast.is_err());
        assert_eq!(full.unwrap(), serde_json::json!([7.0, 2.0]));
        assert!(read("function outer(){var a=b,b=a;var result=a;}")
            .0
            .is_err());
    }
}
