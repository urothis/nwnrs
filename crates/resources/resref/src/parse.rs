use crate::RESREF_MAX_LENGTH;

/// Returns `true` if `value` is a valid NWN resource name.
pub fn is_valid_resref_part1(value: &str) -> bool {
    !value.is_empty() && value.len() <= RESREF_MAX_LENGTH
}
