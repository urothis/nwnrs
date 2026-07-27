use std::path::PathBuf;

use nwnrs_nwpkg::{EventCompletionKind, EventProjectPlan, EventRegistration, EventSourceRange};
use nwnrs_nwscript::{CancellationToken, physical_sources_equal};

use crate::{
    LanguageOperationError, LanguageService, NwScriptSymbolDefinition, PackageConfiguration,
};

/// Presentation-neutral compiler completion category for one event attribute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventLanguageCompletionKind {
    /// The `event` construct after `nwnrs::`.
    Construct,
    /// A catalog event subject.
    Subject,
    /// A supported phase for the selected subject.
    Phase,
    /// A relationship clause.
    Modifier,
    /// A valid same-lane handler.
    Target,
}

/// One structured event completion supplied by the compiler-owned index.
#[derive(Clone, Debug)]
pub struct EventLanguageCompletionItem {
    /// Inserted identifier.
    pub label:         String,
    /// Semantic item category.
    pub kind:          EventLanguageCompletionItemKind,
    /// Concise semantic detail.
    pub detail:        String,
    /// Source documentation, when available.
    pub documentation: Option<String>,
    /// Stable lane/catalog ordering.
    pub sort_order:    usize,
    /// Exact target definition for relationship candidates.
    pub definition:    Option<NwScriptSymbolDefinition>,
}

/// Semantic category of an event completion item.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventLanguageCompletionItemKind {
    /// Attribute keyword.
    Keyword,
    /// Event subject.
    Event,
    /// Event phase.
    Phase,
    /// Handler function.
    Function,
}

/// Compiler-owned completions for one active event attribute.
#[derive(Clone, Debug)]
pub struct EventLanguageCompletions {
    /// Active grammar position.
    pub kind:              EventLanguageCompletionKind,
    /// Authored prefix.
    pub prefix:            String,
    /// Inclusive replacement byte offset.
    pub replacement_start: usize,
    /// Exclusive replacement byte offset.
    pub replacement_end:   usize,
    /// Selected subject, when present.
    pub subject:           Option<String>,
    /// Selected phase, when present.
    pub phase:             Option<String>,
    /// Selected relationship modifier, when present.
    pub modifier:          Option<String>,
    /// Valid compiler-owned choices.
    pub items:             Vec<EventLanguageCompletionItem>,
}

/// Resolved event token category.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventLanguageSymbolKind {
    /// Catalog subject or phase.
    Route,
    /// Relationship handler target.
    Handler,
}

/// Exact token within a resolved event attribute.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventLanguageSymbolToken {
    /// Subject identifier.
    Subject,
    /// Phase identifier.
    Phase,
    /// Relationship target identifier.
    Handler,
}

/// Compiler-owned event symbol resolution result.
#[derive(Clone, Debug)]
pub struct EventLanguageSymbol {
    /// Resolved semantic category.
    pub kind:       EventLanguageSymbolKind,
    /// Canonical event subject.
    pub subject:    String,
    /// Canonical event phase.
    pub phase:      String,
    /// Exact token under the cursor.
    pub token:      EventLanguageSymbolToken,
    /// Exact compiler source range.
    pub range:      EventSourceRange,
    /// Handler definition for relationship targets.
    pub definition: Option<NwScriptSymbolDefinition>,
}

impl LanguageService {
    /// Returns compiler-owned event completions for one synchronized document.
    pub fn event_completions_at(
        &self,
        configuration: &PackageConfiguration,
        cursor_offset: usize,
        cancellation: &CancellationToken,
    ) -> Result<Option<EventLanguageCompletions>, LanguageOperationError> {
        let Some(analysis) = self.event_recovery(configuration, cursor_offset, cancellation)?
        else {
            return Ok(None);
        };
        let mut items = Vec::new();
        match analysis.context.kind {
            EventCompletionKind::Construct => {
                items.push(EventLanguageCompletionItem {
                    label:         "event".to_string(),
                    kind:          EventLanguageCompletionItemKind::Keyword,
                    detail:        "Canonical nwnrs event registration".to_string(),
                    documentation: None,
                    sort_order:    0,
                    definition:    None,
                });
            }
            EventCompletionKind::Subject => {
                let mut subjects = Vec::new();
                for event in nwnrs_runtime::EVENT_CATALOG {
                    if !subjects.contains(&event.subject) {
                        subjects.push(event.subject);
                    }
                }
                items.extend(
                    subjects
                        .into_iter()
                        .enumerate()
                        .map(|(sort_order, subject)| EventLanguageCompletionItem {
                            label: subject.to_string(),
                            kind: EventLanguageCompletionItemKind::Event,
                            detail: "nwnrs event subject".to_string(),
                            documentation: None,
                            sort_order,
                            definition: None,
                        }),
                );
            }
            EventCompletionKind::Phase => {
                items.extend(
                    nwnrs_runtime::EVENT_CATALOG
                        .iter()
                        .filter(|event| analysis.context.subject.as_deref() == Some(event.subject))
                        .enumerate()
                        .map(|(sort_order, event)| EventLanguageCompletionItem {
                            label: event.phase.to_string(),
                            kind: EventLanguageCompletionItemKind::Phase,
                            detail: event.name.to_string(),
                            documentation: None,
                            sort_order,
                            definition: None,
                        }),
                );
            }
            EventCompletionKind::Modifier => {
                items.extend(["before", "after"].into_iter().enumerate().map(
                    |(sort_order, modifier)| EventLanguageCompletionItem {
                        label: modifier.to_string(),
                        kind: EventLanguageCompletionItemKind::Keyword,
                        detail: format!("Order this handler {modifier} same-lane handlers"),
                        documentation: None,
                        sort_order,
                        definition: None,
                    },
                ));
            }
            EventCompletionKind::Target => {
                for candidate in analysis.candidates {
                    let definition = self.exact_event_handler_definition(
                        configuration,
                        &candidate.handler,
                        &candidate.definition,
                        cancellation,
                    )?;
                    let documentation = definition
                        .as_ref()
                        .and_then(|definition| definition.documentation.clone());
                    items.push(EventLanguageCompletionItem {
                        label: candidate.handler,
                        kind: EventLanguageCompletionItemKind::Function,
                        detail: format!(
                            "{}::{} handler",
                            analysis.context.subject.as_deref().unwrap_or_default(),
                            analysis.context.phase.as_deref().unwrap_or_default()
                        ),
                        documentation,
                        sort_order: candidate.sort_order,
                        definition,
                    });
                }
            }
        }
        Ok(Some(EventLanguageCompletions {
            kind: event_completion_kind(analysis.context.kind),
            prefix: analysis.context.prefix,
            replacement_start: analysis.context.replacement_start,
            replacement_end: analysis.context.replacement_end,
            subject: analysis.context.subject,
            phase: analysis.context.phase,
            modifier: analysis.context.modifier,
            items,
        }))
    }

    /// Resolves a catalog route or exact same-lane relationship target.
    pub fn resolve_event_symbol_at(
        &self,
        configuration: &PackageConfiguration,
        cursor_offset: usize,
        cancellation: &CancellationToken,
    ) -> Result<Option<EventLanguageSymbol>, LanguageOperationError> {
        let plan = match self.strict_event_plan(configuration, cancellation) {
            Ok(Some(plan)) => plan,
            Ok(None) => return Ok(None),
            Err(strict_error) => self
                .event_recovery(configuration, cursor_offset, cancellation)?
                .and_then(|analysis| analysis.plan.map(std::sync::Arc::new))
                .ok_or(strict_error)?,
        };
        resolve_event_symbol(self, configuration, &plan, cursor_offset, cancellation)
    }

    fn exact_event_handler_definition(
        &self,
        configuration: &PackageConfiguration,
        handler: &str,
        range: &EventSourceRange,
        cancellation: &CancellationToken,
    ) -> Result<Option<NwScriptSymbolDefinition>, LanguageOperationError> {
        let mut query = configuration.clone();
        query.source_path = range.path.clone();
        Ok(self
            .definitions(&query, handler.to_string(), None, Some(cancellation))?
            .into_iter()
            .find(|definition| {
                definition.name == handler
                    && physical_sources_equal(&definition.path, &range.path)
                    && definition.start_line == range.start_line
            }))
    }
}

fn resolve_event_symbol(
    service: &LanguageService,
    configuration: &PackageConfiguration,
    plan: &EventProjectPlan,
    cursor_offset: usize,
    cancellation: &CancellationToken,
) -> Result<Option<EventLanguageSymbol>, LanguageOperationError> {
    for registration in &plan.registrations {
        if !physical_sources_equal(&registration.handler_range.path, &configuration.source_path) {
            continue;
        }
        if range_contains(&registration.subject_range, cursor_offset) {
            return Ok(Some(route_symbol(
                registration,
                EventLanguageSymbolToken::Subject,
                registration.subject_range.clone(),
            )));
        }
        if range_contains(&registration.phase_range, cursor_offset) {
            return Ok(Some(route_symbol(
                registration,
                EventLanguageSymbolToken::Phase,
                registration.phase_range.clone(),
            )));
        }
        for relation in registration.before.iter().chain(&registration.after) {
            if !range_contains(&relation.range, cursor_offset) {
                continue;
            }
            let definition = plan
                .registrations
                .iter()
                .find(|candidate| {
                    candidate.subject == registration.subject
                        && candidate.phase == registration.phase
                        && candidate.handler == relation.target
                })
                .map(|target| {
                    service.exact_event_handler_definition(
                        configuration,
                        &target.handler,
                        &target.handler_range,
                        cancellation,
                    )
                })
                .transpose()?
                .flatten();
            return Ok(Some(EventLanguageSymbol {
                kind: EventLanguageSymbolKind::Handler,
                subject: registration.subject.clone(),
                phase: registration.phase.clone(),
                token: EventLanguageSymbolToken::Handler,
                range: relation.range.clone(),
                definition,
            }));
        }
    }
    Ok(None)
}

fn route_symbol(
    registration: &EventRegistration,
    token: EventLanguageSymbolToken,
    range: EventSourceRange,
) -> EventLanguageSymbol {
    EventLanguageSymbol {
        kind: EventLanguageSymbolKind::Route,
        subject: registration.subject.clone(),
        phase: registration.phase.clone(),
        token,
        range,
        definition: None,
    }
}

const fn event_completion_kind(kind: EventCompletionKind) -> EventLanguageCompletionKind {
    match kind {
        EventCompletionKind::Construct => EventLanguageCompletionKind::Construct,
        EventCompletionKind::Subject => EventLanguageCompletionKind::Subject,
        EventCompletionKind::Phase => EventLanguageCompletionKind::Phase,
        EventCompletionKind::Modifier => EventLanguageCompletionKind::Modifier,
        EventCompletionKind::Target => EventLanguageCompletionKind::Target,
    }
}

fn range_contains(range: &EventSourceRange, offset: usize) -> bool {
    offset >= range.start_offset && offset < range.end_offset
}

/// Returns a package configuration targeting a specific event handler source.
#[must_use]
pub fn event_handler_configuration(
    base: &PackageConfiguration,
    path: PathBuf,
) -> PackageConfiguration {
    let mut result = base.clone();
    result.source_path = path;
    result
}
