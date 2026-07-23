use std::{collections::BTreeMap, io::Cursor};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use nwnrs_types::{
    gff::{GffCExoLocString, GffField, GffStruct, GffValue},
    localization::BAD_STRREF,
    resman::{CachePolicy, ResMan, ResolvedResRef},
    twoda::{TwoDa, read_twoda},
};
use serde::{Deserialize, Serialize};

use crate::scene::{SceneAreaObject, SceneDocument, SceneError, SceneInstanceKind, SceneResult};

/// Cached immutable lookup tables shared by every object inspection in a scene
/// session.
#[derive(Debug, Default)]
pub struct AreaInspectionCache {
    tables: BTreeMap<String, Option<TwoDa>>,
}

/// Resolves localized strings using the host application's configured TLK
/// chain.
pub trait InspectionLocalizationResolver {
    /// Resolves an authored localized value while preserving its source
    /// metadata.
    fn resolve(&mut self, value: &GffCExoLocString) -> InspectionLocalizedString;
}

/// A complete, lossless, type-aware inspection of one authored area object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AreaObjectInspection {
    /// Inspection payload schema identity.
    pub schema:      String,
    /// Stable scene object key.
    pub key:         String,
    /// User-facing object label.
    pub label:       String,
    /// Authored object category.
    pub kind:        SceneInstanceKind,
    /// Curated sections containing every effective top-level field.
    pub sections:    Vec<InspectionSection>,
    /// Exact source layers used to construct the effective view.
    pub sources:     Vec<InspectionSource>,
    /// Referenced resources discovered from semantically typed fields.
    pub references:  Vec<InspectionResource>,
    /// Non-fatal resolution problems.
    pub diagnostics: Vec<String>,
}

/// One type-aware group of effective fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionSection {
    /// Stable section identity.
    pub id:           String,
    /// User-facing section label.
    pub label:        String,
    /// Whether the inspector should initially expand this section.
    pub default_open: bool,
    /// Effective fields assigned to this section.
    pub fields:       Vec<InspectionField>,
}

/// One exact GFF field with resolved presentation metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionField {
    /// Original GFF label.
    pub name:       String,
    /// User-facing field label.
    pub label:      String,
    /// Stored GFF value kind.
    pub kind:       String,
    /// Compact exact display value.
    pub display:    String,
    /// Primitive string value, including exact 64-bit integer text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text:       Option<String>,
    /// Nested structure id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub struct_id:  Option<i32>,
    /// Ordered child fields for a structure.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields:     Vec<InspectionField>,
    /// Ordered child structures for a list.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries:    Vec<InspectionStructure>,
    /// Exact base64 payload for a void field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value64:    Option<String>,
    /// Localized-string metadata and resolved text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub localized:  Option<InspectionLocalizedString>,
    /// Resolved resource reference, when the field has resource semantics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource:   Option<InspectionResource>,
    /// Resolved 2DA row, when the numeric field has table semantics.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lookup:     Option<InspectionTwoDaLookup>,
    /// Source layer that supplied this effective field.
    pub provenance: InspectionProvenance,
}

/// One nested GFF structure.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionStructure {
    /// Authored GFF structure id.
    pub id:     i32,
    /// Ordered fields.
    pub fields: Vec<InspectionField>,
}

/// A raw GFF source layer retained without flattening.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionSource {
    /// Source role, such as `instance` or `blueprint`.
    pub layer:    String,
    /// Exact resource name.
    pub resource: String,
    /// Winning resource-container origin, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin:   Option<String>,
    /// Complete raw source structure.
    pub data:     InspectionStructure,
}

/// Provenance for one effective value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionProvenance {
    /// Source role.
    pub layer:    String,
    /// Exact source resource.
    pub resource: String,
    /// Winning container origin, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin:   Option<String>,
}

/// A resource reference resolved through the active layered resource manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionResource {
    /// Exact `resref.ext` name.
    pub resource: String,
    /// Winning container origin, when resolved.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin:   Option<String>,
    /// Whether the resource exists in the active resource view.
    pub resolved: bool,
}

/// A localized string together with its raw and resolved identities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionLocalizedString {
    /// Text selected for the configured language and gender.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text:        Option<String>,
    /// Original TLK string reference.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub str_ref:     Option<u32>,
    /// Source used for the selected text.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source:      Option<String>,
    /// Selected NWN language id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language_id: Option<u32>,
    /// Selected gender name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gender:      Option<String>,
    /// Every authored inline language/gender entry.
    pub entries:     Vec<InspectionLocalizedEntry>,
}

/// One exact inline CExoLocString entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionLocalizedEntry {
    /// Combined NWN language/gender id.
    pub id:   i32,
    /// Authored text.
    pub text: String,
}

/// A resolved numeric reference into a 2DA table.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectionTwoDaLookup {
    /// Table resource.
    pub resource: String,
    /// Numeric row index.
    pub row:      usize,
    /// Best available label or localized name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label:    Option<String>,
    /// Column from which the label was selected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column:   Option<String>,
    /// Winning container origin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin:   Option<String>,
}

/// Stateful inspector over the same resource view used to assemble a scene.
pub struct AreaInspector<'a> {
    resman:       &'a mut ResMan,
    cache:        &'a mut AreaInspectionCache,
    localization: &'a mut dyn InspectionLocalizationResolver,
}

impl<'a> AreaInspector<'a> {
    /// Creates an inspector sharing caller-owned resource and lookup caches.
    pub fn new(
        resman: &'a mut ResMan,
        cache: &'a mut AreaInspectionCache,
        localization: &'a mut dyn InspectionLocalizationResolver,
    ) -> Self {
        Self {
            resman,
            cache,
            localization,
        }
    }

    /// Inspects one object from an already assembled area scene.
    ///
    /// # Errors
    ///
    /// Returns [`SceneError`] when the scene has no matching authored GIT
    /// object or a referenced blueprint is malformed.
    pub fn inspect(
        &mut self,
        scene: &SceneDocument,
        object_key: &str,
    ) -> SceneResult<AreaObjectInspection> {
        let objects = scene.area.as_ref().map_or_else(Vec::new, |area| {
            crate::scene::area_object_catalog(&area.instances)
        });
        let object = objects
            .iter()
            .find(|value| value.key == object_key)
            .ok_or_else(|| SceneError::invalid(format!("unknown area object {object_key}")))?;
        let area = scene
            .area
            .as_ref()
            .ok_or_else(|| SceneError::invalid("scene has no authored GIT data"))?;
        let instance = object_structure(&area.instances, object).ok_or_else(|| {
            SceneError::invalid(format!(
                "area object {} no longer matches its GIT list",
                object.key
            ))
        })?;
        let area_name = scene
            .module
            .as_ref()
            .map_or(scene.name.as_str(), |module| module.entry_area.as_str());
        let instance_resource = format!("{area_name}.git");
        let instance_origin = self.resolve_origin(&instance_resource);
        let template = template_resref(instance).or(object.template_resref.as_deref());
        let blueprint_resource =
            template.map(|template| format!("{template}.{}", blueprint_extension(object.kind)));
        let mut diagnostics = Vec::new();
        let blueprint = match blueprint_resource
            .as_deref()
            .map(|resource| self.load_gff(resource))
        {
            Some(Ok(root)) => Some(root),
            Some(Err(error)) => {
                diagnostics.push(format!("could not resolve blueprint: {error}"));
                None
            }
            None => None,
        };
        let blueprint_origin = blueprint_resource
            .as_deref()
            .and_then(|resource| self.resolve_origin(resource));

        let instance_provenance = InspectionProvenance {
            layer:    "instance".into(),
            resource: instance_resource.clone(),
            origin:   instance_origin.clone(),
        };
        let blueprint_provenance =
            blueprint_resource
                .as_ref()
                .map(|resource| InspectionProvenance {
                    layer:    "blueprint".into(),
                    resource: resource.clone(),
                    origin:   blueprint_origin.clone(),
                });

        let mut effective = blueprint
            .as_ref()
            .map_or_else(|| GffStruct::new(instance.id), |root| root.root.clone());
        for (label, field) in instance.fields() {
            effective
                .put_field(label.clone(), field.clone())
                .map_err(|error| SceneError::invalid(error.to_string()))?;
        }

        let mut references = BTreeMap::new();
        let mut sections = section_catalog(object.kind);
        let resource_context = blueprint_extension(object.kind);
        for (name, field) in effective.fields() {
            let provenance = if instance.get_field(name).is_some() {
                instance_provenance.clone()
            } else {
                blueprint_provenance
                    .clone()
                    .unwrap_or_else(|| instance_provenance.clone())
            };
            let inspected = self.inspect_field(
                name,
                field,
                object.kind,
                resource_context,
                provenance,
                &mut references,
            );
            let section_id = section_for_field(object.kind, name);
            if let Some(section) = sections.iter_mut().find(|section| section.id == section_id) {
                section.fields.push(inspected);
            }
        }
        sections.retain(|section| !section.fields.is_empty());

        let instance_data = self.inspect_structure(
            instance,
            object.kind,
            resource_context,
            instance_provenance.clone(),
            &mut references,
        );
        let mut sources = vec![InspectionSource {
            layer:    "instance".into(),
            resource: instance_resource,
            origin:   instance_origin,
            data:     instance_data,
        }];
        if let (Some(root), Some(resource), Some(provenance)) =
            (blueprint.as_ref(), blueprint_resource, blueprint_provenance)
        {
            let data = self.inspect_structure(
                &root.root,
                object.kind,
                resource_context,
                provenance.clone(),
                &mut references,
            );
            sources.push(InspectionSource {
                layer: "blueprint".into(),
                resource,
                origin: provenance.origin,
                data,
            });
        }

        Ok(AreaObjectInspection {
            schema: "nwnrs.area-object-inspection".into(),
            key: object.key.clone(),
            label: object.label.clone(),
            kind: object.kind,
            sections,
            sources,
            references: references.into_values().collect(),
            diagnostics,
        })
    }

    fn load_gff(&mut self, resource: &str) -> SceneResult<nwnrs_types::gff::GffRoot> {
        let resolved = ResolvedResRef::from_filename(resource)
            .map_err(|error| SceneError::invalid(error.to_string()))?;
        let res = self
            .resman
            .get_resolved(&resolved)
            .ok_or_else(|| SceneError::missing(resource.to_string()))?;
        let bytes = res
            .read_all(CachePolicy::Use)
            .map_err(|error| SceneError::invalid(error.to_string()))?;
        nwnrs_types::gff::read_gff_root(&mut Cursor::new(bytes))
            .map_err(|error| SceneError::invalid(error.to_string()))
    }

    fn resolve_origin(&mut self, resource: &str) -> Option<String> {
        ResolvedResRef::from_filename(resource)
            .ok()
            .and_then(|resolved| self.resman.get_resolved(&resolved))
            .map(|res| res.origin().to_string())
    }

    fn inspect_structure(
        &mut self,
        value: &GffStruct,
        object_kind: SceneInstanceKind,
        resource_context: &'static str,
        provenance: InspectionProvenance,
        references: &mut BTreeMap<String, InspectionResource>,
    ) -> InspectionStructure {
        InspectionStructure {
            id:     value.id,
            fields: value
                .fields()
                .iter()
                .map(|(name, field)| {
                    self.inspect_field(
                        name,
                        field,
                        object_kind,
                        resource_context,
                        provenance.clone(),
                        references,
                    )
                })
                .collect(),
        }
    }

    fn inspect_field(
        &mut self,
        name: &str,
        field: &GffField,
        object_kind: SceneInstanceKind,
        resource_context: &'static str,
        provenance: InspectionProvenance,
        references: &mut BTreeMap<String, InspectionResource>,
    ) -> InspectionField {
        let mut result = InspectionField {
            name:       name.into(),
            label:      friendly_label(name),
            kind:       gff_kind(field.value()).into(),
            display:    display_field_value(name, field.value()),
            text:       primitive_text(field.value()),
            struct_id:  None,
            fields:     Vec::new(),
            entries:    Vec::new(),
            value64:    None,
            localized:  None,
            resource:   None,
            lookup:     None,
            provenance: provenance.clone(),
        };
        match field.value() {
            GffValue::CExoLocString(value) => {
                let localized = self.localization.resolve(value);
                if let Some(text) = localized.text.as_ref() {
                    result.display.clone_from(text);
                }
                result.localized = Some(localized);
            }
            GffValue::Void(value) => result.value64 = Some(base64_encode(value)),
            GffValue::Struct(value) => {
                result.struct_id = Some(value.id);
                let child_context = nested_resource_context(name, resource_context);
                result.fields = self
                    .inspect_structure(
                        value,
                        object_kind,
                        child_context,
                        provenance.clone(),
                        references,
                    )
                    .fields;
            }
            GffValue::List(values) => {
                let child_context = nested_resource_context(name, resource_context);
                result.entries = values
                    .iter()
                    .map(|value| {
                        self.inspect_structure(
                            value,
                            object_kind,
                            child_context,
                            provenance.clone(),
                            references,
                        )
                    })
                    .collect();
            }
            _ => {}
        }
        if let Some(resource_name) = resource_for_field(resource_context, name, field.value()) {
            let origin = self.resolve_origin(&resource_name);
            let resource = InspectionResource {
                resolved: origin.is_some(),
                origin,
                resource: resource_name.clone(),
            };
            references
                .entry(resource_name)
                .or_insert_with(|| resource.clone());
            result.resource = Some(resource);
        }
        if let Some((table, columns)) = lookup_spec(object_kind, name)
            && let Some(row) =
                integer_value(field.value()).and_then(|value| usize::try_from(value).ok())
        {
            result.lookup = self.lookup_2da(table, row, columns);
            if let Some(lookup) = result.lookup.as_ref() {
                references
                    .entry(lookup.resource.clone())
                    .or_insert_with(|| InspectionResource {
                        resource: lookup.resource.clone(),
                        origin:   lookup.origin.clone(),
                        resolved: true,
                    });
            }
        }
        result
    }

    fn lookup_2da(
        &mut self,
        resource: &str,
        row: usize,
        columns: &[&str],
    ) -> Option<InspectionTwoDaLookup> {
        if !self.cache.tables.contains_key(resource) {
            let table = ResolvedResRef::from_filename(resource)
                .ok()
                .and_then(|resolved| self.resman.get_resolved(&resolved))
                .and_then(|res| res.read_all(CachePolicy::Use).ok())
                .and_then(|bytes| read_twoda(&mut Cursor::new(bytes)).ok());
            self.cache.tables.insert(resource.into(), table);
        }
        let table = self.cache.tables.get(resource)?.as_ref()?;
        if row >= table.rows.len() {
            return None;
        }
        let selected = columns.iter().find_map(|column| {
            table
                .cell(row, column)
                .filter(|value| !value.trim().is_empty())
                .map(|value| ((*column).to_string(), value))
        });
        let (column, mut label) = selected.map_or_else(
            || (None, table.row_label(row).map(str::to_string)),
            |(column, value)| (Some(column), Some(value)),
        );
        if column.as_deref().is_some_and(is_string_ref_column)
            && let Some(str_ref) = label.as_deref().and_then(|value| value.parse::<u32>().ok())
        {
            let loc = GffCExoLocString {
                str_ref,
                entries: Vec::new(),
            };
            if let Some(text) = self.localization.resolve(&loc).text {
                label = Some(text);
            }
        }
        Some(InspectionTwoDaLookup {
            resource: resource.into(),
            row,
            label,
            column,
            origin: self.resolve_origin(resource),
        })
    }
}

fn object_structure<'a>(
    git: &'a nwnrs_types::gff::GitFile,
    object: &SceneAreaObject,
) -> Option<&'a GffStruct> {
    match object.kind {
        SceneInstanceKind::Creature => git
            .creatures
            .get(object.source_index)
            .map(|value| &value.raw),
        SceneInstanceKind::Door => git.doors.get(object.source_index).map(|value| &value.raw),
        SceneInstanceKind::Placeable => git
            .placeables
            .get(object.source_index)
            .map(|value| &value.raw),
        SceneInstanceKind::Store => git.stores.get(object.source_index).map(|value| &value.raw),
        SceneInstanceKind::Encounter => git
            .encounters
            .get(object.source_index)
            .map(|value| &value.raw),
        SceneInstanceKind::Trigger => git
            .triggers
            .get(object.source_index)
            .map(|value| &value.raw),
        SceneInstanceKind::Waypoint => git
            .waypoints
            .get(object.source_index)
            .map(|value| &value.raw),
        SceneInstanceKind::Sound => git.sounds.get(object.source_index).map(|value| &value.raw),
        _ => None,
    }
}

fn template_resref(value: &GffStruct) -> Option<&str> {
    ["TemplateResRef", "ResRef"].into_iter().find_map(|label| {
        value
            .get_field(label)
            .and_then(|field| match field.value() {
                GffValue::ResRef(value) | GffValue::CExoString(value)
                    if !value.trim().is_empty() =>
                {
                    Some(value.as_str())
                }
                _ => None,
            })
    })
}

const fn blueprint_extension(kind: SceneInstanceKind) -> &'static str {
    match kind {
        SceneInstanceKind::Creature => "utc",
        SceneInstanceKind::Door => "utd",
        SceneInstanceKind::Placeable => "utp",
        SceneInstanceKind::Store => "utm",
        SceneInstanceKind::Encounter => "ute",
        SceneInstanceKind::Trigger => "utt",
        SceneInstanceKind::Waypoint => "utw",
        SceneInstanceKind::Sound => "uts",
        _ => "gff",
    }
}

fn section_catalog(kind: SceneInstanceKind) -> Vec<InspectionSection> {
    let mut result = vec![
        section("identity", "Identity & Text", true),
        section("transform", "Transform", false),
    ];
    match kind {
        SceneInstanceKind::Creature => result.extend([
            section("appearance", "Appearance", false),
            section("gameplay", "Stats & Gameplay", false),
            section("inventory", "Inventory & Equipment", false),
            section("conversation", "Conversation", false),
        ]),
        SceneInstanceKind::Placeable | SceneInstanceKind::Door => result.extend([
            section("appearance", "Appearance", false),
            section("gameplay", "State & Gameplay", false),
            section("inventory", "Inventory", false),
            section("locks", "Lock & Trap", false),
            section("conversation", "Conversation", false),
        ]),
        SceneInstanceKind::Store => result.extend([
            section("gameplay", "Store Settings", false),
            section("inventory", "Inventory", false),
        ]),
        SceneInstanceKind::Encounter => {
            result.push(section("spawning", "Encounter & Spawning", false));
        }
        SceneInstanceKind::Trigger => result.extend([
            section("gameplay", "Trigger & Transition", false),
            section("locks", "Trap", false),
        ]),
        SceneInstanceKind::Waypoint => result.push(section("gameplay", "Map & Transition", false)),
        SceneInstanceKind::Sound => result.push(section("audio", "Audio", false)),
        _ => {}
    }
    result.extend([
        section("scripts", "Scripts", false),
        section("locals", "Local Variables", false),
        section("advanced", "Additional Authored Fields", false),
    ]);
    result
}

fn section(id: &str, label: &str, default_open: bool) -> InspectionSection {
    InspectionSection {
        id: id.into(),
        label: label.into(),
        default_open,
        fields: Vec::new(),
    }
}

fn section_for_field(kind: SceneInstanceKind, name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "tag"
            | "templateresref"
            | "resref"
            | "locname"
            | "localizedname"
            | "firstname"
            | "lastname"
            | "description"
            | "comment"
            | "deity"
            | "subrace"
            | "mapnote"
            | "mapnoteenabled"
    ) {
        return "identity";
    }
    if matches!(
        lower.as_str(),
        "x" | "y"
            | "z"
            | "xposition"
            | "yposition"
            | "zposition"
            | "xorientation"
            | "yorientation"
            | "bearing"
            | "geometry"
    ) {
        return "transform";
    }
    if lower == "vartable" || lower.contains("localvar") {
        return "locals";
    }
    if lower.starts_with("on") || lower.contains("script") {
        return "scripts";
    }
    if lower.contains("conversation") {
        return "conversation";
    }
    if lower.contains("inventory") || lower.contains("itemlist") || lower.contains("equip_item") {
        return "inventory";
    }
    if lower.contains("lock") || lower.contains("trap") || lower.contains("keyname") {
        return "locks";
    }
    if matches!(kind, SceneInstanceKind::Sound) {
        return "audio";
    }
    if matches!(kind, SceneInstanceKind::Encounter)
        && (lower.contains("creature")
            || lower.contains("spawn")
            || lower.contains("respawn")
            || lower.contains("difficulty")
            || lower.contains("active")
            || lower.contains("exhaust"))
    {
        return "spawning";
    }
    if lower.contains("appearance")
        || lower.contains("portrait")
        || lower.contains("phenotype")
        || lower.contains("bodypart")
        || lower.contains("color")
        || matches!(lower.as_str(), "race" | "gender")
    {
        return "appearance";
    }
    if lower.contains("hp")
        || lower.ends_with("ac")
        || lower.contains("armorclass")
        || lower.contains("save")
        || lower.contains("class")
        || lower.contains("feat")
        || lower.contains("skill")
        || lower.contains("spell")
        || lower.contains("faction")
        || lower.contains("alignment")
        || lower.contains("strength")
        || lower.contains("dexterity")
        || lower.contains("constitution")
        || lower.contains("intelligence")
        || lower.contains("wisdom")
        || lower.contains("charisma")
        || matches!(
            lower.as_str(),
            "static" | "useable" | "plot" | "hardness" | "challengerating"
        )
    {
        return "gameplay";
    }
    if matches!(
        kind,
        SceneInstanceKind::Store | SceneInstanceKind::Trigger | SceneInstanceKind::Waypoint
    ) {
        "gameplay"
    } else {
        "advanced"
    }
}

fn gff_kind(value: &GffValue) -> &'static str {
    match value {
        GffValue::Byte(_) => "byte",
        GffValue::Char(_) => "char",
        GffValue::Word(_) => "word",
        GffValue::Short(_) => "short",
        GffValue::Dword(_) => "dword",
        GffValue::Int(_) => "int",
        GffValue::Float(_) => "float",
        GffValue::Dword64(_) => "dword64",
        GffValue::Int64(_) => "int64",
        GffValue::Double(_) => "double",
        GffValue::CExoString(_) => "cexostring",
        GffValue::ResRef(_) => "resref",
        GffValue::CExoLocString(_) => "cexolocstring",
        GffValue::Void(_) => "void",
        GffValue::Struct(_) => "struct",
        GffValue::List(_) => "list",
    }
}

fn display_value(value: &GffValue) -> String {
    match value {
        GffValue::Byte(value) => value.to_string(),
        GffValue::Char(value) => value.to_string(),
        GffValue::Word(value) => value.to_string(),
        GffValue::Short(value) => value.to_string(),
        GffValue::Dword(value) => value.to_string(),
        GffValue::Int(value) => value.to_string(),
        GffValue::Float(value) => value.to_string(),
        GffValue::Dword64(value) => value.to_string(),
        GffValue::Int64(value) => value.to_string(),
        GffValue::Double(value) => value.to_string(),
        GffValue::CExoString(value) | GffValue::ResRef(value) => value.clone(),
        GffValue::CExoLocString(value) => value.entries.first().map_or_else(
            || {
                if value.str_ref == BAD_STRREF {
                    "unresolved".into()
                } else {
                    format!("strref {}", value.str_ref)
                }
            },
            |(_, text)| text.clone(),
        ),
        GffValue::Void(value) => format!("{} bytes", value.len()),
        GffValue::Struct(value) => format!("struct {} · {} fields", value.id, value.fields().len()),
        GffValue::List(value) => format!("{} entries", value.len()),
    }
}

fn display_field_value(name: &str, value: &GffValue) -> String {
    if boolean_field(name)
        && let Some(value) = integer_value(value)
        && matches!(value, 0 | 1)
    {
        return if value == 0 { "No" } else { "Yes" }.into();
    }
    display_value(value)
}

fn boolean_field(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "active"
            | "autoremovekey"
            | "continuoussounds"
            | "disarmable"
            | "exhausted"
            | "hasinventory"
            | "immortal"
            | "interruptable"
            | "keyrequired"
            | "lockable"
            | "locked"
            | "looping"
            | "mapnoteenabled"
            | "nopermdeath"
            | "partyinteract"
            | "plot"
            | "positional"
            | "random"
            | "randomposition"
            | "static"
            | "trapdetectable"
            | "trapdisarmable"
            | "trapflag"
            | "traponeshot"
            | "useable"
    )
}

fn primitive_text(value: &GffValue) -> Option<String> {
    match value {
        GffValue::Struct(_)
        | GffValue::List(_)
        | GffValue::Void(_)
        | GffValue::CExoLocString(_) => None,
        _ => Some(display_value(value)),
    }
}

fn integer_value(value: &GffValue) -> Option<i128> {
    match value {
        GffValue::Byte(value) => Some(i128::from(*value)),
        GffValue::Char(value) => Some(i128::from(*value)),
        GffValue::Word(value) => Some(i128::from(*value)),
        GffValue::Short(value) => Some(i128::from(*value)),
        GffValue::Dword(value) => Some(i128::from(*value)),
        GffValue::Int(value) => Some(i128::from(*value)),
        GffValue::Dword64(value) => Some(i128::from(*value)),
        GffValue::Int64(value) => Some(i128::from(*value)),
        _ => None,
    }
}

fn friendly_label(value: &str) -> String {
    let mut result = String::with_capacity(value.len() + 8);
    let mut previous_lower = false;
    for character in value.chars() {
        if character == '_' {
            if !result.ends_with(' ') {
                result.push(' ');
            }
            previous_lower = false;
        } else {
            if character.is_ascii_uppercase() && previous_lower {
                result.push(' ');
            }
            result.push(character);
            previous_lower = character.is_ascii_lowercase() || character.is_ascii_digit();
        }
    }
    result
}

fn resource_for_field(
    resource_context: &'static str,
    name: &str,
    value: &GffValue,
) -> Option<String> {
    let raw = match value {
        GffValue::ResRef(value) | GffValue::CExoString(value) => value.trim(),
        _ => return None,
    };
    if raw.is_empty() {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    let extension = if matches!(lower.as_str(), "templateresref" | "resref") {
        resource_context
    } else if lower.contains("conversation") {
        "dlg"
    } else if lower.starts_with("on") || lower.contains("script") {
        "nss"
    } else if lower.contains("creatureresref") {
        "utc"
    } else if lower.contains("itemresref") || lower == "inventoryres" {
        "uti"
    } else if lower == "sound" || lower.contains("soundresref") {
        "wav"
    } else {
        return None;
    };
    Some(format!("{raw}.{extension}"))
}

fn nested_resource_context(name: &str, fallback: &'static str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.contains("item") || lower.contains("inventory") || lower.contains("equip") {
        "uti"
    } else if lower.contains("creature") || lower.contains("spawn") {
        "utc"
    } else {
        fallback
    }
}

fn lookup_spec(
    kind: SceneInstanceKind,
    name: &str,
) -> Option<(&'static str, &'static [&'static str])> {
    match (kind, name.to_ascii_lowercase().as_str()) {
        (SceneInstanceKind::Creature, "appearance_type" | "appearancetype" | "appearance") => {
            Some(("appearance.2da", &["LABEL", "STRING_REF"]))
        }
        (SceneInstanceKind::Placeable, "appearance") => {
            Some(("placeables.2da", &["Label", "StrRef"]))
        }
        (SceneInstanceKind::Door, "appearance") => Some(("genericdoors.2da", &["Label", "Name"])),
        (_, "portraitid") => Some(("portraits.2da", &["BaseResRef", "Portrait"])),
        (_, "race") => Some(("racialtypes.2da", &["Label", "Name"])),
        (_, "gender") => Some(("gender.2da", &["Label", "Name"])),
        (_, "phenotype") => Some(("phenotype.2da", &["Label", "Name"])),
        (_, "soundsetfile") => Some(("soundset.2da", &["Label", "StrRef"])),
        (_, "class") => Some(("classes.2da", &["Label", "Name"])),
        (_, "feat") => Some(("feat.2da", &["Label", "FEAT"])),
        (_, "skill") => Some(("skills.2da", &["Label", "Name"])),
        (_, "spell") => Some(("spells.2da", &["Label", "Name"])),
        (_, "baseitem") => Some(("baseitems.2da", &["Label", "Name"])),
        (_, "traptype") => Some(("traps.2da", &["Label", "Name"])),
        (_, "bodybag") => Some(("bodybag.2da", &["Label", "Name"])),
        _ => None,
    }
}

fn is_string_ref_column(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "name" | "strref" | "string_ref" | "feat"
    )
}

fn base64_encode(bytes: &[u8]) -> String {
    BASE64.encode(bytes)
}

#[cfg(test)]
mod tests {
    use nwnrs_types::{
        gff::{AreEnvironment, AreFile, GffRoot, GitFile, parse_git_root},
        set::SetFile,
    };

    use super::*;
    use crate::scene::{DependencyGraph, SceneArea, SceneDocument, SceneEnvironment, SceneSource};

    struct InlineLocalization;

    impl InspectionLocalizationResolver for InlineLocalization {
        fn resolve(&mut self, value: &GffCExoLocString) -> InspectionLocalizedString {
            InspectionLocalizedString {
                text:        value.entries.first().map(|(_, text)| text.clone()),
                str_ref:     (value.str_ref != BAD_STRREF).then_some(value.str_ref),
                source:      Some("test".into()),
                language_id: Some(0),
                gender:      Some("male".into()),
                entries:     value
                    .entries
                    .iter()
                    .map(|(id, text)| InspectionLocalizedEntry {
                        id:   *id,
                        text: text.clone(),
                    })
                    .collect(),
            }
        }
    }

    #[test]
    fn friendly_labels_preserve_acronyms_and_split_words() {
        assert_eq!(friendly_label("CurrentHP"), "Current HP");
        assert_eq!(friendly_label("Appearance_Type"), "Appearance Type");
        assert_eq!(friendly_label("OnHeartbeat"), "On Heartbeat");
    }

    #[test]
    fn void_values_use_standard_base64() {
        assert_eq!(base64_encode(&[0, 1, 2, 253, 254, 255]), "AAEC/f7/");
        assert_eq!(base64_encode(&[1]), "AQ==");
    }

    #[test]
    fn nested_blueprint_references_use_their_container_semantics() {
        assert_eq!(nested_resource_context("ItemList", "utp"), "uti");
        assert_eq!(nested_resource_context("CreatureList", "ute"), "utc");
        assert_eq!(
            resource_for_field(
                "uti",
                "TemplateResRef",
                &GffValue::ResRef("nw_it_gold001".into()),
            ),
            Some("nw_it_gold001.uti".into()),
        );
    }

    #[test]
    fn inspection_preserves_every_instance_field_and_builds_type_sections() {
        let mut placeable = GffStruct::new(9);
        placeable
            .put_value("Tag", GffValue::CExoString("sign_hello".into()))
            .unwrap_or_else(|error| panic!("tag: {error}"));
        placeable
            .put_value(
                "Description",
                GffValue::CExoLocString(GffCExoLocString {
                    str_ref: BAD_STRREF,
                    entries: vec![(0, "A weathered sign.".into())],
                }),
            )
            .unwrap_or_else(|error| panic!("description: {error}"));
        placeable
            .put_value("OnUsed", GffValue::ResRef("sign_used".into()))
            .unwrap_or_else(|error| panic!("script: {error}"));
        placeable
            .put_value("VarTable", GffValue::List(vec![GffStruct::new(0)]))
            .unwrap_or_else(|error| panic!("locals: {error}"));
        placeable
            .put_value("Opaque", GffValue::Void(vec![0, 1, 2]))
            .unwrap_or_else(|error| panic!("opaque: {error}"));
        let authored_field_count = placeable.fields().len();
        let mut git_root = GffRoot::new("GIT ");
        git_root
            .root
            .put_value("Placeable List", GffValue::List(vec![placeable]))
            .unwrap_or_else(|error| panic!("placeable list: {error}"));
        let git = parse_git_root(&git_root).unwrap_or_else(|error| panic!("parse GIT: {error}"));
        let scene = test_area_scene(git);
        let mut resman = ResMan::new(0);
        let mut cache = AreaInspectionCache::default();
        let mut localization = InlineLocalization;

        let inspection = AreaInspector::new(&mut resman, &mut cache, &mut localization)
            .inspect(&scene, "placeable:0")
            .unwrap_or_else(|error| panic!("inspect: {error}"));

        assert_eq!(
            inspection
                .sections
                .iter()
                .map(|section| section.fields.len())
                .sum::<usize>(),
            authored_field_count,
        );
        assert_eq!(inspection.sources.len(), 1);
        assert_eq!(
            inspection.sources[0].data.fields.len(),
            authored_field_count
        );
        let identity = inspection
            .sections
            .iter()
            .find(|section| section.id == "identity")
            .unwrap_or_else(|| panic!("identity section"));
        assert!(
            identity.fields.iter().any(|field| {
                field.name == "Description" && field.display == "A weathered sign."
            })
        );
        assert!(
            inspection
                .sections
                .iter()
                .any(|section| section.id == "scripts")
        );
        assert!(
            inspection
                .sections
                .iter()
                .any(|section| section.id == "locals")
        );
    }

    fn test_area_scene(git: GitFile) -> SceneDocument {
        SceneDocument {
            name:         "start".into(),
            source:       SceneSource::Area,
            models:       Vec::new(),
            model_assets: Vec::new(),
            textures:     Vec::new(),
            shaders:      Vec::new(),
            instances:    Vec::new(),
            area:         Some(SceneArea {
                area:      AreFile {
                    raw:         GffRoot::new("ARE "),
                    resref:      Some("start".into()),
                    tag:         Some("start".into()),
                    width:       0,
                    height:      0,
                    tileset:     None,
                    tiles:       Vec::new(),
                    environment: AreEnvironment::default(),
                },
                instances: git,
                tileset:   SetFile::default(),
            }),
            module:       None,
            environment:  SceneEnvironment::Studio,
            dependencies: DependencyGraph::default(),
            diagnostics:  Vec::new(),
        }
    }
}
