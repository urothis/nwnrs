use super::{ExprKind, Literal, ParseError, Stmt, TopLevelItem, parse_resolved_script, parse_text};
use crate::{InMemoryScriptResolver, LangSpec, SourceFile, SourceId, SourceLoadOptions, TypeKind};

fn test_langspec() -> LangSpec {
    LangSpec {
        engine_num_structures: 3,
        engine_structures:     vec![
            "effect".to_string(),
            "location".to_string(),
            "json".to_string(),
        ],
        constants:             Vec::new(),
        functions:             Vec::new(),
    }
}

#[test]
fn parses_extended_enums_aliases_and_static_assertions() {
    let script = parse_text(
        SourceId::new(0),
        "enum LogLevel { Trace, #[default] #[alias(LOG_INFO)] Info = Trace + 2, }\nenum \
         EventPhase : string { Before = \"before\", After = \"after\" }\ntype Level = \
         LogLevel;\nstatic_assert(LogLevel::Info == LogLevel::Info, \"enum mismatch\");\nvoid \
         main() { Level level = LogLevel::Info; static_assert(1); }",
        Some(&test_langspec()),
    )
    .expect("parse extended enum syntax");

    let Some(TopLevelItem::Enum(level)) = script.items.first() else {
        panic!("expected integer enum");
    };
    assert_eq!(level.name, "LogLevel");
    assert_eq!(level.variants.len(), 2);
    assert!(
        level
            .variants
            .get(1)
            .is_some_and(|variant| variant.is_default)
    );
    assert_eq!(
        level
            .variants
            .get(1)
            .and_then(|variant| variant.aliases.first())
            .map(|alias| alias.name.as_str()),
        Some("LOG_INFO")
    );
    assert!(matches!(script.items.get(1), Some(TopLevelItem::Enum(_))));
    assert!(matches!(
        script.items.get(2),
        Some(TopLevelItem::TypeAlias(_))
    ));
    assert!(matches!(
        script.items.get(3),
        Some(TopLevelItem::StaticAssert(_))
    ));
}

#[test]
fn rejects_const_declaration_modifiers_in_type_aliases() {
    let source_id = SourceId::new(32);
    let source = "type ImmutableInt = const int;\nvoid main() {}";
    let error = parse_text(source_id, source, Some(&test_langspec()))
        .expect_err("const type alias should fail");
    let ParseError::Parse(error) = error else {
        panic!("expected parser error");
    };
    assert_eq!(error.code, crate::CompilerErrorCode::InvalidTypeAlias);
    let source_file = SourceFile::new(source_id, "const-alias.nss", source);
    assert_eq!(
        source_file.span_text(error.span),
        Some("const int".to_string())
    );
}

#[test]
fn parses_top_level_items_using_upstream_shapes() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(1),
        "#include \"x\"\nint VALUE = 1;\nvoid main(int n = -1) { return; }\neffect fx;",
        Some(&test_langspec()),
    )?;

    assert_eq!(script.items.len(), 4);
    assert!(matches!(
        script.items.first(),
        Some(TopLevelItem::Include(_))
    ));
    assert!(matches!(script.items.get(1), Some(TopLevelItem::Global(_))));
    assert!(matches!(
        script.items.get(2),
        Some(TopLevelItem::Function(_))
    ));
    match script.items.get(3) {
        Some(TopLevelItem::Global(decl)) => {
            assert_eq!(
                decl.ty.kind,
                TypeKind::EngineStructure("effect".to_string())
            );
            assert_eq!(
                decl.declarators
                    .first()
                    .map(|declarator| declarator.name.as_str()),
                Some("fx")
            );
        }
        other => {
            return Err(std::io::Error::other(format!(
                "expected global declaration, got {other:?}"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn parses_struct_definitions_and_fields() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(2),
        "struct Foo { int a, b; vector dir; struct Bar child; };",
        Some(&test_langspec()),
    )?;

    match script.items.first() {
        Some(TopLevelItem::Global(decl)) => {
            return Err(std::io::Error::other(format!(
                "expected struct declaration, got global {decl:?}"
            ))
            .into());
        }
        Some(TopLevelItem::Struct(def)) => {
            assert_eq!(def.name, "Foo");
            assert_eq!(def.fields.len(), 3);
            assert_eq!(def.fields.first().map(|field| field.names.len()), Some(2));
            assert_eq!(
                def.fields.get(1).map(|field| field.ty.kind.clone()),
                Some(TypeKind::Vector)
            );
            assert_eq!(
                def.fields.get(2).map(|field| field.ty.kind.clone()),
                Some(TypeKind::Struct("Bar".to_string()))
            );
        }
        other => {
            return Err(std::io::Error::other(format!(
                "expected struct declaration, got {other:?}"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn parses_expression_precedence_and_postfix_forms() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(3),
        "void main() { a >>= b + c * d.e++ ? f : g; }",
        Some(&test_langspec()),
    )?;

    let function = match script.items.first() {
        Some(TopLevelItem::Function(function)) => function,
        other => {
            return Err(std::io::Error::other(format!("expected function, got {other:?}")).into());
        }
    };
    let body = function
        .body
        .as_ref()
        .ok_or_else(|| std::io::Error::other("function body must exist"))?;
    let stmt = match body.statements.first() {
        Some(Stmt::Expression(stmt)) => stmt,
        other => {
            return Err(std::io::Error::other(format!(
                "expected expression statement, got {other:?}"
            ))
            .into());
        }
    };

    match &stmt.expr.kind {
        ExprKind::Assignment {
            ..
        } => {}
        other => {
            return Err(std::io::Error::other(format!(
                "expected assignment expression, got {other:?}"
            ))
            .into());
        }
    }
    Ok(())
}

#[test]
fn parses_chained_prefix_unary_operators() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(30),
        "void main() { int a = 1; int b = !!~-+a; }",
        Some(&test_langspec()),
    )?;

    let function = match script.items.first() {
        Some(TopLevelItem::Function(function)) => function,
        other => {
            return Err(std::io::Error::other(format!("expected function, got {other:?}")).into());
        }
    };
    assert_eq!(
        function.body.as_ref().map(|body| body.statements.len()),
        Some(2)
    );
    Ok(())
}

#[test]
fn missing_expression_semicolon_points_to_the_expression_at_a_block_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let source_id = SourceId::new(31);
    let source = "void main() {\n    asd\n}\n";
    let error = parse_text(source_id, source, Some(&test_langspec()))
        .expect_err("missing expression semicolon should fail");
    let ParseError::Parse(error) = error else {
        return Err(std::io::Error::other("expected parser error").into());
    };
    assert_eq!(
        error.code,
        crate::CompilerErrorCode::NoSemicolonAfterExpression
    );
    let source_file = SourceFile::new(source_id, "missing-semicolon.nss", source);
    assert_eq!(source_file.span_text(error.span), Some("asd".to_string()));
    Ok(())
}

#[test]
fn eof_parser_errors_always_point_to_visible_source_text() -> Result<(), Box<dyn std::error::Error>>
{
    let malformed_sources = [
        ("function parameter list", "void main("),
        ("function block", "void main() {\n    int value = 1;\n"),
        (
            "declaration semicolon",
            "void main() {\n    int value = 1\n",
        ),
        ("return semicolon", "int main() {\n    return 1\n"),
        ("call argument list", "void main() {\n    Call(1"),
        ("if condition", "void main() {\n    if (1"),
        ("enum body", "enum State { Ready"),
        ("type alias", "type State = int"),
        ("static assertion", "static_assert(1"),
        (
            "match body",
            "void main() {\n    int value = match (1) { 1 => 2",
        ),
    ];

    for (name, source) in malformed_sources {
        let source_id = SourceId::new(32);
        let error = parse_text(source_id, source, Some(&test_langspec()))
            .expect_err("malformed source should fail");
        let ParseError::Parse(error) = error else {
            return Err(std::io::Error::other(format!("{name} produced a non-parser error")).into());
        };
        let source_file = SourceFile::new(source_id, format!("{name}.nss"), source);
        let selected = source_file
            .span_bytes(error.span)
            .ok_or_else(|| std::io::Error::other(format!("invalid span for {name}")))?;
        assert!(
            !selected.is_empty() && selected.iter().any(|byte| !byte.is_ascii_whitespace()),
            "{name} pointed at empty or whitespace source: {:?}",
            error.span
        );
    }
    Ok(())
}

#[test]
fn parses_control_flow_statements() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(4),
        "void main() { if (a) { return; } else { while (b) { continue; } } for (i = 0; i < 3; i \
         += 1) { break; } switch (n) { case 1: break; default: return; } do { n -= 1; } while \
         (n); }",
        Some(&test_langspec()),
    )?;

    let function = match script.items.first() {
        Some(TopLevelItem::Function(function)) => function,
        other => {
            return Err(std::io::Error::other(format!("expected function, got {other:?}")).into());
        }
    };
    let body = function
        .body
        .as_ref()
        .ok_or_else(|| std::io::Error::other("function body must exist"))?;
    assert!(matches!(body.statements.first(), Some(Stmt::If(_))));
    assert!(matches!(body.statements.get(1), Some(Stmt::For(_))));
    assert!(matches!(body.statements.get(2), Some(Stmt::Switch(_))));
    assert!(matches!(body.statements.get(3), Some(Stmt::DoWhile(_))));
    Ok(())
}

#[test]
fn preserves_magic_and_vector_literals() -> Result<(), Box<dyn std::error::Error>> {
    let script = parse_text(
        SourceId::new(5),
        "void main() { string a = __FILE__; vector v = [1.0, 2.0]; json j = JSON_OBJECT; }",
        Some(&test_langspec()),
    )?;

    let function = match script.items.first() {
        Some(TopLevelItem::Function(function)) => function,
        other => {
            return Err(std::io::Error::other(format!("expected function, got {other:?}")).into());
        }
    };
    let body = function
        .body
        .as_ref()
        .ok_or_else(|| std::io::Error::other("function body must exist"))?;
    match body.statements.first() {
        Some(Stmt::Declaration(decl)) => match decl
            .declarators
            .first()
            .and_then(|declarator| declarator.initializer.as_ref())
        {
            Some(expr) => assert!(matches!(expr.kind, ExprKind::Literal(Literal::Magic(_)))),
            None => return Err(std::io::Error::other("expected initializer").into()),
        },
        other => {
            return Err(
                std::io::Error::other(format!("expected declaration, got {other:?}")).into(),
            );
        }
    }
    Ok(())
}

#[test]
fn rejects_null_statement_after_if_like_upstream() -> Result<(), Box<dyn std::error::Error>> {
    let error = parse_text(
        SourceId::new(6),
        "void main() { if (TRUE) ; }",
        Some(&test_langspec()),
    )
    .expect_err("parser should reject null if body");

    match error {
        ParseError::Parse(error) => {
            assert_eq!(
                error.code,
                crate::CompilerErrorCode::IfConditionCannotBeFollowedByANullStatement
            );
        }
        other => {
            return Err(
                std::io::Error::other(format!("expected parse error, got {other:?}")).into(),
            );
        }
    }
    Ok(())
}

#[test]
fn parses_resolved_script_through_includes_and_object_like_defines()
-> Result<(), Box<dyn std::error::Error>> {
    let mut resolver = InMemoryScriptResolver::new();
    resolver.insert_source(
        "root",
        br#"#define BASE 2
#include "util"
int main_value = UTIL_PLUS;
"#,
    );
    resolver.insert_source(
        "util",
        br#"#define UTIL_PLUS BASE + 3
int helper = UTIL_PLUS;
"#,
    );

    let script = parse_resolved_script(
        &resolver,
        "root",
        SourceLoadOptions::default(),
        Some(&test_langspec()),
    )?;

    assert_eq!(script.items.len(), 2);
    assert!(matches!(
        script.items.first(),
        Some(TopLevelItem::Global(_))
    ));
    assert!(matches!(script.items.get(1), Some(TopLevelItem::Global(_))));
    Ok(())
}

#[test]
fn parses_full_constant_expressions_in_parameter_defaults() -> Result<(), Box<dyn std::error::Error>>
{
    let script = parse_text(
        SourceId::new(7),
        "const int BASE = 1; void main(int nValue = BASE + 2 * 3) { return; }",
        Some(&test_langspec()),
    )?;

    let function = match script.items.get(1) {
        Some(TopLevelItem::Function(function)) => function,
        other => {
            return Err(std::io::Error::other(format!("expected function, got {other:?}")).into());
        }
    };
    let default = function
        .parameters
        .first()
        .and_then(|parameter| parameter.default.as_ref())
        .ok_or_else(|| std::io::Error::other("expected parameter default"))?;
    assert!(matches!(default.kind, ExprKind::Binary { .. }));
    Ok(())
}
