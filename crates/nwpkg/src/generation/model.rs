use std::{ops::Deref, path::PathBuf};

use nwnrs_nwscript::CompilerDiagnostic;

/// One source range emitted by the canonical event project macro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventSourceRange {
    /// Physical source containing the range.
    pub path:         PathBuf,
    /// Inclusive source byte offset.
    pub start_offset: usize,
    /// Exclusive source byte offset.
    pub end_offset:   usize,
    /// One-based start line.
    pub start_line:   usize,
    /// One-based start column.
    pub start_column: usize,
    /// One-based end line.
    pub end_line:     usize,
    /// One-based end column.
    pub end_column:   usize,
}

/// Direction and source location of one event ordering relationship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRelation {
    /// Exact case-sensitive target handler.
    pub target: String,
    /// Source range of the target identifier.
    pub range:  EventSourceRange,
}

/// One validated event handler registration emitted by the project macro.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventRegistration {
    /// Canonical snake-case event subject.
    pub subject:           String,
    /// Dotted runtime payload name.
    pub runtime_name:      String,
    /// Engine phase.
    pub phase:             String,
    /// Subject identifier range.
    pub subject_range:     EventSourceRange,
    /// Phase identifier range.
    pub phase_range:       EventSourceRange,
    /// Exact handler identifier.
    pub handler:           String,
    /// Attribute range.
    pub attribute_range:   EventSourceRange,
    /// Handler identifier range.
    pub handler_range:     EventSourceRange,
    /// Function signature declaration range.
    pub declaration_range: EventSourceRange,
    /// Handlers that must run after this handler.
    pub before:            Vec<EventRelation>,
    /// Handlers that must run before this handler.
    pub after:             Vec<EventRelation>,
}

/// Canonically ordered registrations for one subject-and-phase lane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventLanePlan {
    /// Canonical snake-case event subject.
    pub subject:       String,
    /// Engine phase.
    pub phase:         String,
    /// Indexes into [`EventProjectPlan::registrations`] in resolved order.
    pub registrations: Vec<usize>,
}

/// Structured event analysis emitted by the NSS project macro.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EventProjectPlan {
    /// Registrations in canonical resolved execution order.
    pub registrations: Vec<EventRegistration>,
    /// Subject-and-phase lanes in first-registration order.
    pub lanes:         Vec<EventLanePlan>,
    /// Deduplicated subscription routes in first-registration order.
    pub subscriptions: Vec<(String, String)>,
    /// Deduplicated package-relative handler includes.
    pub includes:      Vec<String>,
    /// Whether the plan is valid for dispatcher generation.
    pub complete:      bool,
    /// Editor-recovery diagnostics. Strict plans have none.
    pub diagnostics:   Vec<CompilerDiagnostic>,
}

/// Editor completion category recognized from one active event attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventCompletionKind {
    /// `nwnrs::` construct name.
    Construct,
    /// Event subject.
    Subject,
    /// Event phase.
    Phase,
    /// Relationship modifier.
    Modifier,
    /// Same-lane handler target.
    Target,
}

/// Compiler-owned completion context for an event attribute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventCompletionContext {
    /// Completion category.
    pub kind:              EventCompletionKind,
    /// Authored identifier prefix.
    pub prefix:            String,
    /// Inclusive replacement byte offset.
    pub replacement_start: usize,
    /// Exclusive replacement byte offset.
    pub replacement_end:   usize,
    /// Canonical subject when already authored.
    pub subject:           Option<String>,
    /// Engine phase when already authored.
    pub phase:             Option<String>,
    /// Relationship direction for target completion.
    pub modifier:          Option<String>,
    /// Opening `#` byte offset of the active attribute.
    pub attribute_start:   usize,
}

/// One valid relationship completion candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventLaneCandidate {
    /// Exact handler identifier.
    pub handler:    String,
    /// Definition range.
    pub definition: EventSourceRange,
    /// Canonical resolved lane position.
    pub sort_order: usize,
}

/// Result of compiler-owned event completion analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventCompletionAnalysis {
    /// Active attribute completion context.
    pub context:    EventCompletionContext,
    /// Valid same-lane targets. Empty for non-target contexts.
    pub candidates: Vec<EventLaneCandidate>,
    /// Current macro-produced plan for target completion.
    pub plan:       Option<EventProjectPlan>,
}

/// Exact deterministic source generated for a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedNwScriptSource {
    bytes: Vec<u8>,
    text:  String,
}

impl GeneratedNwScriptSource {
    pub(super) fn new(bytes: Vec<u8>) -> Self {
        debug_assert!(bytes.is_ascii());
        let text = bytes.iter().copied().map(char::from).collect();
        Self {
            bytes,
            text,
        }
    }

    /// Returns the exact generated source bytes.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Copies the exact generated source bytes.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        self.bytes.clone()
    }
}

impl Deref for GeneratedNwScriptSource {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl PartialEq<str> for GeneratedNwScriptSource {
    fn eq(&self, other: &str) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl PartialEq<&str> for GeneratedNwScriptSource {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

/// The deterministic native module-load dispatcher generated for a module.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedEventDispatcher {
    /// Logical NWScript name, without an extension.
    pub name:         String,
    /// Complete generated NWScript source.
    pub source:       GeneratedNwScriptSource,
    /// Root used to resolve module-local includes in the generated script.
    pub include_root: PathBuf,
    /// Canonical macro-produced registration and ordering plan.
    pub plan:         EventProjectPlan,
}
