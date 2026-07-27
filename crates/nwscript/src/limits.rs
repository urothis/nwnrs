use serde::{Deserialize, Serialize};

/// Configurable defensive resource bounds for one compiler operation.
///
/// Defaults are intentionally large enough for unusually large production
/// modules. They exist to make memory, recursion, and latency behavior
/// predictable for corrupted or hostile input. Tooling may opt into larger
/// values explicitly when it controls the execution environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CompilerResourceLimits {
    /// Maximum bytes loaded from any one source file.
    pub max_source_file_bytes:   usize,
    /// Maximum bytes loaded across one root script and all includes.
    pub max_total_source_bytes:  usize,
    /// Maximum lexical tokens produced for one source bundle.
    pub max_tokens:              usize,
    /// Maximum nested syntactic scopes or recursive expression constructs.
    pub max_syntax_nesting:      usize,
    /// Maximum tokens retained after extended macro expansion.
    pub max_macro_output_tokens: usize,
}

impl Default for CompilerResourceLimits {
    fn default() -> Self {
        Self {
            max_source_file_bytes:   64 * 1024 * 1024,
            max_total_source_bytes:  1024 * 1024 * 1024,
            max_tokens:              16_000_000,
            max_syntax_nesting:      256,
            max_macro_output_tokens: 16_000_000,
        }
    }
}

impl CompilerResourceLimits {
    /// Validates that every limit can support at least one non-empty script.
    ///
    /// # Errors
    ///
    /// Returns a description of the invalid relationship.
    pub fn validate(self) -> Result<(), &'static str> {
        if self.max_source_file_bytes == 0 {
            return Err("max_source_file_bytes must be non-zero");
        }
        if self.max_total_source_bytes < self.max_source_file_bytes {
            return Err("max_total_source_bytes must be at least max_source_file_bytes");
        }
        if self.max_tokens == 0 {
            return Err("max_tokens must be non-zero");
        }
        if self.max_syntax_nesting == 0 {
            return Err("max_syntax_nesting must be non-zero");
        }
        if self.max_macro_output_tokens == 0 {
            return Err("max_macro_output_tokens must be non-zero");
        }
        Ok(())
    }
}
