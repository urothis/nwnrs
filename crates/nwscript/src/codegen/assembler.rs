use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct LabelId(pub(super) u32);

pub(super) struct ResolvedAssembly {
    pub(super) instructions: Vec<NcsInstruction>,
    pub(super) offsets:      BTreeMap<LabelId, u32>,
}

pub(super) struct FunctionDebugInfo {
    pub(super) start: LabelId,
    pub(super) end:   LabelId,
}

pub(super) struct VariableDebugInfo {
    pub(super) name:      String,
    pub(super) ty:        SemanticType,
    pub(super) start:     LabelId,
    pub(super) end:       Option<LabelId>,
    pub(super) stack_loc: u32,
}

pub(super) struct LineDebugInfo {
    pub(super) source_id: SourceId,
    pub(super) line_num:  usize,
    pub(super) start:     LabelId,
    pub(super) end:       LabelId,
}

pub(super) struct OpenLineDebug {
    pub(super) source_id: SourceId,
    pub(super) line_num:  usize,
    pub(super) refs:      usize,
    pub(super) start:     LabelId,
}

#[derive(Default)]
pub(super) struct LineDebugTracker {
    pub(super) current: Option<OpenLineDebug>,
    pub(super) entries: Vec<LineDebugInfo>,
}

pub(super) struct CodegenOutput {
    pub(super) instructions:  Vec<NcsInstruction>,
    pub(super) label_offsets: BTreeMap<LabelId, u32>,
    pub(super) functions:     BTreeMap<String, FunctionDebugInfo>,
    pub(super) variables:     Vec<VariableDebugInfo>,
    pub(super) lines:         Vec<LineDebugInfo>,
}

pub(super) enum AssemblyItem {
    Label(LabelId),
    Instruction(NcsInstruction),
    RelativeJump { opcode: NcsOpcode, target: LabelId },
}

#[derive(Default)]
pub(super) struct Assembler {
    pub(super) items:      Vec<AssemblyItem>,
    pub(super) next_label: u32,
}

impl Assembler {
    pub(super) fn new_label(&mut self) -> LabelId {
        let label = LabelId(self.next_label);
        self.next_label += 1;
        label
    }

    pub(super) fn place_label(&mut self, label: LabelId) {
        self.items.push(AssemblyItem::Label(label));
    }

    pub(super) fn push(&mut self, instruction: NcsInstruction) {
        self.items.push(AssemblyItem::Instruction(instruction));
    }

    pub(super) fn push_jump(&mut self, opcode: NcsOpcode, target: LabelId) {
        self.items.push(AssemblyItem::RelativeJump {
            opcode,
            target,
        });
    }

    pub(super) fn meld_instructions(&mut self) {
        let mut pending = VecDeque::from(std::mem::take(&mut self.items));
        let mut optimized = Vec::with_capacity(self.items.len());
        while !pending.is_empty() {
            let candidate = {
                let mut instructions = Vec::with_capacity(4);
                let mut positions = Vec::with_capacity(4);
                for (position, item) in pending.iter().enumerate() {
                    match item {
                        AssemblyItem::Label(_) => {}
                        AssemblyItem::Instruction(instruction) => {
                            instructions.push(instruction);
                            positions.push(position);
                            if instructions.len() == 4 {
                                break;
                            }
                        }
                        AssemblyItem::RelativeJump {
                            ..
                        } => break,
                    }
                }
                match (instructions.as_slice(), positions.as_slice()) {
                    ([first, second, third, fourth], [_, second_pos, _, fourth_pos]) => {
                        melded_instruction([first, second, third, fourth])
                            .map(|replacement| (replacement, *second_pos, *fourth_pos))
                    }
                    _ => None,
                }
            };

            if let Some((replacement, constant_position, end_position)) = candidate {
                let mut leading_labels = Vec::new();
                let mut trailing_labels = Vec::new();
                for position in 0..=end_position {
                    if let Some(AssemblyItem::Label(label)) = pending.pop_front() {
                        if position < constant_position {
                            leading_labels.push(AssemblyItem::Label(label));
                        } else {
                            trailing_labels.push(AssemblyItem::Label(label));
                        }
                    }
                }
                optimized.extend(leading_labels);
                optimized.push(AssemblyItem::Instruction(replacement));
                optimized.extend(trailing_labels);
            } else if let Some(item) = pending.pop_front() {
                optimized.push(item);
            }
        }
        self.items = optimized;
    }

    pub(super) fn finalize(self) -> Result<ResolvedAssembly, CodegenError> {
        let mut offsets = BTreeMap::new();
        let mut offset = 0usize;
        for item in &self.items {
            match item {
                AssemblyItem::Label(label) => {
                    offsets.insert(*label, offset);
                }
                AssemblyItem::Instruction(instruction) => {
                    offset += instruction.encoded_len();
                }
                AssemblyItem::RelativeJump {
                    ..
                } => {
                    offset += NCS_OPERATION_BASE_SIZE + 4;
                }
            }
        }

        let mut instructions = Vec::new();
        let mut offset = 0usize;
        for item in self.items {
            match item {
                AssemblyItem::Label(_) => {}
                AssemblyItem::Instruction(instruction) => {
                    offset += instruction.encoded_len();
                    instructions.push(instruction);
                }
                AssemblyItem::RelativeJump {
                    opcode,
                    target,
                } => {
                    let target_offset = offsets.get(&target).copied().ok_or_else(|| {
                        CodegenError::new(None, format!("unresolved code label {target:?}"))
                    })?;
                    let delta = usize_to_i32(target_offset, "jump target offset")?
                        - usize_to_i32(offset, "jump offset")?;
                    let instruction = NcsInstruction {
                        opcode,
                        auxcode: NcsAuxCode::None,
                        extra: delta.to_be_bytes().to_vec(),
                    };
                    offset += instruction.encoded_len();
                    instructions.push(instruction);
                }
            }
        }

        Ok(ResolvedAssembly {
            instructions,
            offsets: offsets
                .into_iter()
                .map(|(label, offset)| {
                    Ok::<_, CodegenError>((label, usize_to_u32(offset, "label offset")?))
                })
                .collect::<Result<_, _>>()?,
        })
    }
}
