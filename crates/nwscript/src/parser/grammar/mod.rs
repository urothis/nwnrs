use super::*;

mod declarations;
mod expressions;
mod helpers;
mod statements;

impl<'a> Parser<'a> {
    pub(super) fn new(
        tokens: Vec<Token>,
        langspec: Option<&'a LangSpec>,
        limits: crate::CompilerResourceLimits,
        cancellation: Option<&'a crate::CancellationToken>,
    ) -> Self {
        let engine_structures = langspec
            .map(|spec| {
                spec.engine_structures
                    .iter()
                    .map(String::as_str)
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();
        Self {
            tokens,
            position: 0,
            engine_structures,
            limits,
            cancellation,
            syntax_depth: 0,
        }
    }

    pub(super) fn parse_script(mut self) -> Result<Script, ParserError> {
        let mut items = Vec::new();
        while !self.at_eof() {
            self.check_operation()?;
            if self.matches_keyword(Keyword::Include) {
                items.push(TopLevelItem::Include(self.parse_include_directive()?));
                continue;
            }
            if self.matches_identifier_text("enum") {
                items.push(TopLevelItem::Enum(self.parse_enum_declaration()?));
                continue;
            }
            if self.matches_identifier_text("type") {
                items.push(TopLevelItem::TypeAlias(self.parse_type_alias()?));
                continue;
            }
            if self.matches_identifier_text("static_assert") {
                items.push(TopLevelItem::StaticAssert(self.parse_static_assert()?));
                continue;
            }
            items.push(self.parse_top_level_item()?);
        }
        Ok(Script {
            items,
        })
    }
}
