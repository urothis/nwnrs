use crate::{NcsInstruction, NcsOpcode};

#[cfg(test)]
pub(crate) fn meld_instructions(instructions: Vec<NcsInstruction>) -> Vec<NcsInstruction> {
    let mut optimized: Vec<NcsInstruction> = Vec::with_capacity(instructions.len());

    for instruction in instructions {
        if let [.., runstack_add, constant, assignment] = optimized.as_slice()
            && let Some(replacement) =
                melded_instruction([runstack_add, constant, assignment, &instruction])
        {
            optimized.pop();
            optimized.pop();
            optimized.pop();
            optimized.push(replacement);
            continue;
        }

        optimized.push(instruction);
    }

    optimized
}

pub(crate) fn melded_instruction(instructions: [&NcsInstruction; 4]) -> Option<NcsInstruction> {
    let [runstack_add, constant, assignment, stack_pointer] = instructions;
    (stack_pointer.opcode == NcsOpcode::ModifyStackPointer
        && runstack_add.opcode == NcsOpcode::RunstackAdd
        && constant.opcode == NcsOpcode::Constant
        && assignment.opcode == NcsOpcode::Assignment
        && runstack_add.auxcode == constant.auxcode
        && assignment_stack_offset(assignment) == Some(-8))
    .then(|| constant.clone())
}

fn assignment_stack_offset(instruction: &NcsInstruction) -> Option<i32> {
    let bytes = instruction.extra.get(0..4)?;
    let mut offset = [0u8; 4];
    offset.copy_from_slice(bytes);
    Some(i32::from_be_bytes(offset))
}
