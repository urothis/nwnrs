use std::path::Path;

use crate::{
    AssignmentOp, BinaryOp, BlockStmt, CaseStmt, Declaration, DefaultStmt, DoWhileStmt, Expr,
    ExprKind, ExpressionStmt, ForStmt, FunctionDecl, IfStmt, IncludeDirective, Literal, Script,
    SimpleStmt, SourceMap, Stmt, StructDecl, StructFieldDecl, SwitchStmt, TypeKind, TypeSpec,
    UnaryOp, VarDeclarator, WhileStmt,
};

/// Renders one parsed script as Graphviz DOT.
#[must_use]
pub fn render_script_graphviz(script: &Script, source_map: Option<&SourceMap>) -> String {
    let mut renderer = GraphvizRenderer::new(source_map);
    renderer.render_script(script);
    renderer.finish()
}

struct GraphvizRenderer<'a> {
    source_map: Option<&'a SourceMap>,
    next_id:    usize,
    nodes:      Vec<String>,
    edges:      Vec<String>,
}

impl<'a> GraphvizRenderer<'a> {
    fn new(source_map: Option<&'a SourceMap>) -> Self {
        Self {
            source_map,
            next_id: 0,
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }

    fn finish(self) -> String {
        let mut dot = String::from(
            "digraph nwscript {\ngraph [bgcolor=\"#f8fafc\", pad=\"0.35\", nodesep=\"0.28\", \
             ranksep=\"0.55\", fontname=\"Helvetica\"];\nrankdir=TB;\nnode [shape=box, \
             style=\"rounded,filled\", fillcolor=\"#ffffff\", color=\"#94a3b8\", \
             fontcolor=\"#0f172a\", fontname=\"Helvetica\", fontsize=10, \
             margin=\"0.12,0.08\"];\nedge [color=\"#94a3b8\", fontcolor=\"#475569\", \
             fontname=\"Helvetica\", fontsize=9, arrowsize=0.65];\n",
        );
        for node in self.nodes {
            dot.push_str("  ");
            dot.push_str(&node);
            dot.push('\n');
        }
        for edge in self.edges {
            dot.push_str("  ");
            dot.push_str(&edge);
            dot.push('\n');
        }
        dot.push_str("}\n");
        dot
    }

    fn render_script(&mut self, script: &Script) {
        let root = self.node("Script");
        for item in &script.items {
            match item {
                crate::TopLevelItem::Include(include) => {
                    let child = self.render_include(include);
                    self.edge(root, child, None);
                }
                crate::TopLevelItem::Global(declaration) => {
                    let child = self.render_declaration("Global", declaration);
                    self.edge(root, child, None);
                }
                crate::TopLevelItem::Function(function) => {
                    let child = self.render_function(function);
                    self.edge(root, child, None);
                }
                crate::TopLevelItem::Struct(struct_decl) => {
                    let child = self.render_struct(struct_decl);
                    self.edge(root, child, None);
                }
                crate::TopLevelItem::Enum(declaration) => {
                    let child = self.node_with_span(
                        format!("Enum {} : {:?}", declaration.name, declaration.backing),
                        declaration.span,
                    );
                    self.edge(root, child, None);
                    for variant in &declaration.variants {
                        let variant_node =
                            self.node_with_span(format!("Variant {}", variant.name), variant.span);
                        self.edge(child, variant_node, None);
                    }
                }
                crate::TopLevelItem::TypeAlias(alias) => {
                    let child =
                        self.node_with_span(format!("TypeAlias {}", alias.name), alias.span);
                    self.edge(root, child, None);
                    let target = self.render_type(&alias.target);
                    self.edge(child, target, Some("target"));
                }
                crate::TopLevelItem::StaticAssert(assertion) => {
                    let child = self.node_with_span("StaticAssert", assertion.span);
                    self.edge(root, child, None);
                    let condition = self.render_expr(&assertion.condition);
                    self.edge(child, condition, Some("condition"));
                }
            }
        }
    }

    fn render_include(&mut self, include: &IncludeDirective) -> usize {
        self.node_with_span(format!("Include {}", include.path), include.span)
    }

    fn render_function(&mut self, function: &FunctionDecl) -> usize {
        let root = self.node_with_span(format!("Function {}", function.name), function.span);
        let return_type = self.render_type(&function.return_type);
        self.edge(root, return_type, Some("return"));
        for parameter in &function.parameters {
            let param = self.node_with_span(format!("Param {}", parameter.name), parameter.span);
            self.edge(root, param, None);
            let ty = self.render_type(&parameter.ty);
            self.edge(param, ty, Some("type"));
            if let Some(default) = &parameter.default {
                let expr = self.render_expr(default);
                self.edge(param, expr, Some("default"));
            }
        }
        if let Some(body) = &function.body {
            let block = self.render_block(body);
            self.edge(root, block, Some("body"));
        }
        root
    }

    fn render_struct(&mut self, struct_decl: &StructDecl) -> usize {
        let root = self.node_with_span(format!("Struct {}", struct_decl.name), struct_decl.span);
        for field in &struct_decl.fields {
            let child = self.render_struct_field(field);
            self.edge(root, child, None);
        }
        root
    }

    fn render_struct_field(&mut self, field: &StructFieldDecl) -> usize {
        let root = self.node_with_span("StructField", field.span);
        let ty = self.render_type(&field.ty);
        self.edge(root, ty, Some("type"));
        for name in &field.names {
            let child = self.node_with_span(format!("Name {}", name.name), name.span);
            self.edge(root, child, None);
        }
        root
    }

    fn render_declaration(&mut self, kind: &str, declaration: &Declaration) -> usize {
        let root = self.node_with_span(kind, declaration.span);
        let ty = self.render_type(&declaration.ty);
        self.edge(root, ty, Some("type"));
        for declarator in &declaration.declarators {
            let child = self.render_declarator(declarator);
            self.edge(root, child, None);
        }
        root
    }

    fn render_declarator(&mut self, declarator: &VarDeclarator) -> usize {
        let root = self.node_with_span(format!("Var {}", declarator.name), declarator.span);
        if let Some(initializer) = &declarator.initializer {
            let expr = self.render_expr(initializer);
            self.edge(root, expr, Some("init"));
        }
        root
    }

    fn render_block(&mut self, block: &BlockStmt) -> usize {
        let root = self.node_with_span("Block", block.span);
        for statement in &block.statements {
            let child = self.render_stmt(statement);
            self.edge(root, child, None);
        }
        root
    }

    fn render_stmt(&mut self, statement: &Stmt) -> usize {
        match statement {
            Stmt::Block(block) => self.render_block(block),
            Stmt::Declaration(declaration) => self.render_declaration("Declaration", declaration),
            Stmt::Expression(expression) => self.render_expression_stmt(expression),
            Stmt::If(stmt) => self.render_if(stmt),
            Stmt::Switch(stmt) => self.render_switch(stmt),
            Stmt::Return(stmt) => self.render_return(stmt),
            Stmt::While(stmt) => self.render_while(stmt),
            Stmt::DoWhile(stmt) => self.render_do_while(stmt),
            Stmt::For(stmt) => self.render_for(stmt),
            Stmt::Case(stmt) => self.render_case(stmt),
            Stmt::Default(stmt) => self.render_default(stmt),
            Stmt::Break(stmt) => self.render_simple("Break", stmt),
            Stmt::Continue(stmt) => self.render_simple("Continue", stmt),
            Stmt::Empty(stmt) => self.render_simple("Empty", stmt),
            Stmt::StaticAssert(assertion) => {
                let root = self.node_with_span("StaticAssert", assertion.span);
                let condition = self.render_expr(&assertion.condition);
                self.edge(root, condition, Some("condition"));
                root
            }
        }
    }

    fn render_expression_stmt(&mut self, statement: &ExpressionStmt) -> usize {
        let root = self.node_with_span("ExpressionStmt", statement.span);
        let expr = self.render_expr(&statement.expr);
        self.edge(root, expr, None);
        root
    }

    fn render_if(&mut self, statement: &IfStmt) -> usize {
        let root = self.node_with_span("If", statement.span);
        let condition = self.render_expr(&statement.condition);
        self.edge(root, condition, Some("condition"));
        let then_branch = self.render_stmt(&statement.then_branch);
        self.edge(root, then_branch, Some("then"));
        if let Some(else_branch) = &statement.else_branch {
            let child = self.render_stmt(else_branch);
            self.edge(root, child, Some("else"));
        }
        root
    }

    fn render_switch(&mut self, statement: &SwitchStmt) -> usize {
        let root = self.node_with_span("Switch", statement.span);
        let condition = self.render_expr(&statement.condition);
        self.edge(root, condition, Some("condition"));
        let body = self.render_stmt(&statement.body);
        self.edge(root, body, Some("body"));
        root
    }

    fn render_return(&mut self, statement: &crate::ReturnStmt) -> usize {
        let root = self.node_with_span("Return", statement.span);
        if let Some(value) = &statement.value {
            let expr = self.render_expr(value);
            self.edge(root, expr, Some("value"));
        }
        root
    }

    fn render_while(&mut self, statement: &WhileStmt) -> usize {
        let root = self.node_with_span("While", statement.span);
        let condition = self.render_expr(&statement.condition);
        self.edge(root, condition, Some("condition"));
        let body = self.render_stmt(&statement.body);
        self.edge(root, body, Some("body"));
        root
    }

    fn render_do_while(&mut self, statement: &DoWhileStmt) -> usize {
        let root = self.node_with_span("DoWhile", statement.span);
        let body = self.render_stmt(&statement.body);
        self.edge(root, body, Some("body"));
        let condition = self.render_expr(&statement.condition);
        self.edge(root, condition, Some("condition"));
        root
    }

    fn render_for(&mut self, statement: &ForStmt) -> usize {
        let root = self.node_with_span("For", statement.span);
        if let Some(initializer) = &statement.initializer {
            let child = self.render_expr(initializer);
            self.edge(root, child, Some("init"));
        }
        if let Some(condition) = &statement.condition {
            let child = self.render_expr(condition);
            self.edge(root, child, Some("condition"));
        }
        if let Some(update) = &statement.update {
            let child = self.render_expr(update);
            self.edge(root, child, Some("update"));
        }
        let body = self.render_stmt(&statement.body);
        self.edge(root, body, Some("body"));
        root
    }

    fn render_case(&mut self, statement: &CaseStmt) -> usize {
        let root = self.node_with_span("Case", statement.span);
        let expr = self.render_expr(&statement.value);
        self.edge(root, expr, Some("value"));
        root
    }

    fn render_default(&mut self, statement: &DefaultStmt) -> usize {
        self.node_with_span("Default", statement.span)
    }

    fn render_simple(&mut self, kind: &str, statement: &SimpleStmt) -> usize {
        self.node_with_span(kind, statement.span)
    }

    fn render_expr(&mut self, expr: &Expr) -> usize {
        match &expr.kind {
            ExprKind::Literal(literal) => {
                self.node_with_span(format!("Literal {}", literal_label(literal)), expr.span)
            }
            ExprKind::Identifier(name) => {
                self.node_with_span(format!("Identifier {}", name), expr.span)
            }
            ExprKind::ScopedIdentifier {
                scope,
                name,
            } => self.node_with_span(format!("ScopedIdentifier {scope}::{name}"), expr.span),
            ExprKind::Match(expression) => {
                let root = self.node_with_span("Match", expr.span);
                let value = self.render_expr(&expression.value);
                self.edge(root, value, Some("value"));
                for arm in &expression.arms {
                    let arm_node = self.node_with_span("MatchArm", arm.span);
                    self.edge(root, arm_node, Some("arm"));
                    if let Some(guard) = &arm.guard {
                        let guard = self.render_expr(guard);
                        self.edge(arm_node, guard, Some("guard"));
                    }
                    match &arm.body {
                        crate::MatchArmBody::Expr(body) => {
                            let body = self.render_expr(body);
                            self.edge(arm_node, body, Some("body"));
                        }
                        crate::MatchArmBody::Block(block) => {
                            for statement in &block.statements {
                                let statement = self.render_stmt(statement);
                                self.edge(arm_node, statement, Some("statement"));
                            }
                            if let Some(tail) = &block.tail {
                                let tail = self.render_expr(tail);
                                self.edge(arm_node, tail, Some("tail"));
                            }
                        }
                    }
                }
                root
            }
            ExprKind::Call {
                callee,
                arguments,
            } => {
                let root = self.node_with_span("Call", expr.span);
                let callee_id = self.render_expr(callee);
                self.edge(root, callee_id, Some("callee"));
                for argument in arguments {
                    let child = self.render_expr(argument);
                    self.edge(root, child, Some("arg"));
                }
                root
            }
            ExprKind::FieldAccess {
                base,
                field,
            } => {
                let root = self.node_with_span(format!("Field {}", field), expr.span);
                let child = self.render_expr(base);
                self.edge(root, child, Some("base"));
                root
            }
            ExprKind::Unary {
                op,
                expr: inner,
            } => {
                let root = self.node_with_span(format!("Unary {}", unary_label(*op)), expr.span);
                let child = self.render_expr(inner);
                self.edge(root, child, None);
                root
            }
            ExprKind::Binary {
                op,
                left,
                right,
            } => {
                let root = self.node_with_span(format!("Binary {}", binary_label(*op)), expr.span);
                let left_id = self.render_expr(left);
                let right_id = self.render_expr(right);
                self.edge(root, left_id, Some("left"));
                self.edge(root, right_id, Some("right"));
                root
            }
            ExprKind::Conditional {
                condition,
                when_true,
                when_false,
            } => {
                let root = self.node_with_span("Conditional", expr.span);
                let condition_id = self.render_expr(condition);
                let true_id = self.render_expr(when_true);
                let false_id = self.render_expr(when_false);
                self.edge(root, condition_id, Some("condition"));
                self.edge(root, true_id, Some("then"));
                self.edge(root, false_id, Some("else"));
                root
            }
            ExprKind::Assignment {
                op,
                left,
                right,
            } => {
                let root =
                    self.node_with_span(format!("Assignment {}", assignment_label(*op)), expr.span);
                let left_id = self.render_expr(left);
                let right_id = self.render_expr(right);
                self.edge(root, left_id, Some("left"));
                self.edge(root, right_id, Some("right"));
                root
            }
        }
    }

    fn render_type(&mut self, ty: &TypeSpec) -> usize {
        self.node_with_span(
            format!("Type {}", type_label(&ty.kind, ty.is_const)),
            ty.span,
        )
    }

    fn node(&mut self, label: impl Into<String>) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        let label = label.into();
        self.nodes.push(format!(
            "n{id} [label=\"{}\", {}];",
            escape(&label),
            node_style(&label)
        ));
        id
    }

    fn node_with_span(&mut self, label: impl Into<String>, span: crate::Span) -> usize {
        let mut full = label.into();
        if let Some(source_map) = self.source_map
            && let Some(file) = source_map.get(span.source_id)
        {
            full.push('\n');
            let display_name = Path::new(&file.name)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&file.name);
            full.push_str(display_name);
            if let Some(location) = file.location(span.start) {
                full.push(':');
                full.push_str(&location.line.to_string());
                full.push(':');
                full.push_str(&location.column.to_string());
            }
        }
        self.node(full)
    }

    fn edge(&mut self, from: usize, to: usize, label: Option<&str>) {
        match label {
            Some(label) => self
                .edges
                .push(format!("n{from} -> n{to} [label=\"{}\"];", escape(label))),
            None => self.edges.push(format!("n{from} -> n{to};")),
        }
    }
}

fn escape(input: &str) -> String {
    input
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

fn node_style(label: &str) -> &'static str {
    let first_line = label.lines().next().unwrap_or(label);
    if first_line == "Script" {
        "fillcolor=\"#0f172a\", color=\"#0f172a\", fontcolor=\"#ffffff\", penwidth=1.5"
    } else if first_line.starts_with("Function ") || first_line.starts_with("Struct ") {
        "fillcolor=\"#dbeafe\", color=\"#3b82f6\", penwidth=1.25"
    } else if matches!(
        first_line,
        "If" | "Switch" | "While" | "DoWhile" | "For" | "Case" | "Default"
    ) {
        "fillcolor=\"#ffedd5\", color=\"#f97316\""
    } else if first_line.starts_with("Literal ")
        || first_line.starts_with("Identifier ")
        || first_line.starts_with("Type ")
    {
        "fillcolor=\"#ecfdf5\", color=\"#10b981\""
    } else if first_line == "Call"
        || first_line.starts_with("Binary ")
        || first_line.starts_with("Unary ")
        || first_line.starts_with("Assignment ")
        || first_line == "Conditional"
    {
        "fillcolor=\"#f3e8ff\", color=\"#a855f7\""
    } else if first_line == "Global"
        || first_line == "Declaration"
        || first_line.starts_with("Var ")
        || first_line.starts_with("Param ")
    {
        "fillcolor=\"#fef9c3\", color=\"#ca8a04\""
    } else {
        "fillcolor=\"#ffffff\", color=\"#94a3b8\""
    }
}

fn type_label(kind: &TypeKind, is_const: bool) -> String {
    let prefix = if is_const { "const " } else { "" };
    match kind {
        TypeKind::Void => format!("{prefix}void"),
        TypeKind::Int => format!("{prefix}int"),
        TypeKind::Float => format!("{prefix}float"),
        TypeKind::String => format!("{prefix}string"),
        TypeKind::Object => format!("{prefix}object"),
        TypeKind::Vector => format!("{prefix}vector"),
        TypeKind::Struct(name) => format!("{prefix}struct {name}"),
        TypeKind::EngineStructure(name) => format!("{prefix}{name}"),
        TypeKind::Named(name) => format!("{prefix}{name}"),
    }
}

fn literal_label(literal: &Literal) -> String {
    match literal {
        Literal::Integer(value) => value.to_string(),
        Literal::Float(value) => value.to_string(),
        Literal::String(value) => format!("{value:?}"),
        Literal::ObjectSelf => "OBJECT_SELF".to_string(),
        Literal::ObjectInvalid => "OBJECT_INVALID".to_string(),
        Literal::LocationInvalid => "LOCATION_INVALID".to_string(),
        Literal::Json(value) => format!("json({value})"),
        Literal::Vector(value) => format!("<{}, {}, {}>", value[0], value[1], value[2]),
        Literal::Magic(value) => format!("{value:?}"),
    }
}

fn unary_label(op: UnaryOp) -> &'static str {
    match op {
        UnaryOp::Negate => "-",
        UnaryOp::OnesComplement => "~",
        UnaryOp::BooleanNot => "!",
        UnaryOp::PreIncrement => "++pre",
        UnaryOp::PreDecrement => "--pre",
        UnaryOp::PostIncrement => "++post",
        UnaryOp::PostDecrement => "--post",
    }
}

fn binary_label(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::Modulus => "%",
        BinaryOp::Add => "+",
        BinaryOp::Subtract => "-",
        BinaryOp::ShiftLeft => "<<",
        BinaryOp::ShiftRight => ">>",
        BinaryOp::UnsignedShiftRight => ">>>",
        BinaryOp::GreaterEqual => ">=",
        BinaryOp::GreaterThan => ">",
        BinaryOp::LessThan => "<",
        BinaryOp::LessEqual => "<=",
        BinaryOp::NotEqual => "!=",
        BinaryOp::EqualEqual => "==",
        BinaryOp::BooleanAnd => "&",
        BinaryOp::ExclusiveOr => "^",
        BinaryOp::InclusiveOr => "|",
        BinaryOp::LogicalAnd => "&&",
        BinaryOp::LogicalOr => "||",
    }
}

fn assignment_label(op: AssignmentOp) -> &'static str {
    match op {
        AssignmentOp::Assign => "=",
        AssignmentOp::AssignMinus => "-=",
        AssignmentOp::AssignPlus => "+=",
        AssignmentOp::AssignMultiply => "*=",
        AssignmentOp::AssignDivide => "/=",
        AssignmentOp::AssignModulus => "%=",
        AssignmentOp::AssignAnd => "&=",
        AssignmentOp::AssignXor => "^=",
        AssignmentOp::AssignOr => "|=",
        AssignmentOp::AssignShiftLeft => "<<=",
        AssignmentOp::AssignShiftRight => ">>=",
        AssignmentOp::AssignUnsignedShiftRight => ">>>=",
    }
}

#[cfg(test)]
mod tests {
    use super::render_script_graphviz;
    use crate::{SourceId, parse_text};

    #[test]
    fn renders_graphviz_for_simple_script() -> Result<(), Box<dyn std::error::Error>> {
        let script = parse_text(
            SourceId::new(0),
            r#"void main() { int value = 1; PrintInteger(value); }"#,
            None,
        )?;

        let dot = render_script_graphviz(&script, None);
        assert!(dot.contains("digraph nwscript"));
        assert!(dot.contains("Function main"));
        assert!(dot.contains("Call"));
        Ok(())
    }
}
