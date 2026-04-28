use std::collections::BTreeMap;

use nwnrs_resman::prelude::ResMan;

use crate::{MODEL_RES_TYPE, NwnScene, NwnTextureRef, NwnTextureSlot};

/// User-selected appearance remaps for one model load.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct NwnAppearanceOverrides {
    /// Maps stable appearance slot ids to selected replacement model names.
    pub slots:    BTreeMap<String, String>,
    /// Maps PLT layer ids to selected palette-row indices.
    pub plt_rows: BTreeMap<u8, u8>,
}

impl NwnAppearanceOverrides {
    /// Returns the selected replacement for `slot_id`, when present.
    #[must_use]
    pub fn get(&self, slot_id: &str) -> Option<&str> {
        self.slots
            .iter()
            .find(|(key, _value)| key.eq_ignore_ascii_case(slot_id))
            .map(|(_key, value)| value.as_str())
    }

    /// Returns the selected palette row for one PLT layer id.
    #[must_use]
    pub fn plt_row(&self, layer_id: u8) -> Option<u8> {
        self.plt_rows.get(&layer_id).copied()
    }
}

/// One selectable appearance slot detected in a model scene.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NwnAppearanceSlot {
    /// Stable slot identifier used for overrides.
    pub id:          String,
    /// Human-readable label for UI display.
    pub label:       String,
    /// Authored token found in the model.
    pub token:       String,
    /// Source node names using this slot.
    pub node_names:  Vec<String>,
    /// Parsed `#part-number` when present.
    pub part_number: Option<i32>,
    /// Normalized form used for matching shipped assets.
    pub normalized:  String,
    /// Stable stem shared by all candidate model names for this slot.
    pub family:      String,
    /// Selectable model-name candidates.
    pub options:     Vec<String>,
}

/// Collects appearance slots from a lowered model scene by scanning model-like
/// bitmap tokens and matching installed part-model candidates.
#[must_use]
pub fn collect_appearance_slots(scene: &NwnScene, resman: &ResMan) -> Vec<NwnAppearanceSlot> {
    let installed_models = resman
        .contents()
        .into_iter()
        .filter(|resref| resref.res_type() == MODEL_RES_TYPE)
        .map(|resref| resref.res_ref().to_string())
        .collect::<Vec<_>>();

    let mut slots_by_id = BTreeMap::<String, NwnAppearanceSlot>::new();
    for material in &scene.materials {
        let Some(source_node) = scene.nodes.get(material.source_node) else {
            continue;
        };
        for texture in &material.textures {
            if !matches!(texture.slot, NwnTextureSlot::Bitmap) {
                continue;
            }
            let Some(parsed) = parse_appearance_token(texture) else {
                continue;
            };
            let slot_id = slot_id_for_node(source_node.part_number, parsed.token.as_str());

            let mut options = installed_models
                .iter()
                .filter(|candidate| candidate_stem(candidate) == Some(parsed.family.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            options.sort_unstable();
            options.dedup();
            if options.is_empty() {
                continue;
            }

            let entry = slots_by_id
                .entry(slot_id.clone())
                .or_insert_with(|| NwnAppearanceSlot {
                    id:          slot_id.clone(),
                    label:       slot_label(source_node.name.as_str(), source_node.part_number),
                    token:       parsed.token.clone(),
                    node_names:  Vec::new(),
                    part_number: source_node.part_number,
                    normalized:  parsed.normalized.clone(),
                    family:      parsed.family.clone(),
                    options:     options.clone(),
                });
            if !entry
                .node_names
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(source_node.name.as_str()))
            {
                entry.node_names.push(source_node.name.clone());
                entry.node_names.sort_unstable();
            }
        }
    }

    let mut slots = slots_by_id.into_values().collect::<Vec<_>>();
    slots.sort_by(|left, right| left.label.cmp(&right.label));
    slots
}

/// Applies appearance overrides by rewriting authored bitmap tokens inside a
/// cloned scene before texture resolution runs.
#[must_use]
pub fn apply_appearance_overrides(
    scene: &NwnScene,
    overrides: &NwnAppearanceOverrides,
) -> NwnScene {
    let mut scene = scene.clone();
    if overrides.slots.is_empty() {
        return scene;
    }

    for material in &mut scene.materials {
        let part_number = scene
            .nodes
            .get(material.source_node)
            .and_then(|node| node.part_number);
        for texture in &mut material.textures {
            if !matches!(texture.slot, NwnTextureSlot::Bitmap) {
                continue;
            }
            let slot_id = slot_id_for_node(part_number, texture.name.as_str());
            if let Some(replacement) = overrides.get(slot_id.as_str()) {
                texture.name = replacement.to_string();
            }
        }
    }

    scene
}

fn slot_id_for_node(part_number: Option<i32>, token: &str) -> String {
    match part_number {
        Some(part_number) => format!("part:{part_number}"),
        None => format!("token:{}", token.to_ascii_lowercase()),
    }
}

fn slot_label(node_name: &str, part_number: Option<i32>) -> String {
    match part_number {
        Some(part_number) => format!("{node_name} (part {part_number})"),
        None => node_name.to_string(),
    }
}

#[derive(Debug, Clone)]
struct ParsedAppearanceToken {
    token:      String,
    normalized: String,
    family:     String,
}

fn parse_appearance_token(texture: &NwnTextureRef) -> Option<ParsedAppearanceToken> {
    let token = texture.name.trim();
    if token.is_empty()
        || token.eq_ignore_ascii_case("null")
        || token.eq_ignore_ascii_case("material")
        || token.eq_ignore_ascii_case("coat_bones")
    {
        return None;
    }

    let normalized = normalize_appearance_token(token);
    let family = candidate_stem(normalized.as_str())?.to_string();
    Some(ParsedAppearanceToken {
        token: token.to_string(),
        normalized,
        family,
    })
}

fn normalize_appearance_token(token: &str) -> String {
    let trimmed = token.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.ends_with('g') {
        let base = &trimmed[..trimmed.len() - 1];
        if candidate_stem(base).is_some() {
            return base.to_string();
        }
    }
    trimmed.to_string()
}

fn candidate_stem(candidate: &str) -> Option<&str> {
    let normalized = candidate.trim();
    let lower = normalized.to_ascii_lowercase();
    if lower.is_empty() || lower.contains('.') {
        return None;
    }

    let (prefix, suffix) = lower.split_once('_')?;
    let mut prefix_chars = prefix.chars();
    let first = prefix_chars.next()?;
    let second = prefix_chars.next()?;
    let third = prefix_chars.next()?;
    let fourth = prefix_chars.next()?;
    if !matches!(first, 'p' | 'i') || !matches!(second, 'm' | 'f') {
        return None;
    }
    if !third.is_ascii_alphabetic() || !fourth.is_ascii_digit() {
        return None;
    }

    let digit_start = suffix.find(|ch: char| ch.is_ascii_digit())?;
    let digits = &suffix[digit_start..];
    if digits.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    Some(&normalized[..normalized.len() - digits.len()])
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::candidate_stem;
    use crate::{NwnAppearanceOverrides, apply_appearance_overrides, parse_scene_model};

    #[test]
    fn candidate_stem_extracts_family() {
        assert_eq!(candidate_stem("pmh0_robe035"), Some("pmh0_robe"));
        assert_eq!(candidate_stem("pmh0_head001"), Some("pmh0_head"));
        assert_eq!(candidate_stem("TF3_g"), None);
    }

    #[test]
    fn apply_appearance_overrides_rewrites_bitmap_tokens() {
        let parsed = parse_scene_model(
            "\
newmodel demo
setsupermodel demo null
classification character
setanimationscale 1
beginmodelgeom demo
node dummy demo
  parent NULL
endnode
node trimesh pelvis
  parent demo
  #part-number 0
  render 1
  bitmap pmh0_chest001
  verts 3
    0 0 0
    1 0 0
    0 1 0
  faces 1
    0 1 2 1 0 1 2
endnode
endmodelgeom demo
donemodel demo
",
        )
        .unwrap_or_else(|error| panic!("parse scene: {error}"));
        let overrides = NwnAppearanceOverrides {
            slots:    [("part:0".to_string(), "pmh0_chest025".to_string())]
                .into_iter()
                .collect(),
            plt_rows: BTreeMap::new(),
        };

        let scene = apply_appearance_overrides(&parsed, &overrides);
        let texture = scene
            .materials
            .first()
            .and_then(|material| material.textures.first())
            .unwrap_or_else(|| panic!("overridden scene missing first texture"));
        assert_eq!(texture.name, "pmh0_chest025");
    }
}
