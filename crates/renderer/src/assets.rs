use std::collections::{BTreeMap, BTreeSet};

use nwnrs_types::{
    dds::{DdsFormat, DdsTexture},
    mdl::{
        NwnAppearanceOverrides, NwnComposedScene, NwnMaterialTextureRole, NwnMaterialTextureSource,
        NwnPropertyValue, NwnScene, NwnTextureRef, NwnTextureSlot, TextureResolverOptions,
        TextureResourceKind, resolve_scene_materials, resolve_texture_ref,
    },
    plt::{PltLayer, PltTexture},
    resman::{CachePolicy, Res, ResMan, ResolvedResRef},
    tga::TgaTexture,
};

use crate::{
    DependencyGraph, DependencyKind, DependencyState, ModelScene, RenderCompressedTexture,
    RenderDiagnostic, RenderDiagnosticSeverity, RenderMaterialAssets, RenderMaterialTexture,
    RenderModelAssets, RenderMtr, RenderMtrParameter, RenderNodeTexture, RenderShaderSource,
    RenderShaderStage, RenderTexture, RenderTextureCompression, RenderTextureKind,
    RenderTextureMip, RenderTxiDirective, RendererError, RendererResult,
};

/// Resolves and decodes the complete visual asset tree for a model collection.
pub(crate) fn resolve_model_assets(
    resman: &mut ResMan,
    models: &[ModelScene],
    dependencies: &mut DependencyGraph,
    diagnostics: &mut Vec<RenderDiagnostic>,
) -> RendererResult<(
    Vec<RenderModelAssets>,
    Vec<RenderTexture>,
    Vec<RenderShaderSource>,
)> {
    let mut context = AssetContext {
        resman,
        dependencies,
        diagnostics,
        textures: Vec::new(),
        texture_indices: BTreeMap::new(),
        shaders: Vec::new(),
        shader_indices: BTreeSet::new(),
    };
    let assets = models
        .iter()
        .map(|model| match model {
            ModelScene::Composed(composed) => context.resolve_composed(composed),
            ModelScene::Auxiliary(scene) => {
                context.resolve_scene(scene, &NwnAppearanceOverrides::default(), Vec::new())
            }
        })
        .collect::<RendererResult<Vec<_>>>()?;
    Ok((assets, context.textures, context.shaders))
}

struct AssetContext<'a> {
    resman:          &'a mut ResMan,
    dependencies:    &'a mut DependencyGraph,
    diagnostics:     &'a mut Vec<RenderDiagnostic>,
    textures:        Vec<RenderTexture>,
    texture_indices: BTreeMap<String, usize>,
    shaders:         Vec<RenderShaderSource>,
    shader_indices:  BTreeSet<(String, RenderShaderStage)>,
}

impl AssetContext<'_> {
    fn resolve_composed(
        &mut self,
        composed: &NwnComposedScene,
    ) -> RendererResult<RenderModelAssets> {
        let attachments = composed
            .attachments
            .iter()
            .map(|attachment| self.resolve_composed(&attachment.scene))
            .collect::<RendererResult<Vec<_>>>()?;
        self.resolve_scene(&composed.scene, &composed.appearance_overrides, attachments)
    }

    fn resolve_scene(
        &mut self,
        scene: &NwnScene,
        appearance: &NwnAppearanceOverrides,
        attachments: Vec<RenderModelAssets>,
    ) -> RendererResult<RenderModelAssets> {
        let model_resource = format!("{}.mdl", scene.name);
        let model_id = self
            .dependencies
            .id_for(&model_resource)
            .unwrap_or_else(|| {
                record_resolved(
                    self.resman,
                    self.dependencies,
                    &model_resource,
                    DependencyKind::Model,
                )
            });
        let resolved =
            resolve_scene_materials(scene, self.resman, &TextureResolverOptions::default())
                .map_err(|error| {
                    RendererError::scene(format!("resolve materials for {}: {error}", scene.name))
                })?;
        let mut materials = Vec::with_capacity(resolved.len());
        for material in resolved {
            let mtr = if let Some(mtr) = material.mtr {
                let mtr_id = self.dependencies.record(
                    mtr.resolved.to_string(),
                    DependencyKind::Material,
                    DependencyState::Resolved,
                    Some(mtr.resource.origin().to_string()),
                    None,
                );
                self.dependencies.connect(model_id, mtr_id, "material");
                let vertex_shader = self.resolve_shader(
                    mtr_id,
                    mtr.material.custom_shader_vs.as_deref(),
                    RenderShaderStage::Vertex,
                );
                let geometry_shader = self.resolve_shader(
                    mtr_id,
                    mtr.material.custom_shader_gs.as_deref(),
                    RenderShaderStage::Geometry,
                );
                let fragment_shader = self.resolve_shader(
                    mtr_id,
                    mtr.material.custom_shader_fs.as_deref(),
                    RenderShaderStage::Fragment,
                );
                Some(RenderMtr {
                    resource: mtr.resolved.to_string(),
                    render_hint: mtr.material.render_hint.clone(),
                    parameters: mtr
                        .material
                        .parameters
                        .iter()
                        .map(|(name, parameter)| RenderMtrParameter {
                            name:       name.clone(),
                            param_type: parameter.param_type.clone(),
                            values:     parameter.values.clone(),
                        })
                        .collect(),
                    vertex_shader,
                    geometry_shader,
                    fragment_shader,
                })
            } else {
                if let Some(missing) = material.missing_mtr {
                    let id = self.dependencies.record(
                        missing.to_string(),
                        DependencyKind::Material,
                        DependencyState::Missing,
                        None,
                        Some("referenced MTR was not found".into()),
                    );
                    self.dependencies.connect(model_id, id, "material");
                    self.diagnostics.push(RenderDiagnostic {
                        severity: RenderDiagnosticSeverity::Error,
                        code:     "material.mtr.missing".into(),
                        message:  format!("{} references missing {missing}", scene.name),
                        resource: Some(missing.to_string()),
                    });
                }
                None
            };

            let mut textures = Vec::with_capacity(material.slots.len());
            for slot in material.slots {
                let role = role_name(slot.role);
                let relationship = format!("texture:{role}");
                let texture_index = if let Some(texture) = slot.resolved {
                    let texture_id = self.dependencies.record(
                        texture.resolved.to_string(),
                        DependencyKind::Texture,
                        DependencyState::Resolved,
                        Some(texture.resource.origin().to_string()),
                        None,
                    );
                    self.dependencies
                        .connect(model_id, texture_id, relationship.clone());
                    match self.decode_texture(&texture.resource, texture.kind, appearance) {
                        Ok(index) => Some(index),
                        Err(error) => {
                            if let Some(node) = self.dependencies.nodes.get_mut(texture_id) {
                                node.state = DependencyState::Invalid;
                                node.message = Some(error.to_string());
                            }
                            self.diagnostics.push(RenderDiagnostic {
                                severity: RenderDiagnosticSeverity::Error,
                                code:     "texture.decode.failed".into(),
                                message:  format!(
                                    "failed to decode {} for {}: {error}",
                                    texture.resolved, scene.name
                                ),
                                resource: Some(texture.resolved.to_string()),
                            });
                            None
                        }
                    }
                } else {
                    if let Some(missing) = slot.missing {
                        for attempted in &missing.attempted {
                            let id = self.dependencies.record(
                                attempted.to_string(),
                                DependencyKind::Texture,
                                DependencyState::Missing,
                                None,
                                Some("texture candidate was not found".into()),
                            );
                            self.dependencies.connect(
                                model_id,
                                id,
                                format!("{relationship}:candidate"),
                            );
                        }
                        self.diagnostics.push(RenderDiagnostic {
                            severity: RenderDiagnosticSeverity::Error,
                            code:     "texture.missing".into(),
                            message:  format!(
                                "{} cannot resolve texture {}",
                                scene.name, slot.texture.name
                            ),
                            resource: Some(slot.texture.name.clone()),
                        });
                    }
                    None
                };
                let directives = slot.txi.map_or_else(Vec::new, |txi| {
                    let txi_resource = format!("{}.txi", slot.texture.name);
                    let txi_id = record_resolved(
                        self.resman,
                        self.dependencies,
                        &txi_resource,
                        DependencyKind::TextureInfo,
                    );
                    self.dependencies.connect(model_id, txi_id, "textureInfo");
                    txi.directives
                        .into_iter()
                        .map(|directive| RenderTxiDirective {
                            name:          directive.name,
                            arguments:     directive.arguments,
                            continuations: directive.continuations,
                        })
                        .collect()
                });
                textures.push(RenderMaterialTexture {
                    role,
                    source: match slot.source {
                        NwnMaterialTextureSource::Mdl => "mdl",
                        NwnMaterialTextureSource::Mtr => "mtr",
                    }
                    .into(),
                    name: slot.texture.name,
                    texture: texture_index,
                    directives,
                });
            }
            materials.push(RenderMaterialAssets {
                material_index: material.material_index,
                source_node: material.source_node,
                render_hint: material.render_hint,
                mtr,
                textures,
            });
        }
        let mut node_textures = Vec::new();
        for (node_index, node) in scene.nodes.iter().enumerate() {
            if let Some(light) = node.light.as_ref() {
                for (index, name) in light.flare_textures.iter().enumerate() {
                    if let Some(texture) = self.resolve_node_texture(
                        model_id,
                        node_index,
                        format!("flare:{index}"),
                        name,
                        appearance,
                    )? {
                        node_textures.push(texture);
                    }
                }
            }
            if let Some(emitter) = node.emitter.as_ref()
                && !emitter_uses_chunk_model(emitter)
                && let Some(name) = emitter.properties.iter().find_map(|property| {
                    property
                        .name
                        .eq_ignore_ascii_case("texture")
                        .then(|| property.values.first())
                        .flatten()
                        .and_then(|value| match value {
                            NwnPropertyValue::Text(value) => Some(value.as_str()),
                            _ => None,
                        })
                })
                && let Some(texture) = self.resolve_node_texture(
                    model_id,
                    node_index,
                    "emitter".into(),
                    name,
                    appearance,
                )?
            {
                node_textures.push(texture);
            }
        }
        Ok(RenderModelAssets {
            model_name: scene.name.clone(),
            materials,
            node_textures,
            attachments,
        })
    }

    fn resolve_node_texture(
        &mut self,
        model_id: usize,
        node_index: usize,
        role: String,
        name: &str,
        appearance: &NwnAppearanceOverrides,
    ) -> RendererResult<Option<RenderNodeTexture>> {
        let name = name.trim();
        if name.is_empty() || name.eq_ignore_ascii_case("null") {
            return Ok(None);
        }
        let texture_ref = NwnTextureRef {
            slot: NwnTextureSlot::Bitmap,
            name: name.to_string(),
        };
        let resolved = match resolve_texture_ref(
            &texture_ref,
            self.resman,
            &TextureResolverOptions::default(),
        ) {
            Ok(resolved) => resolved,
            Err(missing) => {
                for attempted in missing.attempted {
                    let id = self.dependencies.record(
                        attempted.to_string(),
                        DependencyKind::Texture,
                        DependencyState::Missing,
                        None,
                        Some(format!("missing {role} texture")),
                    );
                    self.dependencies
                        .connect(model_id, id, format!("node:{node_index}:{role}"));
                }
                self.diagnostics.push(RenderDiagnostic {
                    severity: RenderDiagnosticSeverity::Error,
                    code:     "node.texture.missing".into(),
                    message:  format!("node {node_index} cannot resolve {role} texture {name}"),
                    resource: Some(name.to_string()),
                });
                return Ok(Some(RenderNodeTexture {
                    node_index,
                    role,
                    name: name.to_string(),
                    texture: None,
                    directives: Vec::new(),
                }));
            }
        };
        let id = self.dependencies.record(
            resolved.resolved.to_string(),
            DependencyKind::Texture,
            DependencyState::Resolved,
            Some(resolved.resource.origin().to_string()),
            None,
        );
        self.dependencies
            .connect(model_id, id, format!("node:{node_index}:{role}"));
        let texture = self.decode_texture(&resolved.resource, resolved.kind, appearance)?;
        let directives = nwnrs_types::txi::TxiFile::optional_from_resman(
            self.resman,
            resolved.resolved.res_ref(),
            CachePolicy::Use,
        )
        .map_err(|error| RendererError::invalid(error.to_string()))?
        .map_or_else(Vec::new, |txi| {
            txi.directives
                .into_iter()
                .map(|directive| RenderTxiDirective {
                    name:          directive.name,
                    arguments:     directive.arguments,
                    continuations: directive.continuations,
                })
                .collect()
        });
        Ok(Some(RenderNodeTexture {
            node_index,
            role,
            name: name.to_string(),
            texture: Some(texture),
            directives,
        }))
    }

    fn decode_texture(
        &mut self,
        resource: &Res,
        kind: TextureResourceKind,
        appearance: &NwnAppearanceOverrides,
    ) -> RendererResult<usize> {
        let palette_key = if kind == TextureResourceKind::Plt {
            appearance
                .plt_rows
                .iter()
                .map(|(layer, row)| format!("{layer}:{row}"))
                .collect::<Vec<_>>()
                .join(",")
        } else {
            String::new()
        };
        let key = format!("{}#{palette_key}", resource.resref()).to_ascii_lowercase();
        if let Some(index) = self.texture_indices.get(&key).copied() {
            return Ok(index);
        }
        let (texture_kind, width, height, rgba8, compressed) = match kind {
            TextureResourceKind::Dds => {
                let texture = DdsTexture::from_res(resource, CachePolicy::Use)
                    .map_err(|error| RendererError::invalid(error.to_string()))?;
                let compression = match texture.format {
                    DdsFormat::Dxt1 => RenderTextureCompression::Dxt1,
                    DdsFormat::Dxt5 => RenderTextureCompression::Dxt5,
                };
                let mip_levels = texture
                    .mip_levels
                    .into_iter()
                    .map(|mip| RenderTextureMip {
                        width:  mip.width,
                        height: mip.height,
                        data:   mip.data,
                    })
                    .collect();
                (
                    RenderTextureKind::Dds,
                    texture.width,
                    texture.height,
                    Vec::new(),
                    Some(RenderCompressedTexture {
                        compression,
                        mip_levels,
                    }),
                )
            }
            TextureResourceKind::Tga => {
                let texture = TgaTexture::from_res(resource, CachePolicy::Use)
                    .map_err(|error| RendererError::invalid(error.to_string()))?;
                let rgba = texture
                    .decode_rgba8()
                    .map_err(|error| RendererError::invalid(error.to_string()))?;
                (
                    RenderTextureKind::Tga,
                    u32::from(texture.width),
                    u32::from(texture.height),
                    rgba,
                    None,
                )
            }
            TextureResourceKind::Plt => {
                let texture = PltTexture::from_res(resource, CachePolicy::Use)
                    .map_err(|error| RendererError::invalid(error.to_string()))?;
                self.record_plt_palettes(&texture);
                let rgba = texture
                    .render_nwn_rgba8(self.resman, &appearance.plt_rows, CachePolicy::Use)
                    .map_err(|error| RendererError::invalid(error.to_string()))?;
                (
                    RenderTextureKind::Plt,
                    texture.width,
                    texture.height,
                    rgba,
                    None,
                )
            }
        };
        let index = self.textures.len();
        self.textures.push(RenderTexture {
            resource: resource.resref().to_string(),
            origin: resource.origin().to_string(),
            kind: texture_kind,
            width,
            height,
            rgba8,
            compressed,
        });
        self.texture_indices.insert(key, index);
        Ok(index)
    }

    fn record_plt_palettes(&mut self, texture: &PltTexture) {
        let mut palettes = BTreeSet::new();
        for pixel in &texture.pixels {
            if let Some(layer) = PltLayer::from_id(pixel.layer_id) {
                palettes.insert(layer.palette_resource());
            }
        }
        for palette in palettes {
            record_resolved(
                self.resman,
                self.dependencies,
                &format!("{palette}.tga"),
                DependencyKind::Texture,
            );
        }
    }

    fn resolve_shader(
        &mut self,
        parent_id: usize,
        name: Option<&str>,
        stage: RenderShaderStage,
    ) -> Option<String> {
        let authored_name = name?;
        let name = authored_name
            .trim()
            .strip_suffix(".shd")
            .unwrap_or_else(|| authored_name.trim());
        if name.is_empty() || name.eq_ignore_ascii_case("null") {
            return None;
        }
        let filename = format!("{name}.shd");
        let Ok(resolved) = ResolvedResRef::from_filename(&filename) else {
            self.diagnostics.push(RenderDiagnostic {
                severity: RenderDiagnosticSeverity::Error,
                code:     "shader.name.invalid".into(),
                message:  format!("invalid custom shader name {name}"),
                resource: Some(filename),
            });
            return None;
        };
        let Some(resource) = self.resman.get_resolved(&resolved) else {
            let id = self.dependencies.record(
                filename.clone(),
                DependencyKind::Shader,
                DependencyState::Missing,
                None,
                Some("custom shader was not found".into()),
            );
            self.dependencies.connect(parent_id, id, "customShader");
            self.diagnostics.push(RenderDiagnostic {
                severity: RenderDiagnosticSeverity::Error,
                code:     "shader.missing".into(),
                message:  format!("custom shader {filename} was not found"),
                resource: Some(filename),
            });
            return None;
        };
        let id = self.dependencies.record(
            filename.clone(),
            DependencyKind::Shader,
            DependencyState::Resolved,
            Some(resource.origin().to_string()),
            None,
        );
        self.dependencies.connect(parent_id, id, "customShader");
        let key = (filename.to_ascii_lowercase(), stage);
        if self.shader_indices.insert(key) {
            match resource.read_all(CachePolicy::Use) {
                Ok(bytes) => match String::from_utf8(bytes) {
                    Ok(source) => {
                        self.shaders.push(RenderShaderSource {
                            resource: filename.clone(),
                            origin: resource.origin().to_string(),
                            stage,
                            source,
                        });
                        self.diagnostics.push(RenderDiagnostic {
                            severity: RenderDiagnosticSeverity::Warning,
                            code:     "shader.webgl.compatibility".into(),
                            message:  format!(
                                "{filename} is preserved and inspectable, but NWN OpenGL shader \
                                 stages cannot be executed directly by a WebGL 2 host; the \
                                 standard material channel mapping is used"
                            ),
                            resource: Some(filename.clone()),
                        });
                    }
                    Err(error) => {
                        if let Some(node) = self.dependencies.nodes.get_mut(id) {
                            node.state = DependencyState::Invalid;
                            node.message = Some(error.to_string());
                        }
                        self.diagnostics.push(RenderDiagnostic {
                            severity: RenderDiagnosticSeverity::Error,
                            code:     "shader.encoding.invalid".into(),
                            message:  format!("custom shader {filename} is not UTF-8: {error}"),
                            resource: Some(filename.clone()),
                        });
                    }
                },
                Err(error) => self.diagnostics.push(RenderDiagnostic {
                    severity: RenderDiagnosticSeverity::Error,
                    code:     "shader.read.failed".into(),
                    message:  format!("could not read {filename}: {error}"),
                    resource: Some(filename.clone()),
                }),
            }
        }
        Some(filename)
    }
}

fn emitter_uses_chunk_model(emitter: &nwnrs_types::mdl::NwnEmitter) -> bool {
    let explosion = emitter.properties.iter().any(|property| {
        property.name.eq_ignore_ascii_case("update")
            && property.values.iter().any(|value| {
                matches!(value, NwnPropertyValue::Text(value) if value.eq_ignore_ascii_case("explosion"))
            })
    });
    explosion && emitter.properties.iter().any(|property| {
        property.name.eq_ignore_ascii_case("chunkname")
            && property.values.iter().any(|value| {
                matches!(value, NwnPropertyValue::Text(value) if !value.trim().is_empty() && !value.eq_ignore_ascii_case("null"))
            })
    })
}

fn role_name(role: NwnMaterialTextureRole) -> String {
    match role {
        NwnMaterialTextureRole::Diffuse => "diffuse".into(),
        NwnMaterialTextureRole::Normal => "normal".into(),
        NwnMaterialTextureRole::Specular => "specular".into(),
        NwnMaterialTextureRole::Roughness => "roughness".into(),
        NwnMaterialTextureRole::Height => "height".into(),
        NwnMaterialTextureRole::Emissive => "emissive".into(),
        NwnMaterialTextureRole::Custom(index) => format!("custom{index}"),
    }
}

fn record_resolved(
    resman: &mut ResMan,
    dependencies: &mut DependencyGraph,
    filename: &str,
    kind: DependencyKind,
) -> usize {
    let Ok(resolved) = ResolvedResRef::from_filename(filename) else {
        return dependencies.record(
            filename,
            kind,
            DependencyState::Invalid,
            None,
            Some("invalid resource reference".into()),
        );
    };
    match resman.get_resolved(&resolved) {
        Some(resource) => dependencies.record(
            filename,
            kind,
            DependencyState::Resolved,
            Some(resource.origin().to_string()),
            None,
        ),
        None => dependencies.record(
            filename,
            kind,
            DependencyState::Missing,
            None,
            Some("resource was not found in the layered resource view".into()),
        ),
    }
}
