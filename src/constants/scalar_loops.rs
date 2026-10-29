use super::*;
impl<'a> Tables<'a> {
    pub(super) fn counter_identity(
        &self,
        function: Node<'a>,
        env: &mut Env<'a>,
        depth: usize,
    ) -> Result<Option<Constant<'a>>> {
        let params = children(child(function, "parameters")?);
        let body = children(child(function, "body")?);
        if params.len() != 1
            || params[0].kind() != "identifier"
            || body.len() != 3
            || body[0].kind() != "variable_declaration"
            || body[1].kind() != "for_statement"
            || body[2].kind() != "return_statement"
        {
            return Ok(None);
        }
        let bindings = children(body[0]);
        if bindings.len() != 2 {
            return Ok(None);
        }
        let (counter, total) = (child(bindings[0], "name")?, child(bindings[1], "name")?);
        if counter.kind() != "identifier" || total.kind() != "identifier" {
            return Ok(None);
        }
        if [self.text(counter), self.text(total), self.text(params[0])]
            .into_iter()
            .collect::<HashSet<_>>()
            .len()
            != 3
        {
            return Ok(None);
        }
        let initial = unwrap(child(bindings[0], "value")?);
        if initial.kind() != "binary_expression"
            || self.text(child(initial, "operator")?) != "&"
            || self.text(unwrap(child(initial, "left")?)) != self.text(params[0])
            || self
                .evaluate(child(bindings[1], "value")?, env, depth + 1)?
                .integer()?
                != 0
        {
            return Ok(None);
        }
        let mask = self
            .evaluate(child(initial, "right")?, env, depth + 1)?
            .integer()?;
        let loop_node = body[1];
        if loop_node
            .child_by_field_name("initializer")
            .is_some_and(|n| n.kind() != "empty_statement")
            || !loop_node
                .child_by_field_name("condition")
                .is_some_and(|n| self.text(unwrap(n)) == self.text(counter))
        {
            return Ok(None);
        }
        let Some(step) = loop_node.child_by_field_name("increment") else {
            return Ok(None);
        };
        if step.kind() != "update_expression"
            || self.text(step) != format!("{}--", self.text(counter))
        {
            return Ok(None);
        }
        let actions = children(child(loop_node, "body")?);
        if actions.len() != 1
            || actions[0].kind() != "expression_statement"
            || self.text(actions[0]).trim_end_matches(';') != format!("{}++", self.text(total))
        {
            return Ok(None);
        }
        let ret = unwrap(body[2].named_child(0).unwrap());
        if ret.kind() != "binary_expression" || self.text(child(ret, "operator")?) != "|" {
            return Ok(None);
        }
        let high = unwrap(child(ret, "left")?);
        if high.kind() != "binary_expression"
            || self.text(child(high, "operator")?) != "&"
            || self.text(unwrap(child(high, "left")?)) != self.text(params[0])
            || self.text(unwrap(child(ret, "right")?)) != self.text(total)
        {
            return Ok(None);
        }
        let other = self
            .evaluate(child(high, "right")?, env, depth + 1)?
            .integer()? as i32;
        if !(0..=65535).contains(&mask) || other != !(mask as i32) {
            return Ok(None);
        }
        Ok(Some(Constant::Number(
            (self.evaluate(params[0], env, depth + 1)?.integer()? as i32) as f64,
        )))
    }
}
