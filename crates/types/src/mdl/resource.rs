use nwnrs_types::resman::ResType;

use crate::mdl::{
    MODEL_RES_TYPE, ModelEncoding, ModelError, ModelResult, NwnScene, detect_model_encoding,
    parse_scene_model_auto,
};

/// NWN resource type id for tile walkmeshes (`WOK`).
pub const WALKMESH_RES_TYPE: ResType = ResType(2016);
/// NWN resource type id for door walkmeshes (`DWK`).
pub const DOOR_WALKMESH_RES_TYPE: ResType = ResType(2052);
/// NWN resource type id for placeable walkmeshes (`PWK`).
pub const PLACEABLE_WALKMESH_RES_TYPE: ResType = ResType(2053);

/// A model-shaped resource that can be lowered into an [`NwnScene`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelResourceKind {
    /// A normal render model (`MDL`).
    Model,
    /// A tile walkmesh (`WOK`).
    Walkmesh,
    /// A door walkmesh (`DWK`).
    DoorWalkmesh,
    /// A placeable walkmesh (`PWK`).
    PlaceableWalkmesh,
}

impl ModelResourceKind {
    /// Returns the registered NWN resource type.
    #[must_use]
    pub const fn res_type(self) -> ResType {
        match self {
            Self::Model => MODEL_RES_TYPE,
            Self::Walkmesh => WALKMESH_RES_TYPE,
            Self::DoorWalkmesh => DOOR_WALKMESH_RES_TYPE,
            Self::PlaceableWalkmesh => PLACEABLE_WALKMESH_RES_TYPE,
        }
    }

    /// Resolves a registered NWN resource type into a model-shaped kind.
    #[must_use]
    pub const fn from_res_type(res_type: ResType) -> Option<Self> {
        match res_type {
            MODEL_RES_TYPE => Some(Self::Model),
            WALKMESH_RES_TYPE => Some(Self::Walkmesh),
            DOOR_WALKMESH_RES_TYPE => Some(Self::DoorWalkmesh),
            PLACEABLE_WALKMESH_RES_TYPE => Some(Self::PlaceableWalkmesh),
            _ => None,
        }
    }

    /// Returns the conventional file extension for this kind.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Model => "mdl",
            Self::Walkmesh => "wok",
            Self::DoorWalkmesh => "dwk",
            Self::PlaceableWalkmesh => "pwk",
        }
    }
}

/// Parses an MDL, WOK, DWK, or PWK payload into the renderer-neutral scene.
///
/// Compiled walkmeshes share the compiled MDL container and are parsed
/// directly. ASCII walkmeshes are valid model fragments rather than complete
/// MDL documents, so this function supplies only the structural model envelope
/// required by the regular semantic parser. The authored fragment remains
/// otherwise unchanged.
///
/// # Errors
///
/// Returns [`ModelError`] when the resource text is malformed, is not UTF-8,
/// or cannot be lowered as a model scene.
pub fn parse_scene_resource_auto(
    kind: ModelResourceKind,
    resource_name: &str,
    bytes: &[u8],
) -> ModelResult<NwnScene> {
    if kind == ModelResourceKind::Model || detect_model_encoding(bytes) == ModelEncoding::Compiled {
        return parse_scene_model_auto(bytes);
    }

    let text = std::str::from_utf8(bytes).map_err(|error| {
        ModelError::msg(format!("ASCII {} is not UTF-8: {error}", kind.extension()))
    })?;
    let wrapped = match kind {
        ModelResourceKind::Model => text.to_string(),
        ModelResourceKind::Walkmesh => wrap_tile_walkmesh(text, resource_name)?,
        ModelResourceKind::DoorWalkmesh | ModelResourceKind::PlaceableWalkmesh => {
            wrap_object_walkmesh(text, resource_name, kind)
        }
    };
    parse_scene_model_auto(wrapped.as_bytes())
}

fn wrap_tile_walkmesh(text: &str, resource_name: &str) -> ModelResult<String> {
    let authored_name = statement_argument(text, "beginwalkmeshgeom", 0)
        .ok_or_else(|| ModelError::msg("ASCII WOK is missing beginwalkmeshgeom"))?;
    if !has_statement(text, "endwalkmeshgeom") {
        return Err(ModelError::msg("ASCII WOK is missing endwalkmeshgeom"));
    }
    let model_name = valid_model_name(&authored_name, resource_name, "walkmesh");
    Ok(format!(
        "newmodel {model_name}\nsetsupermodel {model_name} null\nclassification \
         tile\nsetanimationscale 1\nbeginmodelgeom {model_name}\nnode dummy {model_name}\n  \
         parent null\nendnode\n{text}\nendmodelgeom {model_name}\ndonemodel {model_name}\n"
    ))
}

fn wrap_object_walkmesh(text: &str, resource_name: &str, kind: ModelResourceKind) -> String {
    let model_name = valid_model_name(resource_name, resource_name, "walkmesh");
    let attachment_root = statement_argument(text, "parent", 0)
        .filter(|name| !name.eq_ignore_ascii_case("null"))
        .unwrap_or_else(|| format!("{model_name}_root"));
    let classification = match kind {
        ModelResourceKind::DoorWalkmesh => "door",
        ModelResourceKind::PlaceableWalkmesh => "character",
        ModelResourceKind::Model | ModelResourceKind::Walkmesh => "character",
    };
    format!(
        "newmodel {model_name}\nsetsupermodel {model_name} null\nclassification \
         {classification}\nsetanimationscale 1\nbeginmodelgeom {model_name}\nnode dummy \
         {model_name}\n  parent null\nendnode\nnode dummy {attachment_root}\n  parent \
         {model_name}\nendnode\n{text}\nendmodelgeom {model_name}\ndonemodel {model_name}\n"
    )
}

fn statement_argument(text: &str, keyword: &str, index: usize) -> Option<String> {
    text.lines().find_map(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            return None;
        }
        let mut tokens = trimmed.split_whitespace();
        let found = tokens.next()?;
        if !found.eq_ignore_ascii_case(keyword) {
            return None;
        }
        tokens.nth(index).map(str::to_string)
    })
}

fn has_statement(text: &str, keyword: &str) -> bool {
    text.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.starts_with('#')
            && !trimmed.starts_with("//")
            && trimmed
                .split_whitespace()
                .next()
                .is_some_and(|token| token.eq_ignore_ascii_case(keyword))
    })
}

fn valid_model_name(preferred: &str, fallback: &str, final_fallback: &str) -> String {
    [preferred, fallback, final_fallback]
        .into_iter()
        .map(str::trim)
        .find(|candidate| {
            !candidate.is_empty()
                && candidate
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || character == '_')
        })
        .unwrap_or(final_fallback)
        .to_string()
}

#[cfg(test)]
mod tests {
    use crate::mdl::{ModelResourceKind, NodeKind, parse_scene_resource_auto};

    #[test]
    fn parses_ascii_tile_walkmesh_fragment() {
        let scene = parse_scene_resource_auto(
            ModelResourceKind::Walkmesh,
            "tno01_w01_01",
            br#"beginwalkmeshgeom tno01_w01_01
node aabb walkmesh
  parent tno01_w01_01
  verts 3
    0 0 0
    1 0 0
    0 1 0
  faces 1
    0 1 2 0 0 1 2 3
endnode
endwalkmeshgeom tno01_w01_01
"#,
        )
        .unwrap_or_else(|error| panic!("parse WOK fragment: {error}"));

        assert_eq!(scene.name, "tno01_w01_01");
        assert!(scene.nodes.iter().any(|node| node.kind == NodeKind::Aabb));
    }

    #[test]
    fn parses_ascii_placeable_walkmesh_fragment() {
        let scene = parse_scene_resource_auto(
            ModelResourceKind::PlaceableWalkmesh,
            "plc_chair01",
            br#"node dummy collision
  parent base
endnode
node trimesh use01
  parent collision
  verts 3
    0 0 0
    1 0 0
    0 1 0
  faces 1
    0 1 2 0 0 1 2 0
endnode
"#,
        )
        .unwrap_or_else(|error| panic!("parse PWK fragment: {error}"));

        assert_eq!(scene.name, "plc_chair01");
        assert!(scene.nodes.iter().any(|node| node.name == "base"));
        assert!(scene.nodes.iter().any(|node| node.name == "use01"));
    }

    #[test]
    fn rejects_incomplete_ascii_tile_walkmesh() {
        let result = parse_scene_resource_auto(
            ModelResourceKind::Walkmesh,
            "broken",
            b"beginwalkmeshgeom broken\n",
        );
        assert!(result.is_err());
    }
}
