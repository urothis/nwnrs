use super::*;

impl Analyzer<'_> {
    pub(super) fn collect_plain_constants_for_enum_discriminants(
        &mut self,
    ) -> Result<(), SemanticError> {
        let mut pending = self
            .script
            .items
            .iter()
            .filter_map(|item| {
                let TopLevelItem::Global(declaration) = item else {
                    return None;
                };
                if !declaration.ty.is_const
                    || !matches!(declaration.ty.kind, TypeKind::Int | TypeKind::String)
                {
                    return None;
                }
                Some((declaration, declaration.declarators.as_slice()))
            })
            .flat_map(|(declaration, declarators)| {
                declarators
                    .iter()
                    .map(move |declarator| (declaration, declarator))
            })
            .collect::<Vec<_>>();

        loop {
            self.check_cancelled(crate::Span::new(crate::SourceId::new(0), 0, 0))?;
            let mut progress = false;
            pending.retain(|(declaration, declarator)| {
                let value = declarator.initializer.as_ref().map_or_else(
                    || match declaration.ty.kind {
                        TypeKind::Int => Some(ConstantValue::Int(0)),
                        TypeKind::String => {
                            Some(ConstantValue::String(crate::ScriptString::default()))
                        }
                        _ => None,
                    },
                    |initializer| self.evaluate_constant_expr(initializer),
                );
                let expected = match declaration.ty.kind {
                    TypeKind::Int => SemanticType::Int,
                    TypeKind::String => SemanticType::String,
                    _ => return false,
                };
                if let Some(value) = value
                    && value.ty() == expected
                {
                    self.global_constants.insert(declarator.name.clone(), value);
                    progress = true;
                    false
                } else {
                    true
                }
            });
            if !progress || pending.is_empty() {
                break;
            }
        }
        Ok(())
    }

    pub(super) fn collect_enums(&mut self) -> Result<(), SemanticError> {
        for item in &self.script.items {
            let TopLevelItem::Enum(declaration) = item else {
                continue;
            };
            if self.enums.contains_key(&declaration.name) {
                return Err(SemanticError::new(
                    CompilerErrorCode::DuplicateEnumDefinition,
                    declaration.span,
                    format!("enum {:?} was defined more than once", declaration.name),
                ));
            }
            if self.functions.contains_key(&declaration.name) {
                return Err(SemanticError::new(
                    CompilerErrorCode::InvalidEnumDeclaration,
                    declaration.span,
                    format!(
                        "enum {:?} conflicts with an existing function name",
                        declaration.name
                    ),
                ));
            }
            if declaration.variants.is_empty() {
                return Err(SemanticError::new(
                    CompilerErrorCode::InvalidEnumDeclaration,
                    declaration.span,
                    format!(
                        "enum {:?} must declare at least one variant",
                        declaration.name
                    ),
                ));
            }

            let mut variants = Vec::new();
            let mut names = BTreeSet::new();
            let mut values = BTreeSet::new();
            let mut explicit_default = None;
            for variant in &declaration.variants {
                if !names.insert(variant.name.clone()) {
                    return Err(SemanticError::new(
                        CompilerErrorCode::DuplicateEnumDefinition,
                        variant.span,
                        format!(
                            "enum variant {}::{} was defined more than once",
                            declaration.name, variant.name
                        ),
                    ));
                }
                if variant.is_default && explicit_default.replace(variants.len()).is_some() {
                    return Err(SemanticError::new(
                        CompilerErrorCode::MultipleEnumDefaults,
                        variant.span,
                        format!(
                            "enum {:?} has more than one #[default] variant",
                            declaration.name
                        ),
                    ));
                }

                let value = match declaration.backing {
                    EnumBackingType::Int => {
                        let value = if let Some(expression) = &variant.value {
                            self.evaluate_enum_int_discriminant(
                                expression,
                                &declaration.name,
                                &variants,
                            )
                            .ok_or_else(|| {
                                SemanticError::new(
                                    CompilerErrorCode::InvalidEnumDiscriminant,
                                    expression.span,
                                    format!(
                                        "{}::{} requires a constant int discriminant",
                                        declaration.name, variant.name
                                    ),
                                )
                            })?
                        } else if let Some(previous) = variants.last() {
                            let SemanticEnumValue::Int(previous) = previous.value else {
                                return Err(SemanticError::new(
                                    CompilerErrorCode::InvalidEnumDiscriminant,
                                    variant.span,
                                    format!(
                                        "integer enum {} contains a non-integer discriminant",
                                        declaration.name
                                    ),
                                ));
                            };
                            previous.checked_add(1).ok_or_else(|| {
                                SemanticError::new(
                                    CompilerErrorCode::InvalidEnumDiscriminant,
                                    variant.span,
                                    format!(
                                        "automatic discriminant for {}::{} overflows int",
                                        declaration.name, variant.name
                                    ),
                                )
                            })?
                        } else {
                            0
                        };
                        SemanticEnumValue::Int(value)
                    }
                    EnumBackingType::String => {
                        let expression = variant.value.as_ref().ok_or_else(|| {
                            SemanticError::new(
                                CompilerErrorCode::InvalidEnumDiscriminant,
                                variant.span,
                                format!(
                                    "string enum variant {}::{} requires an explicit value",
                                    declaration.name, variant.name
                                ),
                            )
                        })?;
                        SemanticEnumValue::String(
                            self.evaluate_enum_string_discriminant(
                                expression,
                                &declaration.name,
                                &variants,
                            )
                            .ok_or_else(|| {
                                SemanticError::new(
                                    CompilerErrorCode::InvalidEnumDiscriminant,
                                    expression.span,
                                    format!(
                                        "{}::{} requires a constant string discriminant",
                                        declaration.name, variant.name
                                    ),
                                )
                            })?,
                        )
                    }
                };
                if !values.insert(value.clone()) {
                    return Err(SemanticError::new(
                        CompilerErrorCode::DuplicateEnumDefinition,
                        variant.span,
                        format!(
                            "enum {} assigns the same value to more than one variant",
                            declaration.name
                        ),
                    ));
                }

                let aliases = variant
                    .aliases
                    .iter()
                    .map(|alias| alias.name.clone())
                    .collect::<Vec<_>>();
                for alias in &variant.aliases {
                    if self.global_constants.contains_key(&alias.name)
                        || self.builtin_constants.contains_key(&alias.name)
                    {
                        return Err(SemanticError::new(
                            CompilerErrorCode::DuplicateEnumDefinition,
                            alias.span,
                            format!("enum alias {:?} was defined more than once", alias.name),
                        ));
                    }
                    self.global_constants.insert(
                        alias.name.clone(),
                        ConstantValue::Enum {
                            name:  declaration.name.clone(),
                            value: value.clone(),
                        },
                    );
                    self.globals.insert(
                        alias.name.clone(),
                        SemanticGlobal {
                            name:     alias.name.clone(),
                            ty:       SemanticType::Enum {
                                name:    declaration.name.clone(),
                                backing: declaration.backing,
                            },
                            is_const: true,
                        },
                    );
                }
                variants.push(SemanticEnumVariant {
                    name: variant.name.clone(),
                    value,
                    is_default: variant.is_default,
                    aliases,
                });
            }

            self.enums.insert(
                declaration.name.clone(),
                SemanticEnum {
                    name: declaration.name.clone(),
                    backing: declaration.backing,
                    variants,
                    default_variant: explicit_default.unwrap_or(0),
                },
            );
        }
        Ok(())
    }

    pub(super) fn collect_type_aliases(&mut self) -> Result<(), SemanticError> {
        let mut pending = BTreeMap::new();
        for item in &self.script.items {
            let TopLevelItem::TypeAlias(alias) = item else {
                continue;
            };
            if pending.insert(alias.name.clone(), alias).is_some()
                || self.enums.contains_key(&alias.name)
                || self.functions.contains_key(&alias.name)
            {
                return Err(SemanticError::new(
                    CompilerErrorCode::InvalidTypeAlias,
                    alias.span,
                    format!("type name {:?} was defined more than once", alias.name),
                ));
            }
        }

        while !pending.is_empty() {
            let mut resolved_name = None;
            for (name, alias) in &pending {
                if let Some(target) = self.try_resolve_alias_target(&alias.target) {
                    self.aliases.insert(name.clone(), target);
                    resolved_name = Some(name.clone());
                    break;
                }
            }
            let Some(name) = resolved_name else {
                let Some((_name, alias)) = pending.first_key_value() else {
                    break;
                };
                return Err(SemanticError::new(
                    CompilerErrorCode::InvalidTypeAlias,
                    alias.span,
                    format!(
                        "type alias {:?} is recursive or references an undefined type",
                        alias.name
                    ),
                ));
            };
            pending.remove(&name);
        }
        Ok(())
    }

    pub(super) fn try_resolve_alias_target(&self, target: &TypeSpec) -> Option<SemanticType> {
        match &target.kind {
            TypeKind::Void => Some(SemanticType::Void),
            TypeKind::Int => Some(SemanticType::Int),
            TypeKind::Float => Some(SemanticType::Float),
            TypeKind::String => Some(SemanticType::String),
            TypeKind::Object => Some(SemanticType::Object),
            TypeKind::Vector => Some(SemanticType::Vector),
            TypeKind::Struct(name) => self
                .script
                .items
                .iter()
                .any(|item| matches!(item, TopLevelItem::Struct(item) if item.name == *name))
                .then(|| SemanticType::Struct(name.clone())),
            TypeKind::EngineStructure(name) => Some(SemanticType::EngineStructure(name.clone())),
            TypeKind::Named(name) => self.aliases.get(name).cloned().or_else(|| {
                self.enums.get(name).map(|declaration| SemanticType::Enum {
                    name:    name.clone(),
                    backing: declaration.backing,
                })
            }),
        }
    }

    pub(super) fn evaluate_enum_int_discriminant(
        &self,
        expression: &Expr,
        enum_name: &str,
        variants: &[SemanticEnumVariant],
    ) -> Option<i32> {
        match &expression.kind {
            ExprKind::Literal(Literal::Integer(value)) => Some(*value),
            ExprKind::Identifier(name) => variants
                .iter()
                .find_map(|variant| {
                    (variant.name == *name).then_some(match variant.value {
                        SemanticEnumValue::Int(value) => Some(value),
                        SemanticEnumValue::String(_) => None,
                    })?
                })
                .or_else(|| match self.lookup_constant(name)? {
                    ConstantValue::Int(value) => Some(value),
                    _ => None,
                }),
            ExprKind::ScopedIdentifier {
                scope,
                name,
            } if scope == enum_name => variants.iter().find_map(|variant| {
                (variant.name == *name).then_some(match variant.value {
                    SemanticEnumValue::Int(value) => Some(value),
                    SemanticEnumValue::String(_) => None,
                })?
            }),
            ExprKind::Unary {
                op: UnaryOp::Negate,
                expr,
            } => self
                .evaluate_enum_int_discriminant(expr, enum_name, variants)
                .map(i32::wrapping_neg),
            ExprKind::Unary {
                op: UnaryOp::OnesComplement,
                expr,
            } => self
                .evaluate_enum_int_discriminant(expr, enum_name, variants)
                .map(|value| !value),
            ExprKind::Unary {
                op: UnaryOp::BooleanNot,
                expr,
            } => self
                .evaluate_enum_int_discriminant(expr, enum_name, variants)
                .map(|value| i32::from(value == 0)),
            ExprKind::Binary {
                op,
                left,
                right,
            } => evaluate_int_constant_binary(
                *op,
                self.evaluate_enum_int_discriminant(left, enum_name, variants)?,
                self.evaluate_enum_int_discriminant(right, enum_name, variants)?,
            ),
            ExprKind::Conditional {
                condition,
                when_true,
                when_false,
            } => {
                let condition =
                    self.evaluate_enum_int_discriminant(condition, enum_name, variants)?;
                self.evaluate_enum_int_discriminant(
                    if condition == 0 {
                        when_false
                    } else {
                        when_true
                    },
                    enum_name,
                    variants,
                )
            }
            _ => None,
        }
    }

    pub(super) fn evaluate_enum_string_discriminant(
        &self,
        expression: &Expr,
        enum_name: &str,
        variants: &[SemanticEnumVariant],
    ) -> Option<crate::ScriptString> {
        match &expression.kind {
            ExprKind::Literal(Literal::String(value)) => Some(value.clone()),
            ExprKind::Identifier(name) => variants
                .iter()
                .find_map(|variant| {
                    (variant.name == *name).then_some(match &variant.value {
                        SemanticEnumValue::String(value) => Some(value.clone()),
                        SemanticEnumValue::Int(_) => None,
                    })?
                })
                .or_else(|| match self.lookup_constant(name)? {
                    ConstantValue::String(value) => Some(value),
                    _ => None,
                }),
            ExprKind::ScopedIdentifier {
                scope,
                name,
            } if scope == enum_name => variants.iter().find_map(|variant| {
                (variant.name == *name).then(|| match &variant.value {
                    SemanticEnumValue::String(value) => Some(value.clone()),
                    SemanticEnumValue::Int(_) => None,
                })?
            }),
            ExprKind::Binary {
                op: BinaryOp::Add,
                left,
                right,
            } => {
                let left = self.evaluate_enum_string_discriminant(left, enum_name, variants)?;
                let right = self.evaluate_enum_string_discriminant(right, enum_name, variants)?;
                (left.len().saturating_add(right.len()) < 0x8000).then(|| left.concat(&right))
            }
            _ => None,
        }
    }

    pub(super) fn collect_const_globals_for_function_defaults(
        &mut self,
    ) -> Result<(), SemanticError> {
        for item in &self.script.items {
            let TopLevelItem::Global(declaration) = item else {
                continue;
            };
            if !declaration.ty.is_const {
                continue;
            }

            let ty = self.resolve_type(&declaration.ty)?;
            for declarator in &declaration.declarators {
                let value = if let Some(initializer) = &declarator.initializer {
                    self.evaluate_constant_expr(initializer).ok_or_else(|| {
                        SemanticError::new(
                            CompilerErrorCode::InvalidValueAssignedToConstant,
                            initializer.span,
                            format!(
                                "constant {:?} must be initialized with a constant value",
                                declarator.name
                            ),
                        )
                    })?
                } else {
                    default_constant_value(&ty, &self.enums).ok_or_else(|| {
                        SemanticError::new(
                            CompilerErrorCode::InvalidValueAssignedToConstant,
                            declarator.span,
                            format!(
                                "constant {:?} type {:?} does not support default constant \
                                 initialization",
                                declarator.name, ty
                            ),
                        )
                    })?
                };
                if !types_compatible(&ty, &value.ty()) {
                    return Err(SemanticError::new(
                        CompilerErrorCode::InvalidValueAssignedToConstant,
                        declarator
                            .initializer
                            .as_ref()
                            .map_or(declarator.span, |initializer| initializer.span),
                        format!(
                            "constant {:?} initializer does not match type {:?}",
                            declarator.name, ty
                        ),
                    ));
                }

                let was_precollected =
                    matches!(declaration.ty.kind, TypeKind::Int | TypeKind::String)
                        && self
                            .global_constants
                            .get(&declarator.name)
                            .is_some_and(|existing| existing == &value);
                if was_precollected {
                    self.global_constants.insert(declarator.name.clone(), value);
                } else if self
                    .global_constants
                    .insert(declarator.name.clone(), value)
                    .is_some()
                {
                    return Err(SemanticError::new(
                        CompilerErrorCode::VariableAlreadyUsedWithinScope,
                        declarator.span,
                        format!(
                            "constant {:?} conflicts with an enum compatibility alias",
                            declarator.name
                        ),
                    ));
                }
            }
        }

        Ok(())
    }

    pub(super) fn validate_top_level_static_assertions(&self) -> Result<(), SemanticError> {
        for item in &self.script.items {
            if let TopLevelItem::StaticAssert(assertion) = item {
                self.validate_static_assert(assertion)?;
            }
        }
        Ok(())
    }

    pub(super) fn validate_static_assert(
        &self,
        assertion: &crate::StaticAssertDecl,
    ) -> Result<(), SemanticError> {
        let value = self
            .evaluate_constant_expr(&assertion.condition)
            .ok_or_else(|| {
                SemanticError::new(
                    CompilerErrorCode::StaticAssertionFailed,
                    assertion.condition.span,
                    "static_assert condition must be a constant int expression",
                )
            })?;
        if matches!(value, ConstantValue::Int(value) if value != 0) {
            return Ok(());
        }
        let message = assertion.message.as_ref().map_or_else(
            || "static assertion failed".to_string(),
            |message| message.display_text(),
        );
        Err(SemanticError::new(
            CompilerErrorCode::StaticAssertionFailed,
            assertion.condition.span,
            message,
        ))
    }

    pub(super) fn collect_structs(&mut self) -> Result<(), SemanticError> {
        for item in &self.script.items {
            let TopLevelItem::Struct(definition) = item else {
                continue;
            };

            if self.structs.contains_key(&definition.name)
                || self.enums.contains_key(&definition.name)
                || self.aliases.contains_key(&definition.name)
            {
                return Err(SemanticError::new(
                    CompilerErrorCode::StructureRedefined,
                    definition.span,
                    format!("structure {:?} was defined more than once", definition.name),
                ));
            }

            let mut fields = Vec::new();
            let mut seen_names = BTreeMap::<String, crate::Span>::new();
            for declaration in &definition.fields {
                let field_type = self.resolve_type(&declaration.ty)?;
                for field in &declaration.names {
                    if seen_names.contains_key(&field.name) {
                        return Err(SemanticError::new(
                            CompilerErrorCode::VariableUsedTwiceInSameStructure,
                            field.span,
                            format!(
                                "field {:?} was used twice in structure {:?}",
                                field.name, definition.name
                            ),
                        ));
                    }
                    seen_names.insert(field.name.clone(), field.span);
                    fields.push(SemanticField {
                        name: field.name.clone(),
                        ty:   field_type.clone(),
                    });
                }
            }

            self.structs.insert(
                definition.name.clone(),
                SemanticStruct {
                    name: definition.name.clone(),
                    fields,
                },
            );
        }

        Ok(())
    }

    pub(super) fn collect_functions(&mut self) -> Result<(), SemanticError> {
        for item in &self.script.items {
            let TopLevelItem::Function(function) = item else {
                continue;
            };

            if self.enums.contains_key(&function.name) || self.aliases.contains_key(&function.name)
            {
                return Err(SemanticError::new(
                    CompilerErrorCode::InvalidEnumDeclaration,
                    function.span,
                    format!(
                        "function {:?} conflicts with a source type constructor",
                        function.name
                    ),
                ));
            }

            let signature = self.resolve_function_signature(function)?;
            if let Some(existing) = self.functions.get_mut(&function.name) {
                if existing.signature.is_builtin {
                    return Err(SemanticError::new(
                        CompilerErrorCode::FunctionImplementationAndDefinitionDiffer,
                        function.span,
                        format!(
                            "function {:?} conflicts with builtin declaration",
                            function.name
                        ),
                    ));
                }

                // The native compiler permits a prototype and implementation
                // to disagree only on return type. It continues to use the
                // prototype's return type at call sites.
                if !parameters_match(&existing.signature.parameters, &signature.parameters) {
                    return Err(SemanticError::new(
                        CompilerErrorCode::FunctionImplementationAndDefinitionDiffer,
                        function.span,
                        format!(
                            "function {:?} declaration differs from previous declaration",
                            function.name
                        ),
                    ));
                }

                for (existing_parameter, new_parameter) in existing
                    .signature
                    .parameters
                    .iter_mut()
                    .zip(&signature.parameters)
                {
                    if existing_parameter.default.is_none() && new_parameter.default.is_some() {
                        existing_parameter
                            .default
                            .clone_from(&new_parameter.default);
                        existing_parameter.is_optional = true;
                    }
                }

                if function.body.is_some() {
                    if existing.signature.has_body {
                        return Err(SemanticError::new(
                            CompilerErrorCode::DuplicateFunctionImplementation,
                            function.span,
                            format!(
                                "function {:?} was implemented more than once",
                                function.name
                            ),
                        ));
                    }
                    existing.signature.has_body = true;
                }
                continue;
            }

            self.functions.insert(
                function.name.clone(),
                FunctionInfo {
                    declaration_span: function.span,
                    signature,
                },
            );
        }

        Ok(())
    }

    pub(super) fn collect_globals(&mut self) -> Result<(), SemanticError> {
        for item in &self.script.items {
            let TopLevelItem::Global(declaration) = item else {
                continue;
            };

            let ty = self.resolve_type(&declaration.ty)?;
            for declarator in &declaration.declarators {
                if self.globals.contains_key(&declarator.name) {
                    return Err(SemanticError::new(
                        CompilerErrorCode::VariableAlreadyUsedWithinScope,
                        declarator.span,
                        format!("global {:?} was declared more than once", declarator.name),
                    ));
                }

                if declaration.ty.is_const {
                    let value = if let Some(initializer) = &declarator.initializer {
                        self.evaluate_constant_expr(initializer).ok_or_else(|| {
                            SemanticError::new(
                                CompilerErrorCode::InvalidValueAssignedToConstant,
                                initializer.span,
                                format!(
                                    "constant {:?} must be initialized with a constant value",
                                    declarator.name
                                ),
                            )
                        })?
                    } else {
                        default_constant_value(&ty, &self.enums).ok_or_else(|| {
                            SemanticError::new(
                                CompilerErrorCode::InvalidValueAssignedToConstant,
                                declarator.span,
                                format!(
                                    "constant {:?} type {:?} does not support default constant \
                                     initialization",
                                    declarator.name, ty
                                ),
                            )
                        })?
                    };
                    if !types_compatible(&ty, &value.ty()) {
                        return Err(SemanticError::new(
                            CompilerErrorCode::InvalidValueAssignedToConstant,
                            declarator
                                .initializer
                                .as_ref()
                                .map_or(declarator.span, |initializer| initializer.span),
                            format!(
                                "constant {:?} initializer does not match type {:?}",
                                declarator.name, ty
                            ),
                        ));
                    }

                    self.global_constants
                        .insert(declarator.name.clone(), value.clone());
                } else if let Some(initializer) = &declarator.initializer {
                    let initializer_type = self
                        .analyze_expr(initializer, &mut Vec::new())
                        .map(|resolved| resolved.ty)?;
                    if !types_compatible(&ty, &initializer_type) {
                        return Err(SemanticError::new(
                            type_mismatch_code(&ty, &initializer_type),
                            initializer.span,
                            format!(
                                "initializer for global {:?} has type {:?}, expected {:?}",
                                declarator.name, initializer_type, ty
                            ),
                        ));
                    }
                }

                self.globals.insert(
                    declarator.name.clone(),
                    SemanticGlobal {
                        name:     declarator.name.clone(),
                        ty:       ty.clone(),
                        is_const: declaration.ty.is_const,
                    },
                );
            }
        }

        Ok(())
    }
}
