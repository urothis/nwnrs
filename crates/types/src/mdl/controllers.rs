//! Shared controller metadata used by every MDL representation layer.

/// Internal schema row for a controller whose binary identity is fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ControllerDefinition {
    name:      &'static str,
    binary_id: i32,
}

impl ControllerDefinition {
    const fn new(name: &'static str, binary_id: i32) -> Self {
        Self {
            name,
            binary_id,
        }
    }

    pub(crate) const fn name(self) -> &'static str {
        self.name
    }

    pub(crate) const fn binary_id(self) -> i32 {
        self.binary_id
    }
}

pub(crate) const POSITION_CONTROLLER: ControllerDefinition =
    ControllerDefinition::new("position", 8);
pub(crate) const ORIENTATION_CONTROLLER: ControllerDefinition =
    ControllerDefinition::new("orientation", 20);
pub(crate) const SCALE_CONTROLLER: ControllerDefinition = ControllerDefinition::new("scale", 36);

pub(crate) const SELF_ILLUM_COLOR_CONTROLLER: ControllerDefinition =
    ControllerDefinition::new("selfillumcolor", 100);
pub(crate) const ALPHA_CONTROLLER: ControllerDefinition = ControllerDefinition::new("alpha", 128);

pub(crate) const LIGHT_COLOR_CONTROLLER: ControllerDefinition =
    ControllerDefinition::new("color", 76);
pub(crate) const LIGHT_RADIUS_CONTROLLER: ControllerDefinition =
    ControllerDefinition::new("radius", 88);
pub(crate) const LIGHT_SHADOW_RADIUS_CONTROLLER: ControllerDefinition =
    ControllerDefinition::new("shadowradius", 96);
pub(crate) const LIGHT_VERTICAL_DISPLACEMENT_CONTROLLER: ControllerDefinition =
    ControllerDefinition::new("verticaldisplacement", 100);
pub(crate) const LIGHT_MULTIPLIER_CONTROLLER: ControllerDefinition =
    ControllerDefinition::new("multiplier", 140);

pub(crate) const TRANSFORM_CONTROLLER_DEFINITIONS: &[ControllerDefinition] = &[
    POSITION_CONTROLLER,
    ORIENTATION_CONTROLLER,
    SCALE_CONTROLLER,
];
pub(crate) const MESH_CONTROLLER_DEFINITIONS: &[ControllerDefinition] =
    &[SELF_ILLUM_COLOR_CONTROLLER, ALPHA_CONTROLLER];
pub(crate) const LIGHT_CONTROLLER_DEFINITIONS: &[ControllerDefinition] = &[
    LIGHT_COLOR_CONTROLLER,
    LIGHT_RADIUS_CONTROLLER,
    LIGHT_SHADOW_RADIUS_CONTROLLER,
    LIGHT_VERTICAL_DISPLACEMENT_CONTROLLER,
    LIGHT_MULTIPLIER_CONTROLLER,
];

pub(crate) fn controller_definition_by_binary_id(
    definitions: &'static [ControllerDefinition],
    type_id: i32,
) -> Option<&'static ControllerDefinition> {
    definitions
        .iter()
        .find(|definition| definition.binary_id() == type_id)
}

macro_rules! define_emitter_controllers {
    ($(
        $(#[$meta:meta])*
        $variant:ident => {
            name: $name:literal,
            aliases: [$($alias:literal),* $(,)?],
            bioware: $bioware:literal,
            nwnmdlcomp: $nwnmdlcomp:expr,
            width: $width:literal
        };
    )*) => {
        /// One known emitter animation controller.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub enum NwnEmitterController {
            $(
                $(#[$meta])*
                $variant,
            )*
        }

        impl NwnEmitterController {
            /// Returns the canonical ASCII property name without the `key` suffix.
            #[must_use]
            pub const fn property_name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)*
                }
            }
        }

        pub(crate) const EMITTER_CONTROLLER_DEFINITIONS: &[EmitterControllerDefinition] = &[
            $(
                EmitterControllerDefinition {
                    controller: NwnEmitterController::$variant,
                    aliases: &[$($alias),*],
                    bioware_id: $bioware,
                    nwnmdlcomp_id: $nwnmdlcomp,
                    value_width: $width,
                },
            )*
        ];

        pub(crate) const fn emitter_controller_definition_for(
            controller: NwnEmitterController,
        ) -> &'static EmitterControllerDefinition {
            match controller {
                $(
                    NwnEmitterController::$variant => &EmitterControllerDefinition {
                        controller: NwnEmitterController::$variant,
                        aliases: &[$($alias),*],
                        bioware_id: $bioware,
                        nwnmdlcomp_id: $nwnmdlcomp,
                        value_width: $width,
                    },
                )*
            }
        }
    };
}

/// Internal schema row for one canonical emitter controller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct EmitterControllerDefinition {
    pub(crate) controller:  NwnEmitterController,
    aliases:                &'static [&'static str],
    bioware_id:             i32,
    nwnmdlcomp_id:          Option<i32>,
    pub(crate) value_width: usize,
}

impl EmitterControllerDefinition {
    pub(crate) const fn name(self) -> &'static str {
        self.controller.property_name()
    }

    pub(crate) const fn binary_id(self, nwnmdlcomp: bool) -> i32 {
        if nwnmdlcomp {
            match self.nwnmdlcomp_id {
                Some(id) => id,
                None => self.bioware_id,
            }
        } else {
            self.bioware_id
        }
    }

    fn matches_name(self, name: &str) -> bool {
        self.name().eq_ignore_ascii_case(name)
            || self
                .aliases
                .iter()
                .any(|alias| alias.eq_ignore_ascii_case(name))
    }

    fn matches_any_binary_id(self, type_id: i32) -> bool {
        self.bioware_id == type_id || self.nwnmdlcomp_id == Some(type_id)
    }
}

define_emitter_controllers! {
    /// Particle spawn rate.
    Birthrate => { name: "birthrate", aliases: [], bioware: 88, nwnmdlcomp: None, width: 1 };
    /// Base particle velocity.
    Velocity => { name: "velocity", aliases: [], bioware: 192, nwnmdlcomp: None, width: 1 };
    /// Random velocity contribution.
    RandomVelocity => { name: "randvel", aliases: [], bioware: 164, nwnmdlcomp: None, width: 1 };
    /// Emission cone spread.
    Spread => { name: "spread", aliases: [], bioware: 184, nwnmdlcomp: None, width: 1 };
    /// Gravity strength.
    Gravity => { name: "grav", aliases: [], bioware: 140, nwnmdlcomp: None, width: 1 };
    /// Drag coefficient.
    Drag => { name: "drag", aliases: [], bioware: 124, nwnmdlcomp: None, width: 1 };
    /// Sprite animation frame rate.
    FramesPerSecond => { name: "fps", aliases: [], bioware: 128, nwnmdlcomp: None, width: 1 };
    /// Particle mass.
    Mass => { name: "mass", aliases: [], bioware: 148, nwnmdlcomp: None, width: 1 };
    /// Particle lifetime.
    LifeExpectancy => { name: "lifeexp", aliases: [], bioware: 144, nwnmdlcomp: None, width: 1 };
    /// Particle rotation rate.
    ParticleRotation => { name: "particlerot", aliases: [], bioware: 160, nwnmdlcomp: None, width: 1 };
    /// Initial alpha.
    AlphaStart => { name: "alphastart", aliases: [], bioware: 84, nwnmdlcomp: None, width: 1 };
    /// Mid-life alpha.
    AlphaMid => { name: "alphamid", aliases: [], bioware: 448, nwnmdlcomp: Some(464), width: 1 };
    /// End-of-life alpha.
    AlphaEnd => { name: "alphaend", aliases: [], bioware: 80, nwnmdlcomp: None, width: 1 };
    /// Initial size.
    SizeStart => { name: "sizestart", aliases: [], bioware: 168, nwnmdlcomp: None, width: 1 };
    /// Mid-life size.
    SizeMid => { name: "sizemid", aliases: [], bioware: 468, nwnmdlcomp: Some(484), width: 1 };
    /// End-of-life size.
    SizeEnd => { name: "sizeend", aliases: [], bioware: 172, nwnmdlcomp: None, width: 1 };
    /// Initial color.
    ColorStart => { name: "colorstart", aliases: [], bioware: 108, nwnmdlcomp: None, width: 3 };
    /// Mid-life color.
    ColorMid => { name: "colormid", aliases: [], bioware: 452, nwnmdlcomp: Some(468), width: 3 };
    /// End-of-life color.
    ColorEnd => { name: "colorend", aliases: [], bioware: 96, nwnmdlcomp: None, width: 3 };
    /// Motion-blur trail length.
    BlurLength => { name: "blurlength", aliases: [], bioware: 204, nwnmdlcomp: None, width: 1 };
    /// Particle bounce coefficient.
    BounceCoefficient => { name: "bounce_co", aliases: ["bounceco"], bioware: 92, nwnmdlcomp: None, width: 1 };
    /// Particle-combination time.
    CombineTime => { name: "combinetime", aliases: [], bioware: 120, nwnmdlcomp: None, width: 1 };
    /// Detonation event controller.
    Detonate => { name: "detonate", aliases: [], bioware: 228, nwnmdlcomp: None, width: 1 };
    /// Sprite start frame.
    FrameStart => { name: "framestart", aliases: [], bioware: 136, nwnmdlcomp: None, width: 1 };
    /// Sprite end frame.
    FrameEnd => { name: "frameend", aliases: [], bioware: 132, nwnmdlcomp: None, width: 1 };
    /// Lightning delay.
    LightningDelay => { name: "lightningdelay", aliases: [], bioware: 208, nwnmdlcomp: None, width: 1 };
    /// Lightning radius.
    LightningRadius => { name: "lightningradius", aliases: [], bioware: 212, nwnmdlcomp: None, width: 1 };
    /// Lightning scale.
    LightningScale => { name: "lightningscale", aliases: [], bioware: 216, nwnmdlcomp: None, width: 1 };
    /// Lightning subdivision count.
    LightningSubdivision => { name: "lightningsubdiv", aliases: [], bioware: 220, nwnmdlcomp: None, width: 1 };
    /// Point-to-point Bezier control value 2.
    PointToPointBezier2 => { name: "p2p_bezier2", aliases: [], bioware: 152, nwnmdlcomp: None, width: 1 };
    /// Point-to-point Bezier control value 3.
    PointToPointBezier3 => { name: "p2p_bezier3", aliases: [], bioware: 156, nwnmdlcomp: None, width: 1 };
    /// Start-life percentage.
    PercentStart => { name: "percentstart", aliases: [], bioware: 464, nwnmdlcomp: Some(480), width: 1 };
    /// Mid-life percentage.
    PercentMid => { name: "percentmid", aliases: [], bioware: 465, nwnmdlcomp: Some(481), width: 1 };
    /// End-life percentage.
    PercentEnd => { name: "percentend", aliases: [], bioware: 466, nwnmdlcomp: Some(482), width: 1 };
    /// Initial Y size.
    SizeStartY => { name: "sizestart_y", aliases: [], bioware: 176, nwnmdlcomp: None, width: 1 };
    /// Mid-life Y size.
    SizeMidY => { name: "sizemid_y", aliases: [], bioware: 472, nwnmdlcomp: Some(488), width: 1 };
    /// End-life Y size.
    SizeEndY => { name: "sizeend_y", aliases: [], bioware: 180, nwnmdlcomp: None, width: 1 };
    /// Emission threshold.
    Threshold => { name: "threshold", aliases: [], bioware: 188, nwnmdlcomp: None, width: 1 };
    /// Emitter X size.
    XSize => { name: "xsize", aliases: [], bioware: 196, nwnmdlcomp: None, width: 1 };
    /// Emitter Y size.
    YSize => { name: "ysize", aliases: [], bioware: 200, nwnmdlcomp: None, width: 1 };
}

pub(crate) fn emitter_controller_definition(
    name: &str,
) -> Option<&'static EmitterControllerDefinition> {
    EMITTER_CONTROLLER_DEFINITIONS
        .iter()
        .find(|definition| definition.matches_name(name))
}

pub(crate) fn emitter_controller_definition_by_binary_id(
    type_id: i32,
) -> Option<&'static EmitterControllerDefinition> {
    EMITTER_CONTROLLER_DEFINITIONS
        .iter()
        .find(|definition| definition.matches_any_binary_id(type_id))
}

pub(crate) fn emitter_uses_nwnmdlcomp_ids(type_ids: impl IntoIterator<Item = i32>) -> bool {
    type_ids.into_iter().any(|type_id| {
        EMITTER_CONTROLLER_DEFINITIONS.iter().any(|definition| {
            definition.nwnmdlcomp_id == Some(type_id)
                && !EMITTER_CONTROLLER_DEFINITIONS
                    .iter()
                    .any(|other| other.bioware_id == type_id)
        })
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        EMITTER_CONTROLLER_DEFINITIONS, LIGHT_CONTROLLER_DEFINITIONS,
        LIGHT_VERTICAL_DISPLACEMENT_CONTROLLER, MESH_CONTROLLER_DEFINITIONS, NwnEmitterController,
        SELF_ILLUM_COLOR_CONTROLLER, TRANSFORM_CONTROLLER_DEFINITIONS,
        controller_definition_by_binary_id, emitter_controller_definition,
        emitter_controller_definition_for, emitter_uses_nwnmdlcomp_ids,
    };

    #[test]
    fn emitter_schema_is_complete_and_unambiguous_by_name() {
        let mut names = BTreeSet::new();
        for definition in EMITTER_CONTROLLER_DEFINITIONS {
            assert!(names.insert(definition.name()));
            assert_eq!(
                emitter_controller_definition(definition.name()),
                Some(definition)
            );
            assert_eq!(
                emitter_controller_definition_for(definition.controller),
                definition
            );
            assert!(matches!(definition.value_width, 1 | 3));
        }
        assert_eq!(
            emitter_controller_definition("bounceco").map(|definition| definition.controller),
            Some(NwnEmitterController::BounceCoefficient)
        );
    }

    #[test]
    fn compiler_variant_detection_uses_unambiguous_alternate_ids() {
        assert!(!emitter_uses_nwnmdlcomp_ids([464, 468]));
        assert!(emitter_uses_nwnmdlcomp_ids([480]));
        assert!(emitter_uses_nwnmdlcomp_ids([488]));
    }

    #[test]
    fn contextual_controller_schemas_are_unambiguous() {
        for definitions in [
            TRANSFORM_CONTROLLER_DEFINITIONS,
            MESH_CONTROLLER_DEFINITIONS,
            LIGHT_CONTROLLER_DEFINITIONS,
        ] {
            let mut names = BTreeSet::new();
            let mut ids = BTreeSet::new();
            for definition in definitions {
                assert!(names.insert(definition.name()));
                assert!(ids.insert(definition.binary_id()));
                assert_eq!(
                    controller_definition_by_binary_id(definitions, definition.binary_id()),
                    Some(definition)
                );
            }
        }
    }

    #[test]
    fn binary_ids_may_be_reused_by_different_node_contexts() {
        assert_eq!(
            SELF_ILLUM_COLOR_CONTROLLER.binary_id(),
            LIGHT_VERTICAL_DISPLACEMENT_CONTROLLER.binary_id()
        );
        assert_ne!(
            SELF_ILLUM_COLOR_CONTROLLER.name(),
            LIGHT_VERTICAL_DISPLACEMENT_CONTROLLER.name()
        );
    }
}
