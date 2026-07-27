use super::*;

mod declarations;
mod expressions;
mod statements;
mod types;

pub(super) struct Analyzer<'a> {
    script:            &'a Script,
    options:           SemanticOptions,
    builtin_constants: BTreeMap<String, ConstantValue>,
    global_constants:  BTreeMap<String, ConstantValue>,
    functions:         BTreeMap<String, FunctionInfo>,
    structs:           BTreeMap<String, SemanticStruct>,
    globals:           BTreeMap<String, SemanticGlobal>,
    enums:             BTreeMap<String, SemanticEnum>,
    aliases:           BTreeMap<String, SemanticType>,
    cancellation:      Option<&'a CancellationToken>,
}

impl<'a> Analyzer<'a> {
    pub(super) fn new(
        script: &'a Script,
        langspec: Option<&LangSpec>,
        options: SemanticOptions,
    ) -> Self {
        Self::new_impl(script, langspec, options, None)
    }

    pub(super) fn new_with_cancellation(
        script: &'a Script,
        langspec: Option<&LangSpec>,
        options: SemanticOptions,
        cancellation: &'a CancellationToken,
    ) -> Self {
        Self::new_impl(script, langspec, options, Some(cancellation))
    }

    fn new_impl(
        script: &'a Script,
        langspec: Option<&LangSpec>,
        options: SemanticOptions,
        cancellation: Option<&'a CancellationToken>,
    ) -> Self {
        let mut builtin_constants = BTreeMap::new();
        let mut functions = BTreeMap::new();

        if let Some(langspec) = langspec {
            for constant in &langspec.constants {
                if let Some(value) = constant_from_builtin_value(&constant.value) {
                    builtin_constants.insert(constant.name.clone(), value);
                }
            }

            for function in &langspec.functions {
                let parameters = function
                    .parameters
                    .iter()
                    .map(|parameter| SemanticParameter {
                        name:        parameter.name.clone(),
                        ty:          semantic_type_from_builtin_type(&parameter.ty),
                        is_optional: parameter.default.is_some(),
                        default:     parameter
                            .default
                            .as_ref()
                            .and_then(literal_from_builtin_value),
                    })
                    .collect::<Vec<_>>();

                functions.insert(
                    function.name.clone(),
                    FunctionInfo {
                        signature:        SemanticFunction {
                            name: function.name.clone(),
                            return_type: semantic_type_from_builtin_type(&function.return_type),
                            parameters,
                            has_body: false,
                            is_builtin: true,
                        },
                        declaration_span: crate::Span::new(crate::SourceId::new(0), 0, 0),
                    },
                );
            }
        }

        Self {
            script,
            options,
            builtin_constants,
            global_constants: BTreeMap::new(),
            functions,
            structs: BTreeMap::new(),
            globals: BTreeMap::new(),
            enums: BTreeMap::new(),
            aliases: BTreeMap::new(),
            cancellation,
        }
    }

    pub(super) fn analyze(mut self) -> Result<SemanticModel, SemanticError> {
        self.check_cancelled(crate::Span::new(crate::SourceId::new(0), 0, 0))?;
        self.collect_plain_constants_for_enum_discriminants()?;
        self.check_cancelled(crate::Span::new(crate::SourceId::new(0), 0, 0))?;
        self.collect_enums()?;
        self.check_cancelled(crate::Span::new(crate::SourceId::new(0), 0, 0))?;
        self.collect_type_aliases()?;
        self.check_cancelled(crate::Span::new(crate::SourceId::new(0), 0, 0))?;
        self.collect_structs()?;
        self.check_cancelled(crate::Span::new(crate::SourceId::new(0), 0, 0))?;
        self.collect_const_globals_for_function_defaults()?;
        self.check_cancelled(crate::Span::new(crate::SourceId::new(0), 0, 0))?;
        self.collect_functions()?;
        self.check_cancelled(crate::Span::new(crate::SourceId::new(0), 0, 0))?;
        self.collect_globals()?;
        self.check_cancelled(crate::Span::new(crate::SourceId::new(0), 0, 0))?;
        self.validate_top_level_static_assertions()?;
        self.check_cancelled(crate::Span::new(crate::SourceId::new(0), 0, 0))?;
        self.analyze_function_bodies()?;
        self.check_cancelled(crate::Span::new(crate::SourceId::new(0), 0, 0))?;
        self.validate_entrypoint()?;

        Ok(SemanticModel {
            enums:     self.enums,
            aliases:   self.aliases,
            structs:   self.structs,
            globals:   self.globals,
            functions: self
                .functions
                .into_iter()
                .map(|(name, info)| (name, info.signature))
                .collect(),
        })
    }

    fn check_cancelled(&self, span: crate::Span) -> Result<(), SemanticError> {
        if self
            .cancellation
            .is_some_and(CancellationToken::is_cancelled)
        {
            Err(SemanticError::new(
                CompilerErrorCode::FatalCompilerError,
                span,
                "operation cancelled",
            ))
        } else {
            Ok(())
        }
    }
}
