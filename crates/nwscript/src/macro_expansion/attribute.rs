use super::*;

/// Erases compiler-only nwnrs attribute syntax after NSS macros have expanded.
///
/// Event discovery and validation belong to the nwnrs project macro. This
/// frontend pass only prevents recognized compiler attributes from reaching
/// the ordinary NWScript parser.
pub(super) fn erase_nwnrs_macro_attributes(
    stream: &mut NwTokenStream,
) -> Result<(), MacroExpansionError> {
    let mut output = Vec::new();
    let mut position = 0;
    while position < stream.trees.len() {
        let Some(NwTokenTree::Token(hash)) = stream.trees.get(position) else {
            if let Some(tree) = stream.trees.get(position).cloned() {
                output.push(tree);
            }
            position += 1;
            continue;
        };
        if hash.kind != TokenKind::Hash {
            output.push(NwTokenTree::Token(hash.clone()));
            position += 1;
            continue;
        }
        let Some(NwTokenTree::Group(attribute)) = stream.trees.get(position + 1) else {
            return Err(MacroExpansionError::new(
                hash.span,
                "`#` must be followed by a compiler attribute in brackets",
            ));
        };
        if attribute.delimiter != NwDelimiter::Bracket {
            return Err(MacroExpansionError::new(
                attribute.span(),
                "compiler attributes must use `#[...]`",
            ));
        }
        validate_nwnrs_attribute_route(attribute)?;
        position += 2;
    }
    stream.trees = output;
    Ok(())
}

fn validate_nwnrs_attribute_route(attribute: &NwTokenGroup) -> Result<(), MacroExpansionError> {
    if is_nwnrs_event_attribute(attribute) {
        Ok(())
    } else {
        Err(MacroExpansionError::new(
            attribute.span(),
            "only `#[nwnrs::event::<subject>::<phase>]` compiler attributes are supported",
        ))
    }
}

/// Returns whether a balanced bracket group has the canonical nwnrs event
/// attribute route. Full subject, phase, and relationship validation belongs
/// to the project macro.
#[must_use]
pub fn is_nwnrs_event_attribute(attribute: &NwTokenGroup) -> bool {
    if attribute.delimiter != NwDelimiter::Bracket {
        return false;
    }
    let trees = attribute.stream.trees();
    if trees.len() != 10 && trees.len() != 11 {
        return false;
    }
    matches!(
        trees,
        [
            NwTokenTree::Token(namespace),
            NwTokenTree::Token(colon_1),
            NwTokenTree::Token(colon_2),
            NwTokenTree::Token(kind),
            NwTokenTree::Token(colon_3),
            NwTokenTree::Token(colon_4),
            NwTokenTree::Token(subject),
            NwTokenTree::Token(colon_5),
            NwTokenTree::Token(colon_6),
            NwTokenTree::Token(phase),
            ..
        ] if namespace.text == "nwnrs"
            && kind.text == "event"
            && subject.kind == TokenKind::Identifier
            && phase.kind == TokenKind::Identifier
            && [colon_1, colon_2, colon_3, colon_4, colon_5, colon_6]
                .iter()
                .all(|token| token.kind == TokenKind::Colon)
            && trees.get(10).is_none_or(|tree| matches!(
                tree,
                NwTokenTree::Group(group)
                    if group.delimiter == NwDelimiter::Parenthesis
            ))
    )
}

#[cfg(test)]
mod event_attribute_tests {
    use super::erase_nwnrs_macro_attributes;
    use crate::{
        MacroExpansionOptions, MacroRegistry, NwTokenStream, SourceId, expand_source_macros,
        lex_text, render_nwscript_tokens,
    };

    #[test]
    fn collects_and_erases_module_load_attributes() -> Result<(), Box<dyn std::error::Error>> {
        let tokens = lex_text(
            SourceId::new(0),
            "#[nwnrs::event::module_load::before]\nvoid ProjectStart(json jEvent) {}",
        )?;
        let mut stream = NwTokenStream::from_tokens(&tokens)?;
        erase_nwnrs_macro_attributes(&mut stream)?;
        let rendered = render_nwscript_tokens(&stream);
        assert!(!rendered.contains("#["));
        assert!(rendered.contains("ProjectStart"));
        Ok(())
    }

    #[test]
    fn rejects_unknown_compiler_attribute_routes() -> Result<(), Box<dyn std::error::Error>> {
        let tokens = lex_text(
            SourceId::new(0),
            "#[other::attribute(value)]\nvoid ProjectStart() {}",
        )?;
        let mut stream = NwTokenStream::from_tokens(&tokens)?;
        let error = erase_nwnrs_macro_attributes(&mut stream)
            .expect_err("unknown attribute route should be rejected");
        assert!(
            error
                .message
                .contains("only `#[nwnrs::event::<subject>::<phase>]`")
        );
        Ok(())
    }

    #[test]
    fn erases_event_attributes_emitted_by_bang_macros() -> Result<(), Box<dyn std::error::Error>> {
        let tokens = lex_text(
            SourceId::new(0),
            r#"
                macro_rules! handler {
                    () => {
                        #[nwnrs::event::module_load::before]
                        void Generated(json event) {}
                    };
                }
                handler!()
            "#,
        )?;
        let expanded = expand_source_macros(
            tokens,
            &mut MacroRegistry::new(),
            MacroExpansionOptions::default(),
        )?;
        let stream = NwTokenStream::from_tokens(&expanded)?;
        let rendered = render_nwscript_tokens(&stream);
        assert!(!rendered.contains("#["));
        assert!(rendered.contains("Generated"));
        Ok(())
    }
}
