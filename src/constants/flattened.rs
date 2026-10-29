use super::*;
impl<'a> Tables<'a> {
    pub(super) fn flattened_statements(
        &self,
        function: Node<'a>,
        env: &mut Env<'a>,
        depth: usize,
    ) -> Result<Vec<Node<'a>>> {
        let body = if function.kind() == "statement_block" {
            children(function)
        } else {
            children(child(function, "body")?)
        };
        let Some(loop_node) = body.last().copied().filter(|n| n.kind() == "for_statement") else {
            return Ok(body);
        };
        require(
            body[..body.len() - 1].iter().all(|n| {
                n.kind() == "variable_declaration"
                    && children(*n)
                        .iter()
                        .all(|d| d.child_by_field_name("value").is_none())
            }),
            "Flattened wrapper declarations",
        )?;
        let Some(initializer) = loop_node.child_by_field_name("initializer") else {
            return Err(unknown("Flattened initializer"));
        };
        self.evaluate(initializer, env, depth + 1)?;
        require(
            loop_node.child_by_field_name("increment").is_none()
                && self
                    .evaluate(child(loop_node, "condition")?, env, depth + 1)?
                    .truth()?,
            "Flattened wrapper loop",
        )?;
        let statements = children(child(loop_node, "body")?);
        require(
            statements.len() == 2
                && statements[0].kind() == "switch_statement"
                && statements[1].kind() == "break_statement",
            "Flattened wrapper body",
        )?;
        let selector = unwrap(child(statements[0], "value")?);
        require(
            selector.kind() == "subscript_expression",
            "Flattened order selector",
        )?;
        let index = unwrap(child(selector, "index")?);
        require(
            index.kind() == "update_expression" && self.text(index).ends_with("++"),
            "Flattened order counter",
        )?;
        require(
            self.evaluate(child(index, "argument")?, env, depth + 1)?
                .integer()?
                == 0,
            "Flattened order start",
        )?;
        let counter = child(index, "argument")?;
        let order_object = child(selector, "object")?;
        let reference = crate::compiler::data_reference(self, order_object)
            .ok_or_else(|| unknown("Flattened order binding"))?;
        let counter_reference = crate::compiler::data_reference(self, counter)
            .ok_or_else(|| unknown("Flattened counter binding"))?;
        let Constant::Array(order) = self.evaluate(order_object, env, depth + 1)? else {
            return Err(unknown("Flattened order data"));
        };
        require(
            !order.is_empty() && order.len() <= 64,
            "Flattened order bounds",
        )?;
        let cases = children(child(statements[0], "body")?);
        let mut output = body[..body.len() - 1].to_vec();
        for label in order.iter() {
            crate::cpu::check()?;
            let matches = cases
                .iter()
                .filter(|case| {
                    case.kind() == "switch_case"
                        && child(**case, "value").is_ok_and(|n| {
                            self.evaluate(n, env, depth + 1)
                                .is_ok_and(|v| v.same(label))
                        })
                })
                .collect::<Vec<_>>();
            require(matches.len() == 1, "Flattened unique case")?;
            let children = children(*matches[0]);
            let statements = &children[1..];
            require(
                statements
                    .last()
                    .is_some_and(|n| matches!(n.kind(), "continue_statement" | "return_statement")),
                "Flattened case termination",
            )?;
            for statement in statements {
                require(
                    !self.walk(*statement, true).iter().any(|n| {
                        matches!(
                            n.kind(),
                            "assignment_expression"
                                | "augmented_assignment_expression"
                                | "update_expression"
                        ) && n
                            .child_by_field_name(if n.kind() == "update_expression" {
                                "argument"
                            } else {
                                "left"
                            })
                            .is_some_and(|target| {
                                crate::compiler::data_reference(self, target).as_ref()
                                    == Some(&reference)
                                    || crate::compiler::data_reference(self, target).as_ref()
                                        == Some(&counter_reference)
                                    || target.kind() == "identifier"
                                        && reference.starts_with(&format!("{}[", self.text(target)))
                                    || target.kind() == "subscript_expression"
                                        && child(target, "object").is_ok_and(|o| {
                                            crate::compiler::data_reference(self, o).as_ref()
                                                == Some(&reference)
                                        })
                            })
                    }),
                    "Flattened order mutation",
                )?;
                if statement.kind() != "continue_statement" {
                    output.push(*statement);
                }
            }
            if statements
                .last()
                .is_some_and(|n| n.kind() == "return_statement")
            {
                return Ok(output);
            }
        }
        Err(unknown("Flattened return missing"))
    }
}
