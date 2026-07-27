use super::*;

impl Vm {
    /// Creates an empty VM command table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers one command handler by its numeric action id.
    pub fn define_command<F>(&mut self, command: u16, handler: F)
    where
        F: Fn(&mut VmScript, u16, u8) -> Result<(), VmError> + 'static,
    {
        let index = usize::from(command);
        if self.commands.len() <= index {
            self.commands.resize_with(index + 1, || None);
        }
        if let Some(slot) = self.commands.get_mut(index) {
            *slot = Some(Box::new(handler));
        }
    }

    /// Registers one zero-metadata command handler.
    pub fn define_simple_command<F>(&mut self, command: u16, handler: F)
    where
        F: Fn(&mut VmScript) -> Result<(), VmError> + 'static,
    {
        self.define_command(command, move |script, _command, _argc| handler(script));
    }

    /// Registers one engine-structure default factory by its numeric type
    /// index.
    pub fn define_engine_structure<F>(&mut self, index: u8, factory: F)
    where
        F: Fn(u8) -> VmEngineStructureValue + 'static,
    {
        let index = usize::from(index);
        if self.engine_structures.len() <= index {
            self.engine_structures.resize_with(index + 1, || None);
        }
        if let Some(slot) = self.engine_structures.get_mut(index) {
            *slot = Some(Box::new(factory));
        }
    }

    /// Registers one fixed default engine-structure value by its numeric type
    /// index.
    pub fn define_engine_structure_default(&mut self, index: u8, value: VmEngineStructureValue) {
        self.define_engine_structure(index, move |_index| value.clone());
    }

    /// Registers one engine-structure equality hook by its numeric type index.
    pub fn define_engine_structure_comparer<F>(&mut self, index: u8, comparer: F)
    where
        F: Fn(u8, &VmEngineStructureValue, &VmEngineStructureValue) -> bool + 'static,
    {
        let index = usize::from(index);
        if self.engine_structure_comparers.len() <= index {
            self.engine_structure_comparers
                .resize_with(index + 1, || None);
        }
        if let Some(slot) = self.engine_structure_comparers.get_mut(index) {
            *slot = Some(Box::new(comparer));
        }
    }

    /// Registers one instruction trace hook invoked before each opcode
    /// executes.
    pub fn define_trace_hook<F>(&mut self, hook: F)
    where
        F: Fn(&VmScript, &VmTraceEvent) + 'static,
    {
        self.trace_hook = Some(Box::new(hook));
    }

    /// Removes the currently registered instruction trace hook.
    pub fn clear_trace_hook(&mut self) {
        self.trace_hook = None;
    }

    /// Sets the cooperative cancellation token checked before every opcode.
    pub fn set_cancellation_token(&mut self, cancellation: crate::CancellationToken) {
        self.cancellation = Some(cancellation);
    }

    /// Removes the cooperative cancellation token.
    pub fn clear_cancellation_token(&mut self) {
        self.cancellation = None;
    }

    /// Executes one script until it returns from the outermost frame.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails.
    pub fn run(&self, script: &mut VmScript) -> Result<(), VmError> {
        self.run_with_options(script, VmRunOptions::default())
    }

    /// Executes exactly one instruction.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if decoding or execution fails.
    pub fn step(&self, script: &mut VmScript) -> Result<VmStepOutcome, VmError> {
        self.step_with_stack_limit(script, HARD_VM_STACK_CELL_LIMIT)
    }

    fn step_with_stack_limit(
        &self,
        script: &mut VmScript,
        stack_limit: usize,
    ) -> Result<VmStepOutcome, VmError> {
        const HALT_IP: usize = usize::MAX;

        if self
            .cancellation
            .as_ref()
            .is_some_and(crate::CancellationToken::is_cancelled)
        {
            return Err(VmError::Cancelled);
        }
        script.aborted = false;

        if script.ret.is_empty() {
            script.ret.push(VmReturnFrame {
                target: HALT_IP
            });
        }

        if consume_abort_request(script) {
            return Ok(VmStepOutcome::Aborted);
        }

        let decoded = script
            .program
            .instruction_at(script.ip)
            .ok_or(VmError::InvalidInstructionPointer {
                offset: script.ip
            })?
            .clone();
        self.emit_trace(script, &decoded);
        let next_ip = decoded.offset + decoded.instruction.encoded_len();

        match decoded.instruction.opcode {
            NcsOpcode::NoOperation => {
                script.ip = next_ip;
            }
            NcsOpcode::Jmp => {
                script.ip = jump_target(decoded.offset, read_i32(&decoded, 0)?)?;
            }
            NcsOpcode::Jsr => {
                let target = jump_target(decoded.offset, read_i32(&decoded, 0)?)?;
                script.ret.push(VmReturnFrame {
                    target: next_ip
                });
                script.ip = target;
            }
            NcsOpcode::Jz => {
                if script.pop_int()? == 0 {
                    script.ip = jump_target(decoded.offset, read_i32(&decoded, 0)?)?;
                } else {
                    script.ip = next_ip;
                }
            }
            NcsOpcode::Jnz => {
                if script.pop_int()? != 0 {
                    script.ip = jump_target(decoded.offset, read_i32(&decoded, 0)?)?;
                } else {
                    script.ip = next_ip;
                }
            }
            NcsOpcode::Ret => {
                let frame = script.ret.pop().ok_or_else(|| VmError::StackUnderflow {
                    message: "attempted to return without a return frame".to_string(),
                })?;
                if frame.target == HALT_IP {
                    return Ok(VmStepOutcome::Halted);
                }
                script.ip = frame.target;
            }
            NcsOpcode::SaveBasePointer => {
                script.push_int(
                    i32::try_from(script.bp).map_err(|_error| {
                        invalid_extra(&decoded, "base pointer exceeds i32 range")
                    })?,
                );
                script.bp = script.sp.saturating_sub(1);
                script.ip = next_ip;
            }
            NcsOpcode::RestoreBasePointer => {
                script.bp = usize::try_from(script.pop_int()?)
                    .map_err(|_error| invalid_extra(&decoded, "negative base pointer restore"))?;
                script.ip = next_ip;
            }
            NcsOpcode::RunstackAdd => {
                push_default_value(script, &decoded, self)?;
                script.ip = next_ip;
            }
            NcsOpcode::RunstackCopy | NcsOpcode::RunstackCopyBase => {
                let base = if decoded.instruction.opcode == NcsOpcode::RunstackCopyBase {
                    script.bp
                } else {
                    script.sp
                };
                let src = relative_stack_cell(&decoded, base, read_i32(&decoded, 0)?)?;
                let cells = usize::from(read_u16(&decoded, 4)?) / 4;
                for index in 0..cells {
                    script.assign_cell(src + index, script.sp + index)?;
                }
                script.ip = next_ip;
            }
            NcsOpcode::Assignment | NcsOpcode::AssignmentBase => {
                let cells = usize::from(read_u16(&decoded, 4)?) / 4;
                let dst = if decoded.instruction.opcode == NcsOpcode::AssignmentBase {
                    relative_stack_cell(&decoded, script.bp, read_i32(&decoded, 0)?)?
                } else {
                    let encoded_offset = read_i32(&decoded, 0)?;
                    match relative_stack_cell(&decoded, script.sp, encoded_offset) {
                        Ok(dst) => dst,
                        Err(VmError::StackUnderflow {
                            ..
                        }) => relative_stack_cell(&decoded, script.sp + cells, encoded_offset)?,
                        Err(error) => return Err(error),
                    }
                };
                for index in 0..cells {
                    script.assign_cell(script.sp.saturating_sub(cells) + index, dst + index)?;
                }
                script.ip = next_ip;
            }
            NcsOpcode::Constant => {
                push_constant_value(script, &decoded)?;
                script.ip = next_ip;
            }
            NcsOpcode::ModifyStackPointer => {
                let byte_delta = read_i32(&decoded, 0)?;
                if byte_delta % 4 != 0 {
                    return Err(invalid_extra(
                        &decoded,
                        "MOVSP byte delta must be aligned to a 32-bit stack cell",
                    ));
                }
                if byte_delta > 0 {
                    let cells = usize::try_from(byte_delta / 4)
                        .map_err(|_error| invalid_extra(&decoded, "invalid MOVSP payload"))?;
                    let proposed = script.stack.len().checked_add(cells).ok_or(
                        VmError::StackLimitExceeded {
                            cells: usize::MAX,
                            limit: stack_limit,
                        },
                    )?;
                    if proposed > stack_limit {
                        return Err(VmError::StackLimitExceeded {
                            cells: proposed,
                            limit: stack_limit,
                        });
                    }
                    script.stack.try_reserve(cells).map_err(|_error| {
                        VmError::StackLimitExceeded {
                            cells: proposed,
                            limit: stack_limit,
                        }
                    })?;
                    for _ in 0..cells {
                        script.push_int(0);
                    }
                    script.ip = next_ip;
                    return Ok(VmStepOutcome::Running);
                }
                let cells = usize::try_from(byte_delta.unsigned_abs() / 4)
                    .map_err(|_error| invalid_extra(&decoded, "invalid MOVSP payload"))?;
                script.set_stack_pointer(script.sp.saturating_sub(cells))?;
                script.ip = next_ip;
            }
            NcsOpcode::Increment
            | NcsOpcode::Decrement
            | NcsOpcode::IncrementBase
            | NcsOpcode::DecrementBase => {
                let base = if matches!(
                    decoded.instruction.opcode,
                    NcsOpcode::IncrementBase | NcsOpcode::DecrementBase
                ) {
                    script.bp
                } else {
                    script.sp
                };
                let dst = relative_stack_cell(&decoded, base, read_i32(&decoded, 0)?)?;
                let delta = if matches!(
                    decoded.instruction.opcode,
                    NcsOpcode::Increment | NcsOpcode::IncrementBase
                ) {
                    1
                } else {
                    -1
                };
                let value = script
                    .stack
                    .get_mut(dst)
                    .ok_or_else(|| VmError::StackUnderflow {
                        message: format!("attempted to update missing stack cell {dst}"),
                    })?;
                match value {
                    VmValue::Int(int_value) => *int_value = int_value.wrapping_add(delta),
                    other => {
                        return Err(VmError::TypeMismatch {
                            offset:   decoded.offset,
                            message:  "increment/decrement requires integer target".to_string(),
                            expected: Some("int"),
                            actual:   other.kind_name(),
                        });
                    }
                }
                script.ip = next_ip;
            }
            NcsOpcode::Negation => {
                match decoded.instruction.auxcode {
                    NcsAuxCode::TypeInteger => {
                        let value = script.pop_int()?;
                        script.push_int(value.wrapping_neg());
                    }
                    NcsAuxCode::TypeFloat => {
                        let value = script.pop_float()?;
                        script.push_float(-value);
                    }
                    NcsAuxCode::TypeTypeVectorVector => {
                        let [x, y, z] = script.pop_vector()?;
                        script.push_vector([-x, -y, -z]);
                    }
                    _ => {
                        return unsupported(
                            &decoded,
                            "NEG only supports integer, float, and vector",
                        );
                    }
                }
                script.ip = next_ip;
            }
            NcsOpcode::Equal
            | NcsOpcode::NotEqual
            | NcsOpcode::Lt
            | NcsOpcode::Gt
            | NcsOpcode::Leq
            | NcsOpcode::Geq => {
                apply_comparison(script, &decoded, self)?;
                script.ip = next_ip;
            }
            NcsOpcode::LogicalOr => {
                let rhs = script.pop_int()? != 0;
                let lhs = script.pop_int()? != 0;
                script.push_int(bool_to_int(lhs || rhs));
                script.ip = next_ip;
            }
            NcsOpcode::LogicalAnd => {
                let rhs = script.pop_int()? != 0;
                let lhs = script.pop_int()? != 0;
                script.push_int(bool_to_int(lhs && rhs));
                script.ip = next_ip;
            }
            NcsOpcode::InclusiveOr => {
                let rhs = script.pop_int()?;
                let lhs = script.pop_int()?;
                script.push_int(lhs | rhs);
                script.ip = next_ip;
            }
            NcsOpcode::ExclusiveOr => {
                let rhs = script.pop_int()?;
                let lhs = script.pop_int()?;
                script.push_int(lhs ^ rhs);
                script.ip = next_ip;
            }
            NcsOpcode::BooleanAnd => {
                let rhs = script.pop_int()?;
                let lhs = script.pop_int()?;
                script.push_int(lhs & rhs);
                script.ip = next_ip;
            }
            NcsOpcode::BooleanNot => {
                let value = script.pop_int()? == 0;
                script.push_int(bool_to_int(value));
                script.ip = next_ip;
            }
            NcsOpcode::OnesComplement => {
                let value = script.pop_int()?;
                script.push_int(!value);
                script.ip = next_ip;
            }
            NcsOpcode::ShiftLeft => {
                let rhs = script.pop_int()?;
                let lhs = script.pop_int()?;
                script.push_int(lhs.wrapping_shl(rhs as u32));
                script.ip = next_ip;
            }
            NcsOpcode::ShiftRight => {
                let rhs = script.pop_int()?;
                let lhs = script.pop_int()?;
                script.push_int(lhs.wrapping_shr(rhs as u32));
                script.ip = next_ip;
            }
            NcsOpcode::UShiftRight => {
                let rhs = script.pop_int()?;
                let lhs = script.pop_int()?;
                script.push_int(((lhs as u32).wrapping_shr(rhs as u32)) as i32);
                script.ip = next_ip;
            }
            NcsOpcode::Add => {
                apply_add(script, &decoded)?;
                script.ip = next_ip;
            }
            NcsOpcode::Sub => {
                apply_sub(script, &decoded)?;
                script.ip = next_ip;
            }
            NcsOpcode::Mul => {
                apply_mul(script, &decoded)?;
                script.ip = next_ip;
            }
            NcsOpcode::Div => {
                apply_div(script, &decoded)?;
                script.ip = next_ip;
            }
            NcsOpcode::Modulus => {
                let rhs = script.pop_int()?;
                let lhs = script.pop_int()?;
                if rhs == 0 {
                    return Err(VmError::DivideByZero {
                        offset: decoded.offset,
                    });
                }
                script.push_int(lhs.wrapping_rem(rhs));
                script.ip = next_ip;
            }
            NcsOpcode::DeStruct => {
                let size_orig = usize::from(read_u16(&decoded, 0)?) / 4;
                let start = usize::from(read_u16(&decoded, 2)?) / 4;
                let size = usize::from(read_u16(&decoded, 4)?) / 4;

                if size + start < size_orig {
                    let new_sp = script.sp.saturating_sub(size_orig) + size + start;
                    script.set_stack_pointer(new_sp)?;
                }

                if start > 0 {
                    let from = script.sp.saturating_sub(size + start);
                    let to = script.sp.saturating_sub(start);
                    for index in from..to {
                        script.assign_cell(index + start, index)?;
                    }
                    script.set_stack_pointer(script.sp.saturating_sub(start))?;
                }
                script.ip = next_ip;
            }
            NcsOpcode::StoreIp | NcsOpcode::StoreState => {
                script.save_ip =
                    jump_target(decoded.offset, i32::from(decoded.instruction.auxcode as u8))?;
                if decoded.instruction.opcode == NcsOpcode::StoreState {
                    let situation = capture_saved_situation(script, &decoded, script.save_ip)?;
                    script.save_bp = situation.bp;
                    script.save_sp = situation.sp;
                    script.saved_situation = Some(situation);
                }
                script.ip = next_ip;
            }
            NcsOpcode::ExecuteCommand => {
                let command = read_u16(&decoded, 0)?;
                let argc = read_u8(&decoded, 2)?;
                let Some(handler) = self
                    .commands
                    .get(usize::from(command))
                    .and_then(Option::as_ref)
                else {
                    return Err(VmError::InvalidCommand {
                        offset: decoded.offset,
                        command,
                    });
                };
                handler(script, command, argc)?;
                if consume_abort_request(script) {
                    return Ok(VmStepOutcome::Aborted);
                }
                script.ip = next_ip;
            }
        }

        Ok(VmStepOutcome::Running)
    }

    /// Continues execution until the script is about to execute the instruction
    /// at `offset`.
    ///
    /// If the script reaches the requested offset, this returns
    /// [`VmStepOutcome::Running`] without executing that instruction.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails or exceeds the configured
    /// instruction budget.
    pub fn run_until_offset(
        &self,
        script: &mut VmScript,
        offset: usize,
        options: VmRunOptions,
    ) -> Result<VmStepOutcome, VmError> {
        let mut instructions_executed = 0usize;
        loop {
            if script.ip == offset {
                return Ok(VmStepOutcome::Running);
            }
            check_run_limits(script, options)?;
            if let Some(limit) = options.max_instructions
                && instructions_executed >= limit
            {
                return Err(VmError::InstructionLimitExceeded {
                    offset: script.ip,
                    limit,
                });
            }
            instructions_executed += 1;
            match self.step_with_stack_limit(
                script,
                options.max_stack_cells.unwrap_or(HARD_VM_STACK_CELL_LIMIT),
            )? {
                VmStepOutcome::Running => {}
                outcome => return Ok(outcome),
            }
        }
    }

    /// Executes the current instruction, stepping over user-function calls.
    ///
    /// For non-`JSR` instructions this behaves like [`Vm::step`]. For `JSR`, it
    /// runs until control returns to the caller, preserving a debugger-friendly
    /// "step over" behavior.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails or exceeds the configured
    /// instruction budget.
    pub fn step_over(
        &self,
        script: &mut VmScript,
        options: VmRunOptions,
    ) -> Result<VmStepOutcome, VmError> {
        let Some(instruction) = script.current_instruction().cloned() else {
            return Err(VmError::InvalidInstructionPointer {
                offset: script.ip
            });
        };
        if instruction.opcode != NcsOpcode::Jsr {
            return self.step_with_stack_limit(
                script,
                options.max_stack_cells.unwrap_or(HARD_VM_STACK_CELL_LIMIT),
            );
        }

        let return_offset = script.ip + instruction.encoded_len();
        let depth = script.ret.len();
        check_run_limits(script, options)?;
        match self.step_with_stack_limit(
            script,
            options.max_stack_cells.unwrap_or(HARD_VM_STACK_CELL_LIMIT),
        )? {
            VmStepOutcome::Running => {}
            outcome => return Ok(outcome),
        }

        let mut instructions_executed = 0usize;
        loop {
            if script.ip == return_offset && script.ret.len() == depth {
                return Ok(VmStepOutcome::Running);
            }
            check_run_limits(script, options)?;
            if let Some(limit) = options.max_instructions
                && instructions_executed >= limit
            {
                return Err(VmError::InstructionLimitExceeded {
                    offset: script.ip,
                    limit,
                });
            }
            instructions_executed += 1;
            match self.step_with_stack_limit(
                script,
                options.max_stack_cells.unwrap_or(HARD_VM_STACK_CELL_LIMIT),
            )? {
                VmStepOutcome::Running => {}
                outcome => return Ok(outcome),
            }
        }
    }

    /// Continues execution until the current function returns to its caller.
    ///
    /// If the script is currently at top level, this runs until the script
    /// halts or aborts.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails or exceeds the configured
    /// instruction budget.
    pub fn step_out(
        &self,
        script: &mut VmScript,
        options: VmRunOptions,
    ) -> Result<VmStepOutcome, VmError> {
        let depth = script.ret.len();
        let mut instructions_executed = 0usize;
        loop {
            check_run_limits(script, options)?;
            if let Some(limit) = options.max_instructions
                && instructions_executed >= limit
            {
                return Err(VmError::InstructionLimitExceeded {
                    offset: script.ip,
                    limit,
                });
            }
            instructions_executed += 1;
            match self.step_with_stack_limit(
                script,
                options.max_stack_cells.unwrap_or(HARD_VM_STACK_CELL_LIMIT),
            )? {
                VmStepOutcome::Running => {
                    if script.ret.len() < depth {
                        return Ok(VmStepOutcome::Running);
                    }
                }
                outcome => return Ok(outcome),
            }
        }
    }

    /// Continues execution until the script is about to execute one instruction
    /// mapped to `file_name:line_number` in attached `NDB` metadata.
    ///
    /// If the script reaches the requested line, this returns
    /// [`VmStepOutcome::Running`] without executing that instruction.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the script has no matching debug mapping,
    /// execution fails, or the instruction budget is exceeded.
    pub fn run_until_line(
        &self,
        script: &mut VmScript,
        file_name: &str,
        line_number: usize,
        options: VmRunOptions,
    ) -> Result<VmStepOutcome, VmError> {
        let target = script
            .program
            .source_lines
            .iter()
            .find(|line| {
                line.file_name.eq_ignore_ascii_case(file_name) && line.line_number == line_number
            })
            .map(|line| line.start)
            .ok_or_else(|| VmError::Setup {
                message: format!("no attached source line mapping for {file_name}:{line_number}"),
            })?;
        self.run_until_offset(script, target, options)
    }

    /// Continues execution until the script is about to execute one instruction
    /// inside the named function from attached `NDB` metadata.
    ///
    /// If the script reaches the requested function, this returns
    /// [`VmStepOutcome::Running`] without executing that instruction.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if the script has no matching debug mapping,
    /// execution fails, or the instruction budget is exceeded.
    pub fn run_until_function(
        &self,
        script: &mut VmScript,
        name: &str,
        options: VmRunOptions,
    ) -> Result<VmStepOutcome, VmError> {
        let target = script
            .program
            .functions
            .iter()
            .find(|function| function.label == name)
            .map(|function| function.start)
            .ok_or_else(|| VmError::Setup {
                message: format!("no attached function mapping for {name:?}"),
            })?;
        self.run_until_offset(script, target, options)
    }

    /// Executes one script with explicit runtime controls until it returns from
    /// the outermost frame.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails or exceeds the configured
    /// instruction budget.
    pub fn run_with_options(
        &self,
        script: &mut VmScript,
        options: VmRunOptions,
    ) -> Result<(), VmError> {
        let mut instructions_executed = 0usize;

        loop {
            check_run_limits(script, options)?;
            if let Some(limit) = options.max_instructions
                && instructions_executed >= limit
            {
                return Err(VmError::InstructionLimitExceeded {
                    offset: script.ip,
                    limit,
                });
            }
            instructions_executed += 1;
            match self.step_with_stack_limit(
                script,
                options.max_stack_cells.unwrap_or(HARD_VM_STACK_CELL_LIMIT),
            )? {
                VmStepOutcome::Running => {}
                VmStepOutcome::Halted | VmStepOutcome::Aborted => break,
            }
        }

        Ok(())
    }

    /// Executes one previously captured NWScript action situation.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails.
    pub fn run_situation(&self, situation: &VmSituation) -> Result<VmScript, VmError> {
        self.run_situation_with_options(situation, VmRunOptions::default())
    }

    /// Executes one previously captured NWScript action situation with explicit
    /// runtime controls.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if execution fails.
    pub fn run_situation_with_options(
        &self,
        situation: &VmSituation,
        options: VmRunOptions,
    ) -> Result<VmScript, VmError> {
        let mut script = situation.to_script();
        self.run_with_options(&mut script, options)?;
        Ok(script)
    }

    /// Decodes one script, runs it, and returns the finished runtime state.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if decoding or execution fails.
    pub fn run_bytes(&self, bytes: &[u8], label: impl Into<String>) -> Result<VmScript, VmError> {
        self.run_bytes_with_options(bytes, label, VmRunOptions::default())
    }

    /// Decodes one script, runs it with explicit runtime controls, and returns
    /// the finished runtime state.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if decoding or execution fails.
    pub fn run_bytes_with_options(
        &self,
        bytes: &[u8],
        label: impl Into<String>,
        options: VmRunOptions,
    ) -> Result<VmScript, VmError> {
        let mut script = VmScript::from_bytes(bytes, label)?;
        self.run_with_options(&mut script, options)?;
        Ok(script)
    }

    /// Decodes one script, attaches one `NDB` table, runs it, and returns the
    /// finished runtime state.
    ///
    /// Normal user-function calls do not require `NDB`; this variant adds
    /// function names and source locations for debugger operations.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if decoding, metadata attachment, or execution
    /// fails.
    pub fn run_bytes_with_ndb(
        &self,
        bytes: &[u8],
        label: impl Into<String>,
        ndb: &Ndb,
    ) -> Result<VmScript, VmError> {
        self.run_bytes_with_ndb_and_options(bytes, label, ndb, VmRunOptions::default())
    }

    /// Decodes one script, attaches one `NDB` table, and runs it with explicit
    /// runtime controls.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if decoding, metadata attachment, or execution
    /// fails.
    pub fn run_bytes_with_ndb_and_options(
        &self,
        bytes: &[u8],
        label: impl Into<String>,
        ndb: &Ndb,
        options: VmRunOptions,
    ) -> Result<VmScript, VmError> {
        let mut script = VmScript::from_bytes_with_ndb(bytes, label, ndb)?;
        self.run_with_options(&mut script, options)?;
        Ok(script)
    }

    /// Decodes one script, prepares a named direct function call, and runs it.
    ///
    /// This bypasses the compiler-emitted loader and initializes globals
    /// automatically for `main()`-style entry loaders when needed.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if decoding fails, the function cannot be invoked
    /// directly, or execution fails.
    pub fn run_function_bytes(
        &self,
        bytes: &[u8],
        label: impl Into<String>,
        ndb: &Ndb,
        name: &str,
        args: &[VmValue],
    ) -> Result<VmScript, VmError> {
        self.run_function_bytes_with_options(bytes, label, ndb, name, args, VmRunOptions::default())
    }

    /// Decodes one script, prepares a named direct function call, and runs it
    /// with explicit runtime controls.
    ///
    /// # Errors
    ///
    /// Returns [`VmError`] if decoding fails, the function cannot be invoked
    /// directly, execution fails, or the instruction budget is exceeded.
    pub fn run_function_bytes_with_options(
        &self,
        bytes: &[u8],
        label: impl Into<String>,
        ndb: &Ndb,
        name: &str,
        args: &[VmValue],
        options: VmRunOptions,
    ) -> Result<VmScript, VmError> {
        let mut script = VmScript::from_bytes_with_ndb(bytes, label, ndb)?;
        if script_has_globals(ndb) {
            self.bootstrap_globals_for_direct_call(&mut script, ndb, options)?;
        }
        script.prepare_function_call(ndb, name, args)?;
        self.run_with_options(&mut script, options)?;
        Ok(script)
    }

    pub(super) fn default_engine_structure_value(&self, index: u8) -> VmEngineStructureValue {
        self.engine_structures
            .get(usize::from(index))
            .and_then(Option::as_ref)
            .map_or_else(
                || default_engine_structure_value(index),
                |factory| factory(index),
            )
    }

    pub(super) fn compare_engine_structure_values(
        &self,
        index: u8,
        lhs: &VmEngineStructureValue,
        rhs: &VmEngineStructureValue,
    ) -> bool {
        self.engine_structure_comparers
            .get(usize::from(index))
            .and_then(Option::as_ref)
            .map_or_else(|| lhs == rhs, |comparer| comparer(index, lhs, rhs))
    }

    fn emit_trace(&self, script: &VmScript, decoded: &VmProgramInstruction) {
        let Some(hook) = self.trace_hook.as_ref() else {
            return;
        };
        hook(
            script,
            &VmTraceEvent {
                offset:      decoded.offset,
                ip:          script.ip,
                sp:          script.sp,
                bp:          script.bp,
                instruction: decoded.instruction.clone(),
            },
        );
    }

    fn bootstrap_globals_for_direct_call(
        &self,
        script: &mut VmScript,
        ndb: &Ndb,
        options: VmRunOptions,
    ) -> Result<(), VmError> {
        if !script_has_globals(ndb) {
            return Ok(());
        }

        let entry_name = entry_function_name(ndb).ok_or_else(|| VmError::Setup {
            message: "script entry function is missing".to_string(),
        })?;
        let entry = ndb
            .functions
            .iter()
            .find(|function| function.label == entry_name)
            .ok_or_else(|| VmError::Setup {
                message: format!("missing NDB entry for {entry_name:?}"),
            })?;
        let entry_start = ndb_code_offset(
            entry.binary_start,
            &format!("entry function {entry_name:?} start"),
        )?;

        let mut bootstrap =
            VmScript::from_instructions(Vec::new(), format!("{}#bootstrap", script.label));
        bootstrap.program = script.program.clone();
        let entry_index = *bootstrap.program.offsets_to_index.get(&entry_start).ok_or(
            VmError::InvalidInstructionPointer {
                offset: entry_start,
            },
        )?;
        let Some(instruction) = bootstrap.program.instructions.get_mut(entry_index) else {
            return Err(VmError::InvalidInstructionPointer {
                offset: entry_start,
            });
        };
        instruction.instruction = NcsInstruction {
            opcode:  NcsOpcode::Ret,
            auxcode: NcsAuxCode::None,
            extra:   Vec::new(),
        };
        let global_cleanup = bootstrap
            .program
            .instructions
            .windows(2)
            .position(|window| {
                let [restore, cleanup] = window else {
                    return false;
                };
                restore.offset < entry_start
                    && restore.instruction.opcode == NcsOpcode::RestoreBasePointer
                    && cleanup.instruction.opcode == NcsOpcode::ModifyStackPointer
            })
            .map(|index| index + 1)
            .ok_or_else(|| VmError::Setup {
                message: "global loader is missing its post-entry stack cleanup".to_string(),
            })?;
        let cleanup = bootstrap
            .program
            .instructions
            .get_mut(global_cleanup)
            .ok_or_else(|| VmError::Setup {
                message: "global loader cleanup index is out of bounds".to_string(),
            })?;
        cleanup.instruction = NcsInstruction {
            opcode:  NcsOpcode::Ret,
            auxcode: NcsAuxCode::None,
            extra:   Vec::new(),
        };
        self.run_with_options(&mut bootstrap, options)?;

        let globals_cells = global_stack_cells(ndb)?;
        let prefix_cells = loader_prefix_cells(ndb)?;
        let globals_end = prefix_cells + globals_cells;
        let globals = bootstrap
            .stack
            .get(prefix_cells..globals_end)
            .ok_or_else(|| VmError::StackUnderflow {
                message: format!(
                    "bootstrapped globals frame expected cells {}..{}, but stack only has {}",
                    prefix_cells,
                    globals_end,
                    bootstrap.stack.len()
                ),
            })?
            .to_vec();
        script.ip = 0;
        script.sp = globals.len();
        script.bp = 0;
        script.ret.clear();
        script.stack = globals;
        if globals_cells > 0 {
            script.push_int(0);
            script.bp = globals_cells;
        }
        script.save_ip = 0;
        script.save_sp = 0;
        script.save_bp = 0;
        script.saved_situation = None;
        script.abort_requested = false;
        script.aborted = false;
        Ok(())
    }
}
