use bevy::prelude::{Color, Component, PointLight, Query, Res, Time};
use nwnrs_mdl::prelude::*;

use crate::{NwnModelLightAnimationAsset, NwnModelLightAsset};

const NWN_POINT_LIGHT_INTENSITY_SCALE: f32 = 1600.0;

pub(crate) fn build_model_light_asset(
    scene: &NwnScene,
    node_index: usize,
) -> Option<NwnModelLightAsset> {
    let node = scene.nodes.get(node_index)?;
    let light = node.light.as_ref()?;
    if light.ambient_only != 0 || light.negative_light != 0 {
        return None;
    }

    Some(NwnModelLightAsset {
        base_color:     node.color.unwrap_or([1.0, 1.0, 1.0]),
        base_radius:    node.radius.unwrap_or(0.0),
        base_alpha:     node.alpha.unwrap_or(1.0),
        multiplier:     light.multiplier,
        // Point-light shadows are far too expensive to enable on every NWN-authored
        // torch/lamp in dense areas. Keep local model lights unshadowed until we add
        // an explicit shadow budget / prioritization policy.
        shadow_enabled: false,
        animation:      select_light_animation(scene, node_index, node.name.as_str()),
    })
}

pub(crate) fn point_light_from_nwn(light: &NwnModelLightAsset) -> PointLight {
    point_light_from_values(
        light.base_color,
        light.base_radius,
        light.base_alpha,
        light.multiplier,
        light.shadow_enabled,
    )
}

pub(crate) fn animated_light_component(light: &NwnModelLightAsset) -> Option<NwnAnimatedLight> {
    light
        .animation
        .clone()
        .filter(|animation| animation.length > 0.0)
        .map(|animation| NwnAnimatedLight {
            base_color: light.base_color,
            base_radius: light.base_radius,
            base_alpha: light.base_alpha,
            multiplier: light.multiplier,
            shadow_enabled: light.shadow_enabled,
            animation,
        })
}

pub(crate) fn animate_nwn_model_lights(
    time: Res<'_, Time>,
    mut lights: Query<'_, '_, (&NwnAnimatedLight, &mut PointLight)>,
) {
    let elapsed = time.elapsed_secs();
    for (animated, mut point_light) in &mut lights {
        apply_animated_light(animated, &mut point_light, elapsed);
    }
}

#[derive(Component, Debug, Clone)]
pub(crate) struct NwnAnimatedLight {
    base_color:     [f32; 3],
    base_radius:    f32,
    base_alpha:     f32,
    multiplier:     f32,
    shadow_enabled: bool,
    animation:      NwnModelLightAnimationAsset,
}

fn select_light_animation(
    scene: &NwnScene,
    node_index: usize,
    node_name: &str,
) -> Option<NwnModelLightAnimationAsset> {
    if let Some(default_track) = scene
        .animations
        .iter()
        .find(|animation| animation.name.eq_ignore_ascii_case("default"))
        .and_then(|animation| {
            animation
                .node_tracks
                .iter()
                .find(|track| light_track_matches(track, node_index, node_name))
                .map(|track| (animation.length, track))
        })
    {
        return Some(light_animation_from_track(default_track.0, default_track.1));
    }

    let mut matches = scene
        .animations
        .iter()
        .filter_map(|animation| {
            animation
                .node_tracks
                .iter()
                .find(|track| light_track_matches(track, node_index, node_name))
                .map(|track| (animation.length, track))
        })
        .collect::<Vec<_>>();

    if matches.len() == 1 {
        let (length, track) = matches.remove(0);
        Some(light_animation_from_track(length, track))
    } else {
        None
    }
}

fn light_track_matches(track: &NwnNodeAnimationTrack, node_index: usize, node_name: &str) -> bool {
    track.target_node == Some(node_index)
        || (track.target_node.is_none() && track.target_name.eq_ignore_ascii_case(node_name))
}

fn light_animation_from_track(
    length: f32,
    track: &NwnNodeAnimationTrack,
) -> NwnModelLightAnimationAsset {
    NwnModelLightAnimationAsset {
        length,
        color_keys: track.material.color_keys.clone(),
        radius_keys: track.material.radius_keys.clone(),
        alpha_keys: track.material.alpha_keys.clone(),
    }
}

fn apply_animated_light(animated: &NwnAnimatedLight, point_light: &mut PointLight, elapsed: f32) {
    let sample_time = if animated.animation.length > 0.0 {
        elapsed.rem_euclid(animated.animation.length)
    } else {
        0.0
    };
    let color = sample_vec3_keys(
        &animated.animation.color_keys,
        sample_time,
        animated.base_color,
    );
    let radius = sample_scalar_keys(
        &animated.animation.radius_keys,
        sample_time,
        animated.base_radius,
    );
    let alpha = sample_scalar_keys(
        &animated.animation.alpha_keys,
        sample_time,
        animated.base_alpha,
    );

    *point_light = point_light_from_values(
        color,
        radius,
        alpha,
        animated.multiplier,
        animated.shadow_enabled,
    );
}

fn point_light_from_values(
    color: [f32; 3],
    radius: f32,
    alpha: f32,
    multiplier: f32,
    shadow_enabled: bool,
) -> PointLight {
    PointLight {
        color: Color::srgb(color[0], color[1], color[2]),
        intensity: NWN_POINT_LIGHT_INTENSITY_SCALE * multiplier.max(0.0) * alpha.clamp(0.0, 1.0),
        range: radius.max(0.1),
        radius: 0.0,
        shadows_enabled: shadow_enabled,
        ..Default::default()
    }
}

fn sample_scalar_keys(keys: &[ScalarKey], time: f32, fallback: f32) -> f32 {
    let Some(first) = keys.first() else {
        return fallback;
    };
    if keys.len() == 1 || time <= first.time {
        return first.value;
    }
    let last = keys.last().unwrap_or(first);
    if time >= last.time {
        return last.value;
    }

    for window in keys.windows(2) {
        let [start, end] = window else {
            continue;
        };
        if time <= end.time {
            let duration = (end.time - start.time).max(f32::EPSILON);
            let factor = ((time - start.time) / duration).clamp(0.0, 1.0);
            return start.value + (end.value - start.value) * factor;
        }
    }

    last.value
}

fn sample_vec3_keys(keys: &[Vec3Key], time: f32, fallback: [f32; 3]) -> [f32; 3] {
    let Some(first) = keys.first() else {
        return fallback;
    };
    if keys.len() == 1 || time <= first.time {
        return first.value;
    }
    let last = keys.last().unwrap_or(first);
    if time >= last.time {
        return last.value;
    }

    for window in keys.windows(2) {
        let [start, end] = window else {
            continue;
        };
        if time <= end.time {
            let duration = (end.time - start.time).max(f32::EPSILON);
            let factor = ((time - start.time) / duration).clamp(0.0, 1.0);
            return [
                start.value[0] + (end.value[0] - start.value[0]) * factor,
                start.value[1] + (end.value[1] - start.value[1]) * factor,
                start.value[2] + (end.value[2] - start.value[2]) * factor,
            ];
        }
    }

    last.value
}

#[cfg(test)]
mod tests {
    use bevy::prelude::PointLight;
    use nwnrs_mdl::prelude::{
        AnimationEvent, ModelClassification, NodeKind, NwnAnimation, NwnCoordinateSystem, NwnLight,
        NwnMaterialTrack, NwnNodeAnimationTrack, NwnScene, NwnSceneNode, NwnTransform,
        NwnTransformTrack, ScalarKey, Vec3Key,
    };

    use super::{apply_animated_light, build_model_light_asset};
    use crate::NwnModelLightAnimationAsset;

    #[test]
    fn build_model_light_asset_prefers_default_animation() {
        let scene = scene_with_animations(vec![
            animation("idle", 1.0, vec![light_track("Torch", Some(0), 0.2)]),
            animation("default", 2.0, vec![light_track("Torch", Some(0), 0.8)]),
        ]);

        let light = build_model_light_asset(&scene, 0).unwrap_or_else(|| panic!("light asset"));
        assert!(!light.shadow_enabled);
        assert_eq!(
            light.animation.as_ref().map(|animation| animation.length),
            Some(2.0)
        );
        assert_eq!(
            light
                .animation
                .as_ref()
                .and_then(|animation| animation.alpha_keys.last())
                .map(|key| key.value),
            Some(0.8)
        );
    }

    #[test]
    fn build_model_light_asset_uses_only_matching_track() {
        let scene = scene_with_animations(vec![animation(
            "torch",
            1.5,
            vec![light_track("Torch", Some(0), 0.7)],
        )]);

        let light = build_model_light_asset(&scene, 0).unwrap_or_else(|| panic!("light asset"));
        assert!(!light.shadow_enabled);
        assert_eq!(
            light.animation.as_ref().map(|animation| animation.length),
            Some(1.5)
        );
    }

    #[test]
    fn build_model_light_asset_skips_ambiguous_tracks_without_default() {
        let scene = scene_with_animations(vec![
            animation("idle", 1.0, vec![light_track("Torch", Some(0), 0.2)]),
            animation("walk", 1.0, vec![light_track("Torch", Some(0), 0.8)]),
        ]);

        let light = build_model_light_asset(&scene, 0).unwrap_or_else(|| panic!("light asset"));
        assert!(!light.shadow_enabled);
        assert!(light.animation.is_none());
    }

    #[test]
    fn animated_light_loops_color_radius_and_alpha() {
        let animated = super::NwnAnimatedLight {
            base_color:     [1.0, 0.0, 0.0],
            base_radius:    2.0,
            base_alpha:     0.25,
            multiplier:     1.0,
            shadow_enabled: true,
            animation:      NwnModelLightAnimationAsset {
                length:      1.0,
                color_keys:  vec![
                    Vec3Key {
                        time:  0.0,
                        value: [1.0, 0.0, 0.0],
                    },
                    Vec3Key {
                        time:  1.0,
                        value: [0.0, 0.0, 1.0],
                    },
                ],
                radius_keys: vec![
                    ScalarKey {
                        time:  0.0,
                        value: 2.0,
                    },
                    ScalarKey {
                        time:  1.0,
                        value: 6.0,
                    },
                ],
                alpha_keys:  vec![
                    ScalarKey {
                        time:  0.0,
                        value: 0.25,
                    },
                    ScalarKey {
                        time:  1.0,
                        value: 0.75,
                    },
                ],
            },
        };
        let mut point_light = PointLight::default();

        apply_animated_light(&animated, &mut point_light, 1.5);

        let color = point_light.color.to_srgba();
        assert!((color.red - 0.5).abs() < 0.001);
        assert!((color.blue - 0.5).abs() < 0.001);
        assert!((point_light.range - 4.0).abs() < 0.001);
        assert!((point_light.intensity - 800.0).abs() < 0.001);
        assert!(point_light.shadows_enabled);
    }

    fn scene_with_animations(animations: Vec<NwnAnimation>) -> NwnScene {
        NwnScene {
            name: "torch".to_string(),
            supermodel: None,
            classification: Some(ModelClassification::Item),
            animation_scale: Some(1.0),
            coordinate_system: NwnCoordinateSystem::AuroraSource,
            nodes: vec![NwnSceneNode {
                kind:            NodeKind::Light,
                node_type:       "light".to_string(),
                name:            "Torch".to_string(),
                parent:          None,
                part_number:     None,
                local_transform: NwnTransform {
                    translation:         [0.0, 0.0, 0.0],
                    rotation_axis_angle: [0.0, 0.0, 0.0, 0.0],
                    scale:               [1.0, 1.0, 1.0],
                },
                center:          None,
                color:           Some([1.0, 0.6, 0.2]),
                radius:          Some(3.0),
                alpha:           Some(0.5),
                wirecolor:       None,
                light:           Some(NwnLight {
                    multiplier:         1.0,
                    ambient_only:       0,
                    n_dynamic_type:     None,
                    is_dynamic:         0,
                    affect_dynamic:     1,
                    negative_light:     0,
                    light_priority:     3,
                    fading_light:       1,
                    lens_flares:        0,
                    flare_radius:       0.0,
                    flare_textures:     Vec::new(),
                    flare_sizes:        Vec::new(),
                    flare_positions:    Vec::new(),
                    flare_color_shifts: Vec::new(),
                }),
                emitter:         None,
                reference:       None,
                mesh:            None,
            }],
            meshes: Vec::new(),
            materials: Vec::new(),
            animations,
            diagnostics: Vec::new(),
        }
    }

    fn animation(name: &str, length: f32, tracks: Vec<NwnNodeAnimationTrack>) -> NwnAnimation {
        NwnAnimation {
            name: name.to_string(),
            model_name: "torch".to_string(),
            length,
            transition_time: 0.0,
            root_name: None,
            root_node: None,
            events: Vec::<AnimationEvent>::new(),
            node_tracks: tracks,
        }
    }

    fn light_track(
        target_name: &str,
        target_node: Option<usize>,
        alpha_end: f32,
    ) -> NwnNodeAnimationTrack {
        NwnNodeAnimationTrack {
            target_name: target_name.to_string(),
            target_node,
            kind: NodeKind::Light,
            transform: NwnTransformTrack {
                translation_keys:         Vec::new(),
                rotation_axis_angle_keys: Vec::new(),
                scale_keys:               Vec::new(),
            },
            material: NwnMaterialTrack {
                color_keys:            vec![Vec3Key {
                    time:  0.0,
                    value: [1.0, 0.6, 0.2],
                }],
                radius_keys:           vec![ScalarKey {
                    time:  0.0,
                    value: 3.0,
                }],
                alpha_keys:            vec![
                    ScalarKey {
                        time:  0.0,
                        value: 0.5,
                    },
                    ScalarKey {
                        time:  1.0,
                        value: alpha_end,
                    },
                ],
                self_illum_color_keys: Vec::new(),
            },
            animmesh: None,
        }
    }
}
