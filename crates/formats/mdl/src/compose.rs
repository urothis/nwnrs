use std::{collections::BTreeSet, io::Cursor};

use nwnrs_gff::prelude::{GffRoot, GffStruct, GffValue, read_gff_root};
use nwnrs_resman::prelude::{CachePolicy, Res, ResMan};
use nwnrs_resref::prelude::{ResRef, ResolvedResRef};
use nwnrs_twoda::prelude::{TwoDa, as_2da};

use crate::{
    MODEL_RES_TYPE, ModelError, ModelResult, NwnAppearanceOverrides, NwnScene,
    apply_appearance_overrides,
};

const INVENTORY_SLOT_MASK_HEAD: i32 = 1;
const INVENTORY_SLOT_MASK_CHEST: i32 = 2;
const INVENTORY_SLOT_MASK_BOOTS: i32 = 4;
const INVENTORY_SLOT_MASK_ARMS: i32 = 8;
const INVENTORY_SLOT_MASK_RIGHT_HAND: i32 = 16;
const INVENTORY_SLOT_MASK_LEFT_HAND: i32 = 32;
const INVENTORY_SLOT_MASK_CLOAK: i32 = 64;
const INVENTORY_SLOT_MASK_LEFT_RING: i32 = 128;
const INVENTORY_SLOT_MASK_RIGHT_RING: i32 = 256;
const INVENTORY_SLOT_MASK_NECK: i32 = 512;
const INVENTORY_SLOT_MASK_BELT: i32 = 1024;

/// A composed NWN scene tree with explicit attachment points and suppressed
/// base-geometry nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnComposedScene {
    /// Source model name.
    pub model_name:            String,
    /// Scene data for this model.
    pub scene:                 NwnScene,
    /// Node names whose base geometry should be skipped when flattening.
    pub hidden_geometry_nodes: Vec<String>,
    /// Child scenes attached to specific node names.
    pub attachments:           Vec<NwnSceneAttachment>,
}

/// One child model attached to a scene node.
#[derive(Debug, Clone, PartialEq)]
pub struct NwnSceneAttachment {
    /// Target node name on the parent scene.
    pub target_node_name: String,
    /// Referenced child model name.
    pub model_name:       String,
    /// Loaded child scene tree.
    pub scene:            Box<NwnComposedScene>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlayerCreatureFamily {
    base_model_name: String,
    model_prefix:    String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CreaturePartAttachment {
    node_name:            String,
    model_name:           String,
    appearance_overrides: NwnAppearanceOverrides,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EquippedPaperdoll {
    armor:        Option<EquippedArmorVisual>,
    helmet:       Option<EquippedItemVisual>,
    right_hand:   Option<EquippedItemVisual>,
    left_hand:    Option<EquippedItemVisual>,
    cloak:        Option<EquippedItemVisual>,
    neck:         Option<EquippedItemVisual>,
    belt:         Option<EquippedItemVisual>,
    boots:        Option<EquippedBootVisual>,
    gloves:       Option<EquippedPartVisual>,
    bracers:      Option<EquippedPartVisual>,
    left_ring:    Option<EquippedPartVisual>,
    right_ring:   Option<EquippedPartVisual>,
    hidden_parts: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EquippedItemVisual {
    model_name:           String,
    appearance_overrides: NwnAppearanceOverrides,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EquippedPartVisual {
    model_number:         u32,
    appearance_overrides: NwnAppearanceOverrides,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EquippedBootVisual {
    foot_model_number:    Option<u32>,
    shin_model_number:    Option<u32>,
    leg_model_number:     Option<u32>,
    appearance_overrides: NwnAppearanceOverrides,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EquippedArmorVisual {
    parts:                std::collections::BTreeMap<String, u32>,
    robe_model_number:    Option<u32>,
    appearance_overrides: NwnAppearanceOverrides,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BaseItemInfo {
    model_type: Option<u32>,
    item_class: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CloakModelInfo {
    model_number: u32,
    hidden_parts: BTreeSet<String>,
}

/// Loads one NWN model from `resman`, applies appearance overrides, and
/// resolves authored `refmodel` attachments into a composed scene tree.
pub fn load_composed_scene_from_resman(
    resman: &mut ResMan,
    model_name: &str,
    overrides: &NwnAppearanceOverrides,
) -> ModelResult<NwnComposedScene> {
    let scene = load_scene_from_resman_with_overrides(resman, model_name, overrides)?;
    let mut attachments = Vec::new();
    for node in &scene.nodes {
        let Some(reference_model) = node
            .reference
            .as_ref()
            .and_then(|reference| reference.model.as_deref())
        else {
            continue;
        };
        attachments.push(NwnSceneAttachment {
            target_node_name: node.name.clone(),
            model_name:       reference_model.to_string(),
            scene:            Box::new(load_composed_scene_from_resman(
                resman,
                reference_model,
                overrides,
            )?),
        });
    }

    Ok(NwnComposedScene {
        model_name: model_name.to_string(),
        scene,
        hidden_geometry_nodes: Vec::new(),
        attachments,
    })
}

/// Composes an equipped player creature from one parsed `UTC` blueprint and
/// resolves all attached model parts into a composed scene tree.
pub fn compose_player_creature_from_utc(
    resman: &mut ResMan,
    root: &GffRoot,
) -> ModelResult<NwnComposedScene> {
    let Some(appearance) = creature_appearance_row_from_blueprint(resman, root)? else {
        return Err(ModelError::msg(
            "UTC does not resolve to a player appearance row",
        ));
    };
    let table = load_twoda(resman, "appearance")?;
    let Some(row) = twoda_row_index_for_appearance(&table, appearance) else {
        return Err(ModelError::msg(format!(
            "appearance.2da is missing row {appearance}"
        )));
    };
    let Some(race_token) = table
        .cell(row, "RACE")
        .map(|value| value.trim().to_string())
    else {
        return Err(ModelError::msg("appearance row is missing RACE"));
    };
    let model_type = table
        .cell(row, "MODELTYPE")
        .map(|value| value.trim().to_ascii_uppercase())
        .unwrap_or_default();
    if model_type != "P" && !is_player_appearance_token(race_token.as_str()) {
        return Err(ModelError::msg(format!(
            "appearance row {appearance} does not resolve to a player creature"
        )));
    }

    let Some(family) = player_creature_family_from_blueprint(resman, root, race_token.as_str())?
    else {
        return Err(ModelError::msg("failed to resolve player creature family"));
    };
    let creature_overrides = creature_paperdoll_appearance_overrides(root);
    let equipped = resolve_equipped_paperdoll(resman, root, &family)?;

    let mut model = load_composed_scene_from_resman(
        resman,
        family.base_model_name.as_str(),
        &creature_overrides,
    )?;
    let attachments = build_player_creature_part_attachments(
        resman,
        root,
        &family,
        &creature_overrides,
        &equipped,
    )?;
    model.hidden_geometry_nodes = hidden_geometry_nodes_for_attachments(&attachments);
    for attachment in attachments {
        model.attachments.push(NwnSceneAttachment {
            target_node_name: attachment.node_name.clone(),
            model_name:       attachment.model_name.clone(),
            scene:            Box::new(load_composed_scene_from_resman(
                resman,
                attachment.model_name.as_str(),
                &attachment.appearance_overrides,
            )?),
        });
    }
    Ok(model)
}

/// Loads a `UTC` blueprint from `resman` by resource name and composes it into
/// an equipped player-creature scene tree.
pub fn compose_player_creature_from_resman(
    resman: &mut ResMan,
    blueprint_name: &str,
) -> ModelResult<NwnComposedScene> {
    let root = load_gff_root_from_resman(resman, blueprint_name, "utc")?
        .ok_or_else(|| ModelError::msg(format!("utc not found in ResMan: {blueprint_name}.utc")))?;
    compose_player_creature_from_utc(resman, &root)
}

fn load_scene_from_resman_with_overrides(
    resman: &mut ResMan,
    model_name: &str,
    overrides: &NwnAppearanceOverrides,
) -> ModelResult<NwnScene> {
    let resref = ResRef::new(model_name.to_string(), MODEL_RES_TYPE)
        .map_err(|error| ModelError::msg(format!("invalid mdl resref {model_name}: {error}")))?;
    let res = resman
        .get(&resref)
        .ok_or_else(|| ModelError::msg(format!("model not found in ResMan: {model_name}.mdl")))?;
    let scene = NwnScene::from_auto_res(&res, CachePolicy::Use)?;
    Ok(apply_appearance_overrides(&scene, overrides))
}

fn creature_appearance_row_from_blueprint(
    resman: &mut ResMan,
    root: &GffRoot,
) -> ModelResult<Option<usize>> {
    if let Some(appearance) = gff_u32_any(
        &root.root,
        &["Appearance_Type", "AppearanceType", "Appearance"],
    ) {
        return Ok(Some(appearance as usize));
    }

    let Some(race) = gff_u32_any(&root.root, &["Race", "Subrace", "RacialType"]) else {
        return Ok(None);
    };
    racialtype_appearance_row(resman, race as usize)
}

fn player_creature_family_from_blueprint(
    resman: &mut ResMan,
    root: &GffRoot,
    race_token: &str,
) -> ModelResult<Option<PlayerCreatureFamily>> {
    let gender = gff_u32_any(&root.root, &["Gender"]).unwrap_or(0);
    let gender_token = match gender {
        1 => 'f',
        _ => 'm',
    };
    let race_token = race_token.trim().to_ascii_lowercase();
    let Some(race_letter) = race_token
        .chars()
        .next()
        .filter(|ch| ch.is_ascii_alphabetic())
    else {
        return Ok(None);
    };
    let phenotype = gff_u32_any(&root.root, &["Phenotype"]).unwrap_or(0);
    let phenotype_digit = player_creature_phenotype_digit(resman, phenotype as usize)?;
    let model_prefix = player_creature_model_prefix(gender_token, race_letter, phenotype_digit);
    Ok(Some(PlayerCreatureFamily {
        base_model_name: model_prefix.clone(),
        model_prefix,
    }))
}

fn player_creature_phenotype_digit(resman: &mut ResMan, phenotype: usize) -> ModelResult<u8> {
    let table = load_twoda(resman, "phenotype")?;
    Ok(default_player_phenotype_digit(&table, phenotype))
}

fn player_creature_model_prefix(
    gender_token: char,
    race_letter: char,
    phenotype_digit: u8,
) -> String {
    format!(
        "p{gender_token}{}{phenotype_digit}",
        race_letter.to_ascii_lowercase()
    )
}

fn default_player_phenotype_digit(table: &TwoDa, phenotype: usize) -> u8 {
    let Some(row) = twoda_row_index_for_appearance(table, phenotype) else {
        return 0;
    };
    table
        .cell(row, "DefaultPhenoType")
        .and_then(|value| value.trim().parse::<u8>().ok())
        .unwrap_or(0)
}

fn build_player_creature_part_attachments(
    resman: &mut ResMan,
    root: &GffRoot,
    family: &PlayerCreatureFamily,
    creature_overrides: &NwnAppearanceOverrides,
    equipped: &EquippedPaperdoll,
) -> ModelResult<Vec<CreaturePartAttachment>> {
    let mut attachments = Vec::new();
    let armor_overrides = equipped
        .armor
        .as_ref()
        .map(|armor| merged_appearance_overrides(creature_overrides, &armor.appearance_overrides))
        .unwrap_or_else(|| creature_overrides.clone());
    let equipped_part_overrides = equipped_player_part_overrides(equipped);
    let capart = load_twoda(resman, "capart")?;
    for row in 0..capart.len() {
        let Some(model_stem_value) = capart.cell(row, "MDLNAME") else {
            continue;
        };
        let Some(node_name_value) = capart.cell(row, "NODENAME") else {
            continue;
        };
        let model_stem = model_stem_value.trim();
        let node_name = node_name_value.trim();
        let model_stem_key = model_stem.to_ascii_uppercase();
        if equipped.hidden_parts.contains(&model_stem_key) {
            continue;
        }
        if model_stem.eq_ignore_ascii_case("robe") {
            if let Some(robe_model_number) = equipped
                .armor
                .as_ref()
                .and_then(|armor| armor.robe_model_number)
            {
                attachments.push(CreaturePartAttachment {
                    node_name:            "rootdummy".to_string(),
                    model_name:           format!(
                        "{}_robe{robe_model_number:03}",
                        family.model_prefix
                    ),
                    appearance_overrides: armor_overrides.clone(),
                });
            }
            continue;
        }
        if model_stem.is_empty()
            || model_stem == "****"
            || node_name.is_empty()
            || node_name == "****"
            || node_name.eq_ignore_ascii_case("root")
        {
            continue;
        }
        let (model_number, appearance_overrides) =
            if let Some(visual) = equipped_part_overrides.get(&model_stem_key) {
                (
                    visual.model_number,
                    merged_appearance_overrides(creature_overrides, &visual.appearance_overrides),
                )
            } else if let Some(model_number) = equipped
                .armor
                .as_ref()
                .and_then(|armor| armor.parts.get(&model_stem_key).copied())
            {
                (model_number, armor_overrides.clone())
            } else {
                (
                    creature_body_part_model_number(
                        &root.root,
                        creature_body_part_field_aliases(model_stem),
                    )
                    .unwrap_or(1),
                    creature_overrides.clone(),
                )
            };
        if model_number == 0 {
            continue;
        }
        attachments.push(CreaturePartAttachment {
            node_name: node_name.to_string(),
            model_name: format!(
                "{}_{}{:03}",
                family.model_prefix,
                model_stem.to_ascii_lowercase(),
                model_number
            ),
            appearance_overrides,
        });
    }

    let head_model_number =
        creature_body_part_model_number(&root.root, &["BodyPart_Head"]).unwrap_or(1);
    if head_model_number > 0 {
        attachments.push(CreaturePartAttachment {
            node_name:            "head_g".to_string(),
            model_name:           format!("{}_head{:03}", family.model_prefix, head_model_number),
            appearance_overrides: creature_overrides.clone(),
        });
    }

    if !equipped.hidden_parts.contains("TAIL")
        && let Some(tail_model) = appearance_model_name_from_named_twoda(
            resman,
            "tailmodel",
            usize::try_from(gff_u32_any(&root.root, &["Tail"]).unwrap_or(0)).unwrap_or_default(),
            "MODEL",
        )?
    {
        attachments.push(CreaturePartAttachment {
            node_name:            "tail".to_string(),
            model_name:           tail_model,
            appearance_overrides: creature_overrides.clone(),
        });
    }
    if !equipped.hidden_parts.contains("WINGS")
        && let Some(wing_model) = appearance_model_name_from_named_twoda(
            resman,
            "wingmodel",
            usize::try_from(gff_u32_any(&root.root, &["Wings"]).unwrap_or(0)).unwrap_or_default(),
            "MODEL",
        )?
    {
        attachments.push(CreaturePartAttachment {
            node_name:            "wings".to_string(),
            model_name:           wing_model,
            appearance_overrides: creature_overrides.clone(),
        });
    }
    if let Some(neck) = equipped.neck.as_ref() {
        attachments.push(CreaturePartAttachment {
            node_name:            "neck_g".to_string(),
            model_name:           neck.model_name.clone(),
            appearance_overrides: neck.appearance_overrides.clone(),
        });
    }
    if let Some(belt) = equipped.belt.as_ref() {
        attachments.push(CreaturePartAttachment {
            node_name:            "belt_g".to_string(),
            model_name:           belt.model_name.clone(),
            appearance_overrides: belt.appearance_overrides.clone(),
        });
    }
    if let Some(cloak) = equipped.cloak.as_ref() {
        attachments.push(CreaturePartAttachment {
            node_name:            "rootdummy".to_string(),
            model_name:           cloak.model_name.clone(),
            appearance_overrides: cloak.appearance_overrides.clone(),
        });
    }
    if let Some(helmet) = equipped.helmet.as_ref() {
        attachments.push(CreaturePartAttachment {
            node_name:            "head".to_string(),
            model_name:           helmet.model_name.clone(),
            appearance_overrides: merged_appearance_overrides(
                creature_overrides,
                &helmet.appearance_overrides,
            ),
        });
    }
    if let Some(right_hand) = equipped.right_hand.as_ref() {
        attachments.push(CreaturePartAttachment {
            node_name:            "rhand".to_string(),
            model_name:           right_hand.model_name.clone(),
            appearance_overrides: right_hand.appearance_overrides.clone(),
        });
    }
    if let Some(left_hand) = equipped.left_hand.as_ref() {
        attachments.push(CreaturePartAttachment {
            node_name:            "lhand".to_string(),
            model_name:           left_hand.model_name.clone(),
            appearance_overrides: left_hand.appearance_overrides.clone(),
        });
    }

    Ok(attachments)
}

fn hidden_geometry_nodes_for_attachments(attachments: &[CreaturePartAttachment]) -> Vec<String> {
    let mut hidden = Vec::new();
    for attachment in attachments {
        if attachment.node_name.eq_ignore_ascii_case("head") {
            continue;
        }
        if !hidden
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(attachment.node_name.as_str()))
        {
            hidden.push(attachment.node_name.clone());
        }
    }
    hidden
}

fn resolve_equipped_paperdoll(
    resman: &mut ResMan,
    root: &GffRoot,
    family: &PlayerCreatureFamily,
) -> ModelResult<EquippedPaperdoll> {
    let mut equipped = EquippedPaperdoll::default();
    let Some(entries) = root
        .root
        .get_field("Equip_ItemList")
        .map(|field| field.value())
        .and_then(gff_list)
    else {
        return Ok(equipped);
    };

    for entry in entries {
        let Some(resref) = gff_string(entry.get_field("EquippedRes").map(|field| field.value()))
        else {
            continue;
        };
        let Some(item_root) = load_gff_root_from_resman(resman, resref.as_str(), "uti")? else {
            continue;
        };
        match entry.id {
            INVENTORY_SLOT_MASK_CHEST => {
                if let Some(armor) = equipped_armor_visual_from_item(resman, &item_root)? {
                    equipped.armor = Some(armor);
                }
            }
            INVENTORY_SLOT_MASK_HEAD => {
                equipped.helmet = equipped_held_item_visual_from_item(resman, &item_root)?;
            }
            INVENTORY_SLOT_MASK_RIGHT_HAND => {
                equipped.right_hand = equipped_held_item_visual_from_item(resman, &item_root)?;
            }
            INVENTORY_SLOT_MASK_LEFT_HAND => {
                equipped.left_hand = equipped_held_item_visual_from_item(resman, &item_root)?;
            }
            INVENTORY_SLOT_MASK_BOOTS => {
                equipped.boots = equipped_boot_visual_from_item(resman, &item_root, family)?;
            }
            INVENTORY_SLOT_MASK_ARMS => {
                let Some(base_item) = gff_u32(
                    item_root
                        .root
                        .get_field("BaseItem")
                        .map(|field| field.value()),
                ) else {
                    continue;
                };
                let Some(base_item_info) = base_item_info(resman, base_item as usize)? else {
                    continue;
                };
                match base_item_info.item_class.as_deref() {
                    Some("it_glove") => {
                        equipped.gloves = equipped_symmetric_part_visual_from_item(
                            resman, &item_root, family, "HAND",
                        )?;
                    }
                    Some("it_bracer") => {
                        equipped.bracers = equipped_symmetric_part_visual_from_item(
                            resman, &item_root, family, "FORE",
                        )?;
                    }
                    _ => {}
                }
            }
            INVENTORY_SLOT_MASK_CLOAK => {
                if let Some((cloak, hidden_parts)) =
                    equipped_cloak_visual_from_item(resman, &item_root, family)?
                {
                    equipped.hidden_parts = hidden_parts;
                    equipped.cloak = Some(cloak);
                }
            }
            INVENTORY_SLOT_MASK_LEFT_RING => {
                equipped.left_ring =
                    equipped_single_part_visual_from_item(resman, &item_root, family, "HANDL")?;
            }
            INVENTORY_SLOT_MASK_RIGHT_RING => {
                equipped.right_ring =
                    equipped_single_part_visual_from_item(resman, &item_root, family, "HANDR")?;
            }
            INVENTORY_SLOT_MASK_NECK => {
                equipped.neck =
                    equipped_family_accessory_visual_from_item(resman, &item_root, family, "neck")?;
            }
            INVENTORY_SLOT_MASK_BELT => {
                equipped.belt =
                    equipped_family_accessory_visual_from_item(resman, &item_root, family, "belt")?;
            }
            _ => {}
        }
    }

    Ok(equipped)
}

fn equipped_armor_visual_from_item(
    resman: &mut ResMan,
    root: &GffRoot,
) -> ModelResult<Option<EquippedArmorVisual>> {
    let Some(base_item) = gff_u32(root.root.get_field("BaseItem").map(|field| field.value()))
    else {
        return Ok(None);
    };
    if base_item_info(resman, base_item as usize)?.and_then(|info| info.model_type) != Some(3) {
        return Ok(None);
    }

    let mut armor = EquippedArmorVisual::default();
    for model_stem in [
        "FOOTR", "FOOTL", "SHINR", "SHINL", "LEGR", "LEGL", "PELVIS", "CHEST", "BELT", "NECK",
        "FORER", "FOREL", "BICEPR", "BICEPL", "SHOR", "SHOL", "HANDR", "HANDL",
    ] {
        if let Some(model_number) =
            creature_body_part_model_number(&root.root, armor_part_field_aliases(model_stem))
                .filter(|value| *value > 0)
        {
            armor.parts.insert(model_stem.to_string(), model_number);
        }
    }
    if let Some(robe_model_number) =
        creature_body_part_model_number(&root.root, &["ArmorPart_Robe"]).filter(|value| *value > 0)
    {
        armor.robe_model_number = Some(robe_model_number);
    }
    armor.appearance_overrides = item_plt_appearance_overrides(&root.root);

    Ok(Some(armor))
}

fn equipped_held_item_visual_from_item(
    resman: &mut ResMan,
    root: &GffRoot,
) -> ModelResult<Option<EquippedItemVisual>> {
    let Some(base_item) = gff_u32(root.root.get_field("BaseItem").map(|field| field.value()))
    else {
        return Ok(None);
    };
    let Some(base_item_info) = base_item_info(resman, base_item as usize)? else {
        return Ok(None);
    };
    let Some(model_name) = held_item_model_name(resman, root, &base_item_info)? else {
        return Ok(None);
    };
    Ok(Some(EquippedItemVisual {
        model_name,
        appearance_overrides: item_plt_appearance_overrides(&root.root),
    }))
}

fn equipped_boot_visual_from_item(
    resman: &mut ResMan,
    root: &GffRoot,
    family: &PlayerCreatureFamily,
) -> ModelResult<Option<EquippedBootVisual>> {
    let Some(base_item) = gff_u32(root.root.get_field("BaseItem").map(|field| field.value()))
    else {
        return Ok(None);
    };
    let Some(base_item_info) = base_item_info(resman, base_item as usize)? else {
        return Ok(None);
    };
    if base_item_info.item_class.as_deref() != Some("it_boots") {
        return Ok(None);
    }
    let mut visual = EquippedBootVisual {
        appearance_overrides: item_plt_appearance_overrides(&root.root),
        ..Default::default()
    };
    visual.foot_model_number = item_model_part(root, "ModelPart1")
        .filter(|value| player_part_model_exists(resman, family, "FOOTR", *value));
    visual.shin_model_number = item_model_part(root, "ModelPart2")
        .filter(|value| player_part_model_exists(resman, family, "SHINR", *value));
    visual.leg_model_number = item_model_part(root, "ModelPart3")
        .filter(|value| player_part_model_exists(resman, family, "LEGR", *value));
    if visual.foot_model_number.is_none()
        && visual.shin_model_number.is_none()
        && visual.leg_model_number.is_none()
    {
        return Ok(None);
    }
    Ok(Some(visual))
}

fn equipped_symmetric_part_visual_from_item(
    resman: &mut ResMan,
    root: &GffRoot,
    family: &PlayerCreatureFamily,
    stem_prefix: &str,
) -> ModelResult<Option<EquippedPartVisual>> {
    let Some(model_number) = item_model_part(root, "ModelPart1") else {
        return Ok(None);
    };
    if !player_part_model_exists(resman, family, &format!("{stem_prefix}R"), model_number) {
        return Ok(None);
    }
    Ok(Some(EquippedPartVisual {
        model_number,
        appearance_overrides: item_plt_appearance_overrides(&root.root),
    }))
}

fn equipped_single_part_visual_from_item(
    resman: &mut ResMan,
    root: &GffRoot,
    family: &PlayerCreatureFamily,
    stem: &str,
) -> ModelResult<Option<EquippedPartVisual>> {
    let Some(model_number) = item_model_part(root, "ModelPart1") else {
        return Ok(None);
    };
    if !player_part_model_exists(resman, family, stem, model_number) {
        return Ok(None);
    }
    Ok(Some(EquippedPartVisual {
        model_number,
        appearance_overrides: item_plt_appearance_overrides(&root.root),
    }))
}

fn equipped_family_accessory_visual_from_item(
    resman: &mut ResMan,
    root: &GffRoot,
    family: &PlayerCreatureFamily,
    stem: &str,
) -> ModelResult<Option<EquippedItemVisual>> {
    let Some(model_part) = item_model_part(root, "ModelPart1") else {
        return Ok(None);
    };
    let model_name = format!("{}_{stem}{model_part:03}", family.model_prefix);
    if first_existing_model_candidate(resman, std::slice::from_ref(&model_name)).is_none() {
        return Ok(None);
    }
    Ok(Some(EquippedItemVisual {
        model_name,
        appearance_overrides: item_plt_appearance_overrides(&root.root),
    }))
}

fn equipped_cloak_visual_from_item(
    resman: &mut ResMan,
    root: &GffRoot,
    family: &PlayerCreatureFamily,
) -> ModelResult<Option<(EquippedItemVisual, BTreeSet<String>)>> {
    let Some(cloak_appearance) = item_model_part(root, "ModelPart1") else {
        return Ok(None);
    };
    let cloak_table = load_twoda(resman, "cloakmodel")?;
    let Some(info) = cloak_model_info(&cloak_table, cloak_appearance) else {
        return Ok(None);
    };
    let model_name = format!("{}_cloak_{:03}", family.model_prefix, info.model_number);
    if first_existing_model_candidate(resman, std::slice::from_ref(&model_name)).is_none() {
        return Ok(None);
    }
    Ok(Some((
        EquippedItemVisual {
            model_name,
            appearance_overrides: item_plt_appearance_overrides(&root.root),
        },
        info.hidden_parts,
    )))
}

fn base_item_info(resman: &mut ResMan, base_item: usize) -> ModelResult<Option<BaseItemInfo>> {
    let table = load_twoda(resman, "baseitems")?;
    let Some(row) = twoda_row_index_for_appearance(&table, base_item) else {
        return Ok(None);
    };
    Ok(Some(BaseItemInfo {
        model_type: table
            .cell(row, "ModelType")
            .and_then(|value| value.trim().parse::<u32>().ok()),
        item_class: table
            .cell(row, "ItemClass")
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty() && value != "****"),
    }))
}

fn cloak_model_info(table: &TwoDa, appearance: u32) -> Option<CloakModelInfo> {
    let row = twoda_row_index_for_appearance(table, appearance as usize)?;
    let model_number = table
        .cell(row, "MODEL")
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|value| *value > 0)?;
    let mut hidden_parts = BTreeSet::new();
    if twoda_truthy_cell(table, row, "HIDEWING") {
        hidden_parts.insert("WINGS".to_string());
    }
    if twoda_truthy_cell(table, row, "HIDETAIL") {
        hidden_parts.insert("TAIL".to_string());
    }
    if twoda_truthy_cell(table, row, "HIDESHOL") {
        hidden_parts.insert("SHOL".to_string());
    }
    if twoda_truthy_cell(table, row, "HIDESHOR") {
        hidden_parts.insert("SHOR".to_string());
    }
    Some(CloakModelInfo {
        model_number,
        hidden_parts,
    })
}

fn twoda_truthy_cell(table: &TwoDa, row: usize, column: &str) -> bool {
    table
        .cell(row, column)
        .is_some_and(|value| matches!(value.trim(), "1" | "true" | "TRUE"))
}

fn item_model_part(root: &GffRoot, field: &str) -> Option<u32> {
    gff_u32(root.root.get_field(field).map(|entry| entry.value())).filter(|value| *value > 0)
}

fn player_part_model_exists(
    resman: &mut ResMan,
    family: &PlayerCreatureFamily,
    model_stem: &str,
    model_number: u32,
) -> bool {
    first_existing_model_candidate(
        resman,
        &[format!(
            "{}_{}{:03}",
            family.model_prefix,
            model_stem.to_ascii_lowercase(),
            model_number
        )],
    )
    .is_some()
}

fn equipped_player_part_overrides(
    equipped: &EquippedPaperdoll,
) -> std::collections::BTreeMap<String, EquippedPartVisual> {
    let mut overrides = std::collections::BTreeMap::new();
    if let Some(boots) = equipped.boots.as_ref() {
        if let Some(model_number) = boots.foot_model_number {
            let visual = EquippedPartVisual {
                model_number,
                appearance_overrides: boots.appearance_overrides.clone(),
            };
            overrides.insert("FOOTL".to_string(), visual.clone());
            overrides.insert("FOOTR".to_string(), visual);
        }
        if let Some(model_number) = boots.shin_model_number {
            let visual = EquippedPartVisual {
                model_number,
                appearance_overrides: boots.appearance_overrides.clone(),
            };
            overrides.insert("SHINL".to_string(), visual.clone());
            overrides.insert("SHINR".to_string(), visual);
        }
        if let Some(model_number) = boots.leg_model_number {
            let visual = EquippedPartVisual {
                model_number,
                appearance_overrides: boots.appearance_overrides.clone(),
            };
            overrides.insert("LEGL".to_string(), visual.clone());
            overrides.insert("LEGR".to_string(), visual);
        }
    }
    if let Some(bracers) = equipped.bracers.as_ref() {
        overrides.insert("FOREL".to_string(), bracers.clone());
        overrides.insert("FORER".to_string(), bracers.clone());
    }
    if let Some(left_ring) = equipped.left_ring.as_ref() {
        overrides.insert("HANDL".to_string(), left_ring.clone());
    }
    if let Some(right_ring) = equipped.right_ring.as_ref() {
        overrides.insert("HANDR".to_string(), right_ring.clone());
    }
    if let Some(gloves) = equipped.gloves.as_ref() {
        overrides.insert("HANDL".to_string(), gloves.clone());
        overrides.insert("HANDR".to_string(), gloves.clone());
    }
    overrides
}

fn creature_paperdoll_appearance_overrides(root: &GffRoot) -> NwnAppearanceOverrides {
    let mut overrides = NwnAppearanceOverrides::default();
    insert_plt_row_override(
        &mut overrides,
        0,
        &root.root,
        &["Color_Skin", "SkinColor", "ColorSkin"],
    );
    insert_plt_row_override(
        &mut overrides,
        1,
        &root.root,
        &["Color_Hair", "HairColor", "ColorHair"],
    );
    insert_plt_row_override(
        &mut overrides,
        8,
        &root.root,
        &["Color_Tattoo1", "Tattoo1Color", "ColorTattoo1"],
    );
    insert_plt_row_override(
        &mut overrides,
        9,
        &root.root,
        &["Color_Tattoo2", "Tattoo2Color", "ColorTattoo2"],
    );
    overrides
}

fn item_plt_appearance_overrides(value: &GffStruct) -> NwnAppearanceOverrides {
    let mut overrides = NwnAppearanceOverrides::default();
    insert_plt_row_override(&mut overrides, 2, value, &["Metal1Color"]);
    insert_plt_row_override(&mut overrides, 3, value, &["Metal2Color"]);
    insert_plt_row_override(&mut overrides, 4, value, &["Cloth1Color"]);
    insert_plt_row_override(&mut overrides, 5, value, &["Cloth2Color"]);
    insert_plt_row_override(&mut overrides, 6, value, &["Leather1Color"]);
    insert_plt_row_override(&mut overrides, 7, value, &["Leather2Color"]);
    overrides
}

fn insert_plt_row_override(
    overrides: &mut NwnAppearanceOverrides,
    layer_id: u8,
    value: &GffStruct,
    fields: &[&str],
) {
    if let Some(row) = fields
        .iter()
        .find_map(|field| gff_u8(value.get_field(field).map(|entry| entry.value())))
    {
        overrides.plt_rows.insert(layer_id, row);
    }
}

fn merged_appearance_overrides(
    base: &NwnAppearanceOverrides,
    extra: &NwnAppearanceOverrides,
) -> NwnAppearanceOverrides {
    let mut merged = base.clone();
    for (slot, value) in &extra.slots {
        merged.slots.insert(slot.clone(), value.clone());
    }
    for (layer_id, row) in &extra.plt_rows {
        merged.plt_rows.insert(*layer_id, *row);
    }
    merged
}

fn held_item_model_name(
    resman: &mut ResMan,
    root: &GffRoot,
    base_item_info: &BaseItemInfo,
) -> ModelResult<Option<String>> {
    let Some(model_part) = gff_u32(root.root.get_field("ModelPart1").map(|field| field.value()))
        .filter(|value| *value > 0)
    else {
        return Ok(None);
    };
    let Some(item_class) = base_item_info.item_class.as_deref() else {
        return Ok(None);
    };
    let model_candidates = match base_item_info.model_type {
        Some(1) => vec![format!("helm_{model_part:03}")],
        Some(2) => vec![
            format!("{item_class}_m_{model_part:03}"),
            format!("{item_class}_b_{model_part:03}"),
            format!("{item_class}_t_{model_part:03}"),
            format!("{item_class}_{model_part:03}"),
        ],
        Some(0) => vec![
            format!("{item_class}_{model_part:03}"),
            format!("{item_class}_m_{model_part:03}"),
            format!("{item_class}_b_{model_part:03}"),
            format!("{item_class}_t_{model_part:03}"),
        ],
        _ => Vec::new(),
    };

    Ok(first_existing_model_candidate(resman, &model_candidates))
}

fn first_existing_model_candidate(resman: &mut ResMan, candidates: &[String]) -> Option<String> {
    candidates.iter().find_map(|candidate| {
        ResRef::new(candidate.clone(), MODEL_RES_TYPE)
            .ok()
            .and_then(|resref| resman.get(&resref).map(|_res| candidate.clone()))
    })
}

fn load_gff_root_from_resman(
    resman: &mut ResMan,
    stem: &str,
    extension: &str,
) -> ModelResult<Option<GffRoot>> {
    let resolved = ResolvedResRef::from_filename(&format!("{stem}.{extension}"))
        .map_err(|error| ModelError::msg(format!("invalid resref {stem}.{extension}: {error}")))?;
    let Some(res) = resman.get_resolved(&resolved) else {
        return Ok(None);
    };
    Ok(Some(read_gff_root_from_res(&res)?))
}

fn read_gff_root_from_res(res: &Res) -> ModelResult<GffRoot> {
    let bytes = res
        .read_all(CachePolicy::Use)
        .map_err(|error| ModelError::msg(format!("read {}: {error}", res.resref())))?;
    read_gff_root(&mut Cursor::new(bytes))
        .map_err(|error| ModelError::msg(format!("parse {}: {error}", res.resref())))
}

fn appearance_model_name_from_named_twoda(
    resman: &mut ResMan,
    table_name: &str,
    appearance: usize,
    column: &str,
) -> ModelResult<Option<String>> {
    if appearance == 0 {
        return Ok(None);
    }
    let table = load_twoda(resman, table_name)?;
    let Some(row) = twoda_row_index_for_appearance(&table, appearance) else {
        return Ok(None);
    };
    Ok(table
        .cell(row, column)
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "****")
        .map(str::to_string))
}

fn creature_body_part_model_number(value: &GffStruct, fields: &[&str]) -> Option<u32> {
    fields
        .iter()
        .find_map(|field| gff_u32(value.get_field(field).map(|entry| entry.value())))
}

fn creature_body_part_field_aliases(model_stem: &str) -> &'static [&'static str] {
    match model_stem.to_ascii_uppercase().as_str() {
        "FOOTR" => &["BodyPart_RFoot", "BodyPart_Foot_R"],
        "FOOTL" => &["BodyPart_LFoot", "BodyPart_Foot_L"],
        "SHINR" => &["BodyPart_RShin", "BodyPart_Shin_R"],
        "SHINL" => &["BodyPart_LShin", "BodyPart_Shin_L"],
        "LEGR" => &["BodyPart_RThigh", "BodyPart_Thigh_R"],
        "LEGL" => &["BodyPart_LThigh", "BodyPart_Thigh_L"],
        "PELVIS" => &["BodyPart_Pelvis"],
        "CHEST" => &["BodyPart_Torso", "BodyPart_Chest"],
        "BELT" => &["BodyPart_Belt"],
        "NECK" => &["BodyPart_Neck"],
        "FORER" => &[
            "BodyPart_RForeArm",
            "BodyPart_RForearm",
            "BodyPart_ForeArm_R",
            "BodyPart_Forearm_R",
        ],
        "FOREL" => &[
            "BodyPart_LForeArm",
            "BodyPart_LForearm",
            "BodyPart_ForeArm_L",
            "BodyPart_Forearm_L",
        ],
        "BICEPR" => &["BodyPart_RBicep", "BodyPart_Bicep_R"],
        "BICEPL" => &["BodyPart_LBicep", "BodyPart_Bicep_L"],
        "SHOR" => &["BodyPart_RShoulder", "BodyPart_Shoulder_R"],
        "SHOL" => &["BodyPart_LShoulder", "BodyPart_Shoulder_L"],
        "HANDR" => &["BodyPart_RHand", "BodyPart_Hand_R"],
        "HANDL" => &["BodyPart_LHand", "BodyPart_Hand_L"],
        _ => &[],
    }
}

fn armor_part_field_aliases(model_stem: &str) -> &'static [&'static str] {
    match model_stem.to_ascii_uppercase().as_str() {
        "FOOTR" => &["ArmorPart_RFoot"],
        "FOOTL" => &["ArmorPart_LFoot"],
        "SHINR" => &["ArmorPart_RShin"],
        "SHINL" => &["ArmorPart_LShin"],
        "LEGR" => &["ArmorPart_RThigh"],
        "LEGL" => &["ArmorPart_LThigh"],
        "PELVIS" => &["ArmorPart_Pelvis"],
        "CHEST" => &["ArmorPart_Torso"],
        "BELT" => &["ArmorPart_Belt"],
        "NECK" => &["ArmorPart_Neck"],
        "FORER" => &["ArmorPart_RFArm", "ArmorPart_RForearm"],
        "FOREL" => &["ArmorPart_LFArm", "ArmorPart_LForearm"],
        "BICEPR" => &["ArmorPart_RBicep"],
        "BICEPL" => &["ArmorPart_LBicep"],
        "SHOR" => &["ArmorPart_RShoul", "ArmorPart_RShoulder"],
        "SHOL" => &["ArmorPart_LShoul", "ArmorPart_LShoulder"],
        "HANDR" => &["ArmorPart_RHand"],
        "HANDL" => &["ArmorPart_LHand"],
        _ => &[],
    }
}

fn racialtype_appearance_row(resman: &mut ResMan, race: usize) -> ModelResult<Option<usize>> {
    let table = load_twoda(resman, "racialtypes")?;
    let Some(row) = twoda_row_index_for_appearance(&table, race) else {
        return Ok(None);
    };
    Ok(table
        .cell(row, "Appearance")
        .and_then(|value| value.trim().parse::<usize>().ok()))
}

fn is_player_appearance_token(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() <= 2 && trimmed.chars().all(|ch| ch.is_ascii_alphabetic())
}

fn twoda_row_index_for_appearance(table: &TwoDa, appearance: usize) -> Option<usize> {
    (0..table.len())
        .find(|&row| {
            table
                .row_label(row)
                .and_then(|label| label.trim().parse::<usize>().ok())
                .is_some_and(|row_id| row_id == appearance)
        })
        .or_else(|| (appearance < table.len()).then_some(appearance))
}

fn load_twoda(resman: &mut ResMan, table_name: &str) -> ModelResult<TwoDa> {
    let resolved = ResolvedResRef::from_filename(&format!("{table_name}.2da"))
        .map_err(|error| ModelError::msg(format!("2da resref {table_name}.2da: {error}")))?;
    let res = resman
        .get_resolved(&resolved)
        .ok_or_else(|| ModelError::msg(format!("2da not found in ResMan: {table_name}.2da")))?;
    as_2da(&res).map_err(|error| ModelError::msg(format!("read {table_name}.2da: {error}")))
}

fn gff_u32(value: Option<&GffValue>) -> Option<u32> {
    match value? {
        GffValue::Byte(value) => Some(u32::from(*value)),
        GffValue::Word(value) => Some(u32::from(*value)),
        GffValue::Dword(value) => Some(*value),
        GffValue::Int(value) => u32::try_from(*value).ok(),
        _ => None,
    }
}

fn gff_u32_any(value: &GffStruct, fields: &[&str]) -> Option<u32> {
    fields
        .iter()
        .find_map(|field| gff_u32(value.get_field(field).map(|entry| entry.value())))
}

fn gff_string(value: Option<&GffValue>) -> Option<String> {
    match value? {
        GffValue::CExoString(value) | GffValue::ResRef(value) => Some(value.clone()),
        _ => None,
    }
}

fn gff_list(value: &GffValue) -> Option<&Vec<GffStruct>> {
    match value {
        GffValue::List(value) => Some(value),
        _ => None,
    }
}

fn gff_u8(value: Option<&GffValue>) -> Option<u8> {
    match value? {
        GffValue::Byte(value) => Some(*value),
        GffValue::Word(value) => u8::try_from(*value).ok(),
        GffValue::Dword(value) => u8::try_from(*value).ok(),
        GffValue::Int(value) => u8::try_from(*value).ok(),
        _ => None,
    }
}
