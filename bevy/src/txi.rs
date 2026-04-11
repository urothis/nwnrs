use bevy::math::{Affine2, Mat2, Mat3, Vec2, Vec3};
use nwnrs_mdl::prelude::{NwnCoordinateSystem, NwnMaterial, NwnPrimitive};
use nwnrs_txi::prelude::TxiFile;

use crate::{NwnModelTxiAsset, NwnModelTxiProcedureAsset, position_from_nwn};

pub(crate) fn build_model_txi_asset(
    material: &NwnMaterial,
    txi: &TxiFile,
) -> Option<NwnModelTxiAsset> {
    let procedure = txi
        .procedure_type
        .as_deref()
        .filter(|name| name.eq_ignore_ascii_case("arturo"))
        .map(|_| NwnModelTxiProcedureAsset::Arturo {
            channel_scale:        txi.channel_scale.clone().unwrap_or_default(),
            channel_translate:    txi.channel_translate.clone().unwrap_or_default(),
            distort:              txi.distort,
            arturo_width:         txi.arturo_width,
            arturo_height:        txi.arturo_height,
            distortion_amplitude: txi.distortion_amplitude,
            speed:                txi.speed,
            default_height:       txi.default_height,
            default_width:        txi.default_width,
            alpha_mean:           txi.alpha_mean,
        });

    if procedure.is_none()
        && material.rotate_texture == 0
        && txi.bump_map_texture.is_none()
        && txi.bumpy_shiny_texture.is_none()
    {
        return None;
    }

    Some(NwnModelTxiAsset {
        rotate_texture: material.rotate_texture,
        bump_map_texture: txi.bump_map_texture.clone(),
        bumpy_shiny_texture: txi.bumpy_shiny_texture.clone(),
        procedure,
    })
}

pub(crate) fn derive_txi_uv_to_local_horizontal(
    primitive: &NwnPrimitive,
    coordinate_system: NwnCoordinateSystem,
) -> Option<Affine2> {
    let uv_set = primitive.uv_sets.first()?;
    let mut normal_matrix = Mat3::ZERO;
    let mut x_rhs = Vec3::ZERO;
    let mut z_rhs = Vec3::ZERO;
    let mut sample_count = 0_u32;

    for face in &primitive.faces {
        let [v0, v1, v2] = face.vertex_indices;
        let [uv0, uv1, uv2] = face.uv_indices;
        for (vertex_index, uv_index) in [(v0, uv0), (v1, uv1), (v2, uv2)] {
            let position = primitive
                .positions
                .get(usize::try_from(vertex_index).ok()?)
                .copied()
                .map(|position| position_from_nwn(position, coordinate_system))?;
            let uv = uv_set
                .coordinates
                .get(usize::try_from(uv_index).ok()?)
                .copied()?;
            let sample = Vec3::new(uv[0], uv[1], 1.0);
            normal_matrix +=
                Mat3::from_cols(sample * sample.x, sample * sample.y, sample * sample.z);
            x_rhs += sample * position[0];
            z_rhs += sample * position[2];
            sample_count += 1;
        }
    }

    if sample_count >= 3 && normal_matrix.determinant().abs() > f32::EPSILON {
        let inverse = normal_matrix.inverse();
        let x_solution = inverse * x_rhs;
        let z_solution = inverse * z_rhs;
        let basis = Mat2::from_cols(
            Vec2::new(x_solution.x, z_solution.x),
            Vec2::new(x_solution.y, z_solution.y),
        );
        if basis.determinant().abs() > f32::EPSILON {
            return Some(Affine2::from_mat2_translation(
                basis,
                Vec2::new(x_solution.z, z_solution.z),
            ));
        }
    }

    derive_txi_uv_to_local_horizontal_from_face(primitive, uv_set, coordinate_system)
}

fn derive_txi_uv_to_local_horizontal_from_face(
    primitive: &NwnPrimitive,
    uv_set: &nwnrs_mdl::prelude::NwnUvSet,
    coordinate_system: NwnCoordinateSystem,
) -> Option<Affine2> {
    for face in &primitive.faces {
        let [v0, v1, v2] = face.vertex_indices;
        let [uv0, uv1, uv2] = face.uv_indices;
        let p0 = primitive
            .positions
            .get(usize::try_from(v0).ok()?)
            .copied()
            .map(|position| position_from_nwn(position, coordinate_system))?;
        let p1 = primitive
            .positions
            .get(usize::try_from(v1).ok()?)
            .copied()
            .map(|position| position_from_nwn(position, coordinate_system))?;
        let p2 = primitive
            .positions
            .get(usize::try_from(v2).ok()?)
            .copied()
            .map(|position| position_from_nwn(position, coordinate_system))?;
        let t0 = uv_set
            .coordinates
            .get(usize::try_from(uv0).ok()?)
            .copied()?;
        let t1 = uv_set
            .coordinates
            .get(usize::try_from(uv1).ok()?)
            .copied()?;
        let t2 = uv_set
            .coordinates
            .get(usize::try_from(uv2).ok()?)
            .copied()?;

        let p0 = Vec2::new(p0[0], p0[2]);
        let p1 = Vec2::new(p1[0], p1[2]);
        let p2 = Vec2::new(p2[0], p2[2]);
        let dp1 = p1 - p0;
        let dp2 = p2 - p0;
        let duv1 = Vec2::new(t1[0] - t0[0], t1[1] - t0[1]);
        let duv2 = Vec2::new(t2[0] - t0[0], t2[1] - t0[1]);
        let uv_matrix = Mat2::from_cols(duv1, duv2);
        if uv_matrix.determinant().abs() <= f32::EPSILON {
            continue;
        }
        let basis = Mat2::from_cols(dp1, dp2) * uv_matrix.inverse();
        if basis.determinant().abs() <= f32::EPSILON {
            continue;
        }
        let origin = p0 - basis * Vec2::new(t0[0], t0[1]);
        return Some(Affine2::from_mat2_translation(basis, origin));
    }

    None
}
