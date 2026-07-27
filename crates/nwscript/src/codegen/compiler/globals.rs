use super::*;

pub(super) struct GlobalEmitter<'a, 'b> {
    pub(super) compiler:        &'b mut O0Compiler<'a>,
    pub(super) allocated_bytes: usize,
    pub(super) temp_bytes:      usize,
}

impl GlobalEmitter<'_, '_> {
    pub(super) fn emit_expr(&mut self, expr: &HirExpr) -> Result<(), CodegenError> {
        self.compiler.check_cancelled(Some(expr.span))?;
        self.compiler.global_init_bytes = Some(self.allocated_bytes);
        let result = emit_expr_common(self.compiler, &mut self.temp_bytes, None, expr);
        self.compiler.global_init_bytes = None;
        result
    }

    pub(super) fn emit_store_global(
        &mut self,
        name: &str,
        span: crate::Span,
    ) -> Result<(), CodegenError> {
        let layout = self
            .compiler
            .global_layout
            .get(name)
            .ok_or_else(|| CodegenError::new(Some(span), format!("unknown global {name:?}")))?;
        let offset = usize_to_i32(layout.offset, "global offset")?
            - usize_to_i32(
                self.allocated_bytes + self.temp_bytes,
                "global initialization stack size",
            )?;
        self.compiler.assembler.push(NcsInstruction {
            opcode:  NcsOpcode::Assignment,
            auxcode: NcsAuxCode::TypeVoid,
            extra:   assignment_extra(offset, layout.size)?,
        });
        Ok(())
    }

    pub(super) fn emit_pop_type(&mut self, ty: &SemanticType) -> Result<(), CodegenError> {
        let size = size_of_type(ty, &self.compiler.structs)?;
        if size > 0 {
            self.temp_bytes = self.temp_bytes.saturating_sub(size);
            self.compiler.assembler.push(NcsInstruction {
                opcode:  NcsOpcode::ModifyStackPointer,
                auxcode: NcsAuxCode::None,
                extra:   (-usize_to_i32(size, "stack pop size")?)
                    .to_be_bytes()
                    .to_vec(),
            });
        }
        Ok(())
    }
}
