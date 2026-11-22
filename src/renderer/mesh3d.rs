//! Software 3D Mesh Rasterizer (CPU-only, deterministic)
//!
//! Provides procedural mesh generation, glTF/GLB model loading, perspective
//! projection, view-space backface culling, signed-area (barycentric) triangle
//! rasterization with a z-buffer, anti-aliased supersampling, UV texture mapping,
//! and Blinn-Phong shading (ambient + diffuse + specular + rim light).

#[cfg(feature = "exp-3d")]
use anyhow::{Context, Result, anyhow};
#[cfg(feature = "exp-3d")]
use glam::{Mat3, Mat4, Vec3, Vec4};
#[cfg(feature = "exp-3d")]
use std::collections::HashMap;
#[cfg(feature = "exp-3d")]
use std::path::{Path, PathBuf};
#[cfg(feature = "exp-3d")]
use std::sync::{Arc, Mutex, OnceLock};
#[cfg(feature = "exp-3d")]
use tiny_skia::PixmapMut;

#[cfg(feature = "exp-3d")]
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
    /// Normalized texture coordinates (0..1). Procedural primitives get sane
    /// defaults; glTF meshes carry `TEXCOORD_0` when the file provides it.
    pub uv: [f32; 2],
}

#[cfg(feature = "exp-3d")]
#[derive(Debug, Clone)]
pub struct Mesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

#[cfg(feature = "exp-3d")]
impl Mesh {
    /// Generate a cube mesh (1x1x1 centered at origin)
    pub fn cube() -> Self {
        // 8 vertices of a cube
        let positions = [
            Vec3::new(-0.5, -0.5, -0.5), // 0
            Vec3::new(0.5, -0.5, -0.5),  // 1
            Vec3::new(0.5, 0.5, -0.5),   // 2
            Vec3::new(-0.5, 0.5, -0.5),  // 3
            Vec3::new(-0.5, -0.5, 0.5),  // 4
            Vec3::new(0.5, -0.5, 0.5),   // 5
            Vec3::new(0.5, 0.5, 0.5),    // 6
            Vec3::new(-0.5, 0.5, 0.5),   // 7
        ];

        // Face normals
        let normals = [
            Vec3::new(0.0, 0.0, -1.0), // -Z
            Vec3::new(0.0, 0.0, 1.0),  // +Z
            Vec3::new(0.0, -1.0, 0.0), // -Y
            Vec3::new(0.0, 1.0, 0.0),  // +Y
            Vec3::new(-1.0, 0.0, 0.0), // -X
            Vec3::new(1.0, 0.0, 0.0),  // +X
        ];

        // 6 faces, 2 triangles each = 12 triangles = 36 indices
        let _indices = [
            // -Z face
            0, 1, 2, 2, 3, 0, // +Z face
            4, 6, 5, 6, 4, 7, // -Y face
            0, 4, 5, 5, 1, 0, // +Y face
            3, 2, 6, 6, 7, 3, // -X face
            0, 3, 7, 7, 4, 0, // +X face
            1, 5, 6, 6, 2, 1,
        ];

        let mut vertices = Vec::with_capacity(24); // 6 faces * 4 vertices (flat shading)
        let mut flat_indices = Vec::with_capacity(36);

        // For flat shading, duplicate vertices per face with face normal
        for (face_idx, face_normal) in normals.iter().enumerate() {
            let base_idx = vertices.len() as u32;
            let face_vertices = match face_idx {
                0 => [0, 1, 2, 3], // -Z
                1 => [4, 5, 6, 7], // +Z
                2 => [0, 1, 5, 4], // -Y
                3 => [3, 2, 6, 7], // +Y
                4 => [0, 3, 7, 4], // -X
                5 => [1, 2, 6, 5], // +X
                _ => [0, 1, 2, 3],
            };
            // Quad UVs run left-to-right, top-to-bottom per face so every face
            // shows the whole texture without mirroring.
            const QUAD_UV: [[f32; 2]; 4] = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
            for (quad_idx, &vi) in face_vertices.iter().enumerate() {
                vertices.push(Vertex {
                    position: positions[vi],
                    normal: *face_normal,
                    uv: QUAD_UV[quad_idx],
                });
            }
            // Two triangles per face
            flat_indices.extend_from_slice(&[
                base_idx,
                base_idx + 1,
                base_idx + 2,
                base_idx + 2,
                base_idx + 3,
                base_idx,
            ]);
        }

        Self {
            vertices,
            indices: flat_indices,
        }
    }

    /// Generate a sphere mesh (UV sphere)
    pub fn sphere(latitude_bands: u32, longitude_bands: u32) -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for lat in 0..=latitude_bands {
            let theta = lat as f32 * std::f32::consts::PI / latitude_bands as f32;
            let sin_theta = theta.sin();
            let cos_theta = theta.cos();

            for lon in 0..=longitude_bands {
                let phi = lon as f32 * 2.0 * std::f32::consts::PI / longitude_bands as f32;
                let sin_phi = phi.sin();
                let cos_phi = phi.cos();

                let x = cos_phi * sin_theta;
                let y = cos_theta;
                let z = sin_phi * sin_theta;

                let position = Vec3::new(x, y, z);
                let normal = position.normalize();
                let u = lon as f32 / longitude_bands as f32;
                let v = lat as f32 / latitude_bands as f32;

                vertices.push(Vertex {
                    position,
                    normal,
                    uv: [u, v],
                });
            }
        }

        for lat in 0..latitude_bands {
            for lon in 0..longitude_bands {
                let first = lat * (longitude_bands + 1) + lon;
                let second = first + longitude_bands + 1;

                indices.push(first);
                indices.push(second);
                indices.push(first + 1);

                indices.push(second);
                indices.push(second + 1);
                indices.push(first + 1);
            }
        }

        Self { vertices, indices }
    }

    /// Generate a pyramid mesh (square base, centered at origin)
    pub fn pyramid() -> Self {
        // 5 vertices
        let positions = [
            Vec3::new(0.0, 0.5, 0.0),    // apex (0)
            Vec3::new(-0.5, -0.5, -0.5), // base 0 (1)
            Vec3::new(0.5, -0.5, -0.5),  // base 1 (2)
            Vec3::new(0.5, -0.5, 0.5),   // base 2 (3)
            Vec3::new(-0.5, -0.5, 0.5),  // base 3 (4)
        ];

        // Face normals (flat shading)
        let face_normals = [
            Vec3::new(0.0, 0.707, -0.707).normalize(), // front
            Vec3::new(0.707, 0.707, 0.0).normalize(),  // right
            Vec3::new(0.0, 0.707, 0.707).normalize(),  // back
            Vec3::new(-0.707, 0.707, 0.0).normalize(), // left
            Vec3::new(0.0, -1.0, 0.0),                 // bottom triangle 1
            Vec3::new(0.0, -1.0, 0.0),                 // bottom triangle 2
        ];

        let face_vertices = [
            [0, 1, 2], // front
            [0, 2, 3], // right
            [0, 3, 4], // back
            [0, 4, 1], // left
            [1, 4, 3], // bottom (two triangles)
            [3, 2, 1], // bottom
        ];

        let mut vertices = Vec::with_capacity(18);
        let mut indices = Vec::with_capacity(18);

        for (face_idx, face_normal) in face_normals.iter().enumerate() {
            let base_idx = vertices.len() as u32;
            // Side faces get an apex/top triangle UV; the two bottom triangles
            // each cover half the texture.
            let uv = if face_idx < 4 {
                [[0.5, 0.0], [0.0, 1.0], [1.0, 1.0]]
            } else if face_idx == 4 {
                [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]
            } else {
                [[1.0, 1.0], [0.0, 1.0], [0.0, 0.0]]
            };
            for (corner, &vi) in face_vertices[face_idx].iter().enumerate() {
                vertices.push(Vertex {
                    position: positions[vi],
                    normal: *face_normal,
                    uv: uv[corner],
                });
            }
            indices.extend_from_slice(&[base_idx, base_idx + 1, base_idx + 2]);
        }

        Self { vertices, indices }
    }

    /// Generate a torus mesh
    pub fn torus(
        major_radius: f32,
        minor_radius: f32,
        major_segments: u32,
        minor_segments: u32,
    ) -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for i in 0..=major_segments {
            let u = i as f32 * 2.0 * std::f32::consts::PI / major_segments as f32;
            let cu = u.cos();
            let su = u.sin();

            for j in 0..=minor_segments {
                let v = j as f32 * 2.0 * std::f32::consts::PI / minor_segments as f32;
                let cv = v.cos();
                let sv = v.sin();

                let x = (major_radius + minor_radius * cv) * cu;
                let y = (major_radius + minor_radius * cv) * su;
                let z = minor_radius * sv;

                let position = Vec3::new(x, y, z);

                // Normal for torus
                let nx = cv * cu;
                let ny = cv * su;
                let nz = sv;
                let normal = Vec3::new(nx, ny, nz).normalize();
                let u = i as f32 / major_segments as f32;
                let v = j as f32 / minor_segments as f32;

                vertices.push(Vertex {
                    position,
                    normal,
                    uv: [u, v],
                });
            }
        }

        for i in 0..major_segments {
            for j in 0..minor_segments {
                let first = i * (minor_segments + 1) + j;
                let second = first + minor_segments + 1;

                indices.push(first);
                indices.push(second);
                indices.push(first + 1);

                indices.push(second);
                indices.push(second + 1);
                indices.push(first + 1);
            }
        }

        Self { vertices, indices }
    }

    /// Load a mesh from `.glb` or `.gltf` bytes.
    ///
    /// Extracts every triangle primitive, converts them to one indexed mesh,
    /// synthesises smooth normals when the file omits them, and normalizes the
    /// bounding box into the unit cube `[-0.5, 0.5]` so the model fits the
    /// default camera without clipping.
    #[allow(dead_code)]
    pub fn from_gltf_bytes(bytes: &[u8]) -> Result<Self> {
        Self::from_gltf_bytes_with_base(bytes, None)
    }

    /// Same as [`Mesh::from_gltf_bytes`] but resolves relative buffer/image URIs
    /// against `base_dir`, which is what `.gltf` files with external `.bin`
    /// payloads need.
    pub fn from_gltf_bytes_with_base(bytes: &[u8], base_dir: Option<&Path>) -> Result<Self> {
        let gltf =
            gltf::Gltf::from_slice(bytes).map_err(|e| anyhow!("invalid glTF container: {e}"))?;
        let buffers = gltf::import_buffers(&gltf.document, base_dir, gltf.blob.clone())
            .map_err(|e| anyhow!("failed to read glTF buffers: {e}"))?;

        let mut vertices: Vec<Vertex> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();

        for node in gltf.document.nodes() {
            if let Some(mesh) = node.mesh() {
                let transform = Mat4::from_cols_array_2d(&node.transform().matrix());
                for primitive in mesh.primitives() {
                    let reader =
                        primitive.reader(|buffer| buffers.get(buffer.index()).map(|d| &**d));
                    let mode = primitive.mode();
                    if !matches!(
                        mode,
                        gltf::mesh::Mode::Triangles | gltf::mesh::Mode::TriangleStrip
                    ) {
                        continue;
                    }

                    let Some(positions) = reader.read_positions() else {
                        continue;
                    };
                    let base = vertices.len() as u32;
                    let normals: Vec<[f32; 3]> = match reader.read_normals() {
                        Some(iter) => iter.collect(),
                        None => Vec::new(),
                    };
                    let uvs: Vec<[f32; 2]> = match reader.read_tex_coords(0) {
                        Some(iter) => iter.into_f32().collect(),
                        None => Vec::new(),
                    };

                    for (i, pos) in positions.enumerate() {
                        let p = transform.transform_point3(Vec3::from(pos));
                        // Normals must ignore translation, so rotate/scale-only.
                        let n = normals.get(i).map(|n| {
                            let nm = Mat3::from_mat4(transform);
                            (nm * Vec3::from(*n)).normalize()
                        });
                        let uv = uvs.get(i).copied().unwrap_or([0.0, 0.0]);
                        vertices.push(Vertex {
                            position: p,
                            normal: n.unwrap_or(Vec3::ZERO),
                            uv,
                        });
                    }

                    let local_indices: Vec<u32> = match reader.read_indices() {
                        Some(indices) => indices.into_u32().collect(),
                        None => (0..vertices.len() as u32 - base).collect(),
                    };

                    if matches!(mode, gltf::mesh::Mode::TriangleStrip) {
                        // Strip -> list: emit (v[i], v[i+1], v[i+2]) keeping winding.
                        for i in 0..local_indices.len().saturating_sub(2) {
                            let a = local_indices[i];
                            let b = local_indices[i + 1];
                            let c = local_indices[i + 2];
                            if i % 2 == 0 {
                                indices.extend_from_slice(&[base + a, base + b, base + c]);
                            } else {
                                indices.extend_from_slice(&[base + a, base + c, base + b]);
                            }
                        }
                    } else {
                        for idx in local_indices {
                            indices.push(base + idx);
                        }
                    }
                }
            }
        }

        if indices.is_empty() {
            return Err(anyhow!("glTF contains no triangle geometry"));
        }

        // Files without normals (or with zero-length ones) get smooth vertex
        // normals averaged from the adjacent faces, which keeps curvature intact
        // instead of faceting the silhouette.
        let needs_normals = vertices.iter().any(|v| v.normal.length_squared() < 1e-12);
        if needs_normals {
            let mut accum: Vec<Vec3> = vec![Vec3::ZERO; vertices.len()];
            for tri in indices.as_chunks::<3>().0 {
                let p0 = vertices[tri[0] as usize].position;
                let p1 = vertices[tri[1] as usize].position;
                let p2 = vertices[tri[2] as usize].position;
                let face = (p1 - p0).cross(p2 - p0);
                if face.length_squared() > 1e-20 {
                    let n = face.normalize();
                    for &idx in tri {
                        accum[idx as usize] += n;
                    }
                }
            }
            for (v, n) in vertices.iter_mut().zip(accum) {
                if v.normal.length_squared() < 1e-12 {
                    v.normal = if n.length_squared() > 1e-20 {
                        n.normalize()
                    } else {
                        Vec3::new(0.0, 1.0, 0.0)
                    };
                }
            }
        }

        // Fit the model into the unit cube so authored scale/rotation behaves
        // identically for procedural and imported geometry.
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for v in &vertices {
            min = min.min(v.position);
            max = max.max(v.position);
        }
        let center = (min + max) * 0.5;
        let extent = (max - min).max_element();
        let fit = if extent > 1e-6 { 1.0 / extent } else { 1.0 };
        for v in &mut vertices {
            v.position = (v.position - center) * fit;
        }

        Ok(Self { vertices, indices })
    }
}

#[cfg(feature = "exp-3d")]
#[derive(Debug, Clone, Copy)]
struct ProjectedVertex {
    x: f32,
    y: f32,
    z: f32,        // depth for z-buffer
    w: f32,        // homogeneous w
    normal_z: f32, // view-space normal z: negative faces the camera
    /// Barycentric-interpolatable shading terms, sampled per fragment.
    intensity: f32,
    specular: f32,
    rim: f32,
    /// Texture coordinates divided by `w` (perspective-correct, see `w`).
    uv_over_w: [f32; 2],
    inv_w: f32,
}

#[cfg(feature = "exp-3d")]
fn project_vertex(
    vertex: &Vertex,
    mvp: Mat4,
    normal_matrix: Mat3,
    light_view: Vec3,
    ambient: f32,
) -> ProjectedVertex {
    let pos4 = Vec4::new(vertex.position.x, vertex.position.y, vertex.position.z, 1.0);
    let clip = mvp * pos4;

    // Perspective divide
    let inv_w = 1.0 / clip.w;
    let x = clip.x * inv_w;
    let y = clip.y * inv_w;
    let z = clip.z * inv_w;

    // Transform normal into view space so shading follows the object as it rotates.
    let normal_view = (normal_matrix * vertex.normal).normalize();

    // Half-Lambert diffuse: keeps a smooth terminator instead of clipping every
    // back-facing normal to the flat ambient floor, which reads as hard banding.
    let half = (normal_view.dot(light_view) + 1.0) * 0.5;
    let diffuse = half * half;
    let intensity = (ambient + (1.0 - ambient) * diffuse).clamp(0.0, 1.0);

    // Blinn-Phong specular: the light points from the surface toward
    // `-light_view` in view space, the eye sits at the origin, and the camera
    // looks down -Z, so the view vector at this vertex is roughly +Z.
    let view_dir = Vec3::Z;
    let half_dir = (-light_view + view_dir).normalize_or_zero();
    let specular = normal_view.dot(half_dir).max(0.0).powi(24) * 0.55;

    // Rim light: normals perpendicular to the view vector catch a thin
    // silhouette glow, which is what makes a matte object read as glossy.
    let rim = (1.0 - normal_view.dot(view_dir).abs()).powi(3) * 0.45;

    ProjectedVertex {
        x,
        y,
        z,
        w: clip.w,
        normal_z: normal_view.z,
        intensity,
        specular,
        rim,
        uv_over_w: [vertex.uv[0] * inv_w, vertex.uv[1] * inv_w],
        inv_w,
    }
}

/// Decoded RGBA texture used for UV sampling. Kept separate from the mesh so a
/// single mesh can be re-shaded with different textures across frames.
#[cfg(feature = "exp-3d")]
#[derive(Debug, Clone)]
pub struct MeshTexture {
    pub width: u32,
    pub height: u32,
    /// Straight (non-premultiplied) RGBA8 pixels, row-major.
    pub pixels: Vec<u8>,
}

#[cfg(feature = "exp-3d")]
impl MeshTexture {
    /// Construct from straight RGBA8 data. Used by tests and API callers.
    #[allow(dead_code)]
    pub fn from_raw(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self> {
        let expected = (width as usize) * (height as usize) * 4;
        if width == 0 || height == 0 || pixels.len() != expected {
            return Err(anyhow!(
                "texture buffer must be {expected} bytes for {width}x{height}, got {}",
                pixels.len()
            ));
        }
        Ok(Self {
            width,
            height,
            pixels,
        })
    }

    pub fn from_file(path: &Path) -> Result<Self> {
        let img = image::open(path)
            .with_context(|| format!("failed to decode mesh texture: {}", path.display()))?
            .to_rgba8();
        Ok(Self {
            width: img.width(),
            height: img.height(),
            pixels: img.into_raw(),
        })
    }

    /// Bilinear sample with clamp-to-edge addressing. Deterministic because the
    /// weights come from the same arithmetic every run.
    fn sample(&self, u: f32, v: f32, out: &mut [f32; 4]) {
        let max_x = self.width as f32 - 1.0;
        let max_y = self.height as f32 - 1.0;
        // glTF/OpenGL UV origin is bottom-left; our pixel rows start at the top.
        let fx = (u.rem_euclid(1.0) * max_x).clamp(0.0, max_x);
        let fy = ((1.0 - v.rem_euclid(1.0)) * max_y).clamp(0.0, max_y);

        let x0 = fx.floor() as u32;
        let y0 = fy.floor() as u32;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);
        let tx = fx - x0 as f32;
        let ty = fy - y0 as f32;

        let px = |x: u32, y: u32| -> [f32; 4] {
            let idx = ((y as usize) * self.width as usize + x as usize) * 4;
            [
                self.pixels[idx] as f32 / 255.0,
                self.pixels[idx + 1] as f32 / 255.0,
                self.pixels[idx + 2] as f32 / 255.0,
                self.pixels[idx + 3] as f32 / 255.0,
            ]
        };

        let c00 = px(x0, y0);
        let c10 = px(x1, y0);
        let c01 = px(x0, y1);
        let c11 = px(x1, y1);
        for c in 0..4 {
            let top = c00[c] + (c10[c] - c00[c]) * tx;
            let bot = c01[c] + (c11[c] - c01[c]) * tx;
            out[c] = top + (bot - top) * ty;
        }
    }
}

/// Load (and memoize) a texture from disk. Used by the renderer for
/// `Mesh3d.texture`; the cache makes an animated scene decode the file once.
#[cfg(feature = "exp-3d")]
pub fn load_texture(path: &str) -> Result<Arc<MeshTexture>> {
    load_texture_cached(Path::new(path))
}

#[cfg(feature = "exp-3d")]
fn load_texture_cached(path: &Path) -> Result<Arc<MeshTexture>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Arc<MeshTexture>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    // Resolve the key so two spellings of the same file share one decode.
    let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(hit) = cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .cloned()
    {
        return Ok(hit);
    }
    let texture = Arc::new(MeshTexture::from_file(&key)?);
    cache
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(key, texture.clone());
    Ok(texture)
}

#[cfg(feature = "exp-3d")]
#[allow(clippy::too_many_arguments)]
pub fn render_mesh3d_to_pixmap(
    pixmap: &mut PixmapMut,
    mesh: &Mesh,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    fov: f32,
    color: &str,
    light_dir: [f32; 3],
    wireframe: bool,
    opacity: f32,
    scale: f32,
    rotate_x: f32,
    rotate_y: f32,
    rotate_z: f32,
    texture: Option<&MeshTexture>,
) {
    // Parse color
    let base_color = parse_color(color).unwrap_or([0.22, 0.74, 0.97, 1.0]); // #38BDF8

    // Model matrix: scale, rotate, translate
    // Base object size is 1.0 (cube/sphere/pyramid/torus are unit-sized)
    let model = Mat4::from_scale(Vec3::splat(scale))
        * Mat4::from_rotation_x(rotate_x.to_radians())
        * Mat4::from_rotation_y(rotate_y.to_radians())
        * Mat4::from_rotation_z(rotate_z.to_radians())
        * Mat4::from_translation(Vec3::new(0.0, 0.0, 0.0));

    // Compute the mesh bounding radius so every primitive fits inside the viewport
    // regardless of its intrinsic size (the sphere is 2x the cube's half-extent).
    let mut radius: f32 = 0.0;
    for v in &mesh.vertices {
        radius = radius.max(v.position.length());
    }
    let radius = radius.max(1.0);

    // View matrix: camera at (0, 0, distance) looking at origin.
    // Distance derived from the projected radius so the object always fits the view;
    // a small margin keeps the silhouette off the clipping edge.
    let half_fov = (fov.to_radians() / 2.0).tan();
    let view_dist = (radius * scale * 1.15 / half_fov).max(0.5);
    let view = Mat4::look_at_rh(
        Vec3::new(0.0, 0.0, view_dist),
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    );

    // Projection matrix
    let aspect = width / height;
    let proj = Mat4::perspective_rh(fov.to_radians(), aspect, 0.1, 1000.0);

    // MVP
    let mvp = proj * view * model;

    // Light direction is authored in world space; bring it into view space so it
    // stays fixed relative to the camera while the mesh spins.
    let light_dir_vec = Vec3::new(light_dir[0], light_dir[1], light_dir[2]).normalize();
    let view_rot = glam::Mat3::from_mat4(view);
    let normal_matrix = view_rot * glam::Mat3::from_mat4(model);
    let light_view = (view_rot * light_dir_vec).normalize();
    let ambient = 0.3;

    // Project all vertices
    let mut projected = Vec::with_capacity(mesh.vertices.len());
    for vertex in &mesh.vertices {
        projected.push(project_vertex(
            vertex,
            mvp,
            normal_matrix,
            light_view,
            ambient,
        ));
    }

    // Destination region in pixmap space. Fractional coordinates are truncated the
    // same way the layout does, so the mesh always lands on whole pixels.
    let x0 = x as i32;
    let y0 = y as i32;
    let region_w = ((x + width) as i32 - x0).max(0) as usize;
    let region_h = ((y + height) as i32 - y0).max(0) as usize;
    if region_w == 0 || region_h == 0 {
        return;
    }

    // Supersampling: rasterize into a higher-resolution offscreen buffer and
    // box-downsample it, which anti-aliases both triangle silhouettes and
    // wireframe lines. Samples per axis are chosen as the largest that keeps the
    // offscreen buffer bounded, which is deterministic for a given rect size.
    let mut ss = 1usize;
    for candidate in [4usize, 3, 2] {
        let budget = region_w * candidate * region_h * candidate;
        if budget <= 4_000_000 {
            ss = candidate;
            break;
        }
    }
    let sw = region_w * ss;
    let sh = region_h * ss;

    // Convert NDC [-1,1] to offscreen screen coordinates [0, sw] x [0, sh].
    // NDC: x in [-1,1] (left to right), y in [-1,1] (bottom to top)
    // Screen: y flipped so it grows top to bottom.
    for v in &mut projected {
        v.x = (v.x + 1.0) * 0.5 * sw as f32;
        v.y = (1.0 - v.y) * 0.5 * sh as f32;
        // z remains in NDC depth range for z-buffer
    }

    // Premultiplied RGBA accumulation buffer (source over) at supersample resolution.
    let mut color_buffer = vec![[0.0f32; 4]; sw * sh];
    // Z-buffer
    let mut z_buffer = vec![f32::INFINITY; sw * sh];

    // Rasterize triangles
    for tri in mesh.indices.chunks(3) {
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;

        let v0 = &projected[i0];
        let v1 = &projected[i1];
        let v2 = &projected[i2];

        // Near-plane culling: skip triangles with any vertex behind camera (w <= 0)
        if v0.w <= 0.0 || v1.w <= 0.0 || v2.w <= 0.0 {
            continue;
        }

        // Skip degenerate (zero-area) triangles. They otherwise produce a bogus
        // barycentric triple of (0,0,0) that passes the inside test and paints a
        // black blob over valid geometry (visible as holes at the sphere poles).
        let edge1 = (v1.x - v0.x, v1.y - v0.y);
        let edge2 = (v2.x - v0.x, v2.y - v0.y);
        let cross_z = edge1.0 * edge2.1 - edge1.1 * edge2.0;
        if cross_z.abs() < 1e-4 {
            continue;
        }

        // Backface culling by view-space normal. In view space the camera looks down
        // -Z, so a front-facing surface has its outward normal pointing toward +Z.
        // Faces whose normal points away from the camera cannot be seen. Wireframes
        // deliberately keep every edge visible.
        if !wireframe {
            let avg_normal_z = (v0.normal_z + v1.normal_z + v2.normal_z) / 3.0;
            if avg_normal_z < -1e-3 {
                continue;
            }
        }

        // Bounding box in supersampled screen coordinates. Bounds are expanded by
        // half a sample so pixel-center samples that straddle the silhouette are
        // not clipped away (the barycentric test rejects anything truly outside).
        let min_x = ((v0.x.min(v1.x).min(v2.x) - 0.5).floor() as i32).max(0);
        let max_x = ((v0.x.max(v1.x).max(v2.x) + 0.5).ceil() as i32).min(sw as i32 - 1);
        let min_y = ((v0.y.min(v1.y).min(v2.y) - 0.5).floor() as i32).max(0);
        let max_y = ((v0.y.max(v1.y).max(v2.y) + 0.5).ceil() as i32).min(sh as i32 - 1);

        if min_x > max_x || min_y > max_y {
            continue;
        }

        // Screen space coordinates of vertices
        let a = (v0.x, v0.y);
        let b = (v1.x, v1.y);
        let c = (v2.x, v2.y);

        if wireframe {
            // Draw wireframe edges
            draw_line(
                &mut color_buffer,
                &mut z_buffer,
                v0,
                v1,
                base_color,
                opacity,
                sw,
            );
            draw_line(
                &mut color_buffer,
                &mut z_buffer,
                v1,
                v2,
                base_color,
                opacity,
                sw,
            );
            draw_line(
                &mut color_buffer,
                &mut z_buffer,
                v2,
                v0,
                base_color,
                opacity,
                sw,
            );
        } else {
            // Filled triangle. Sample at pixel centers so the box downsample
            // produces proper coverage-based anti-aliasing.
            let alpha = (base_color[3] * opacity).clamp(0.0, 1.0);
            let abs_cross = cross_z.abs();

            // Cracks between triangles that share an edge are otherwise visible as
            // hairline background gaps (the sphere developed a dark seam and
            // speckles at 2x). Expand every edge outward by half a subsample in
            // barycentric units; the z-buffer resolves the overlapping slivers.
            let tol_u = 0.5 * ((c.0 - b.0).powi(2) + (c.1 - b.1).powi(2)).sqrt() / abs_cross;
            let tol_v = 0.5 * ((a.0 - c.0).powi(2) + (a.1 - c.1).powi(2)).sqrt() / abs_cross;
            let tol_w = 0.5 * ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt() / abs_cross;

            // Incremental signed-area rasterization: the three barycentric weights
            // are seeded once per scanline and advanced by a constant step per
            // sample, so the inner loop needs no divisions or re-evaluated cross
            // products. This is the classic signed-area (barycentric) formulation.
            let inv_cross = 1.0 / cross_z;
            let (ax, ay) = a;
            let (bx, by) = b;
            let (cx, cy) = c;
            let sxf = min_x as f32 + 0.5;
            let syf = min_y as f32 + 0.5;

            // d(weight)/d(px) and d(weight)/d(py), normalized by twice the area.
            let du_dx = (by - cy) * inv_cross;
            let du_dy = (cx - bx) * inv_cross;
            let dv_dx = (cy - ay) * inv_cross;
            let dv_dy = (ax - cx) * inv_cross;

            let mut row_u = (bx * cy - by * cx + sxf * (by - cy) + syf * (cx - bx)) * inv_cross;
            let mut row_v = (cx * ay - cy * ax + sxf * (cy - ay) + syf * (ax - cx)) * inv_cross;

            for py in min_y..=max_y {
                let row_start = (py as usize) * sw;
                let mut u = row_u;
                let mut v = row_v;
                for px in min_x..=max_x {
                    let w = 1.0 - u - v;

                    if u >= -tol_u && v >= -tol_v && w >= -tol_w {
                        let idx = row_start + (px as usize);
                        let depth = u * v0.z + v * v1.z + w * v2.z;
                        if depth < z_buffer[idx] {
                            z_buffer[idx] = depth;

                            let intensity =
                                (u * v0.intensity + v * v1.intensity + w * v2.intensity)
                                    .clamp(0.0, 1.0);
                            let specular = (u * v0.specular + v * v1.specular + w * v2.specular)
                                .clamp(0.0, 1.0);
                            let rim = (u * v0.rim + v * v1.rim + w * v2.rim).clamp(0.0, 1.0);

                            // Base albedo: either the flat layer color or, when a
                            // texture is bound, the bilinear UV sample. UVs are
                            // interpolated in `uv/w` space and divided by the
                            // interpolated `1/w`, which is perspective-correct.
                            let mut albedo = [base_color[0], base_color[1], base_color[2]];
                            if let Some(tex) = texture {
                                let inv_w = u * v0.inv_w + v * v1.inv_w + w * v2.inv_w;
                                if inv_w > 1e-9 {
                                    let inv = 1.0 / inv_w;
                                    let uv_u = (u * v0.uv_over_w[0]
                                        + v * v1.uv_over_w[0]
                                        + w * v2.uv_over_w[0])
                                        * inv;
                                    let uv_v = (u * v0.uv_over_w[1]
                                        + v * v1.uv_over_w[1]
                                        + w * v2.uv_over_w[1])
                                        * inv;
                                    let mut sampled = [0.0f32; 4];
                                    tex.sample(uv_u, uv_v, &mut sampled);
                                    // Tint the texture with the layer color so
                                    // `color="#FFFFFF"` is a pure pass-through.
                                    albedo = [
                                        sampled[0] * base_color[0],
                                        sampled[1] * base_color[1],
                                        sampled[2] * base_color[2],
                                    ];
                                }
                            }

                            // Blinn-Phong: ambient + diffuse on the albedo, then
                            // add specular and rim light, which are additions on
                            // top of the surface color rather than multipliers.
                            let lit = [
                                (albedo[0] * intensity + specular + rim * albedo[0] * 0.35)
                                    .clamp(0.0, 1.0),
                                (albedo[1] * intensity + specular + rim * albedo[1] * 0.35)
                                    .clamp(0.0, 1.0),
                                (albedo[2] * intensity + specular + rim * albedo[2] * 0.35)
                                    .clamp(0.0, 1.0),
                            ];

                            // Store premultiplied source so downsampling and the
                            // final composite stay correct.
                            color_buffer[idx] =
                                [lit[0] * alpha, lit[1] * alpha, lit[2] * alpha, alpha];
                        }
                    }
                    u += du_dx;
                    v += dv_dx;
                }
                row_u += du_dy;
                row_v += dv_dy;
            }
        }
    }

    // Box downsample the supersampled buffer and alpha-over it onto the pixmap.
    let samples = (ss * ss) as f32;
    let pixmap_w = pixmap.width() as usize;
    let pixmap_h = pixmap.height() as usize;
    let data = pixmap.data_mut();

    for ry in 0..region_h {
        for rx in 0..region_w {
            let mut sum = [0.0f32; 4];
            for sy in 0..ss {
                let row = (ry * ss + sy) * sw;
                for sx in 0..ss {
                    let s = color_buffer[row + rx * ss + sx];
                    sum[0] += s[0];
                    sum[1] += s[1];
                    sum[2] += s[2];
                    sum[3] += s[3];
                }
            }
            let src = [
                sum[0] / samples,
                sum[1] / samples,
                sum[2] / samples,
                sum[3] / samples,
            ];
            if src[3] <= 0.0 {
                continue;
            }

            let px = x0 + rx as i32;
            let py = y0 + ry as i32;
            if px < 0 || py < 0 || px as usize >= pixmap_w || py as usize >= pixmap_h {
                continue;
            }

            let pix_idx = (py as usize * pixmap_w + px as usize) * 4;
            if pix_idx + 3 >= data.len() {
                continue;
            }

            // Destination is premultiplied; composite premultiplied source over it.
            let dst_a = data[pix_idx + 3] as f32 / 255.0;
            let keep = 1.0 - src[3];
            let out_a = src[3] + dst_a * keep;
            let out = [
                src[0] + data[pix_idx] as f32 / 255.0 * keep,
                src[1] + data[pix_idx + 1] as f32 / 255.0 * keep,
                src[2] + data[pix_idx + 2] as f32 / 255.0 * keep,
                out_a,
            ];

            // Premultiplied channels can never exceed alpha; clamp anyway to
            // absorb float rounding so tiny-skia stays consistent.
            data[pix_idx] = (out[0].min(out_a) * 255.0).round().clamp(0.0, 255.0) as u8;
            data[pix_idx + 1] = (out[1].min(out_a) * 255.0).round().clamp(0.0, 255.0) as u8;
            data[pix_idx + 2] = (out[2].min(out_a) * 255.0).round().clamp(0.0, 255.0) as u8;
            data[pix_idx + 3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
}

#[cfg(feature = "exp-3d")]
#[allow(clippy::too_many_arguments)]
fn draw_line(
    color_buffer: &mut [[f32; 4]],
    z_buffer: &mut [f32],
    v0: &ProjectedVertex,
    v1: &ProjectedVertex,
    base_color: [f32; 4],
    opacity: f32,
    stride: usize,
) {
    let x0 = v0.x.round() as i32;
    let y0 = v0.y.round() as i32;
    let x1 = v1.x.round() as i32;
    let y1 = v1.y.round() as i32;

    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;

    let mut x = x0;
    let mut y = y0;

    let alpha = (base_color[3] * opacity).clamp(0.0, 1.0);
    let rows = z_buffer.len() / stride;

    let mut t = 0.0;
    let steps = dx.max(dy) as f32;
    if steps > 0.0 {
        loop {
            if x >= 0 && y >= 0 && (x as usize) < stride && (y as usize) < rows {
                let idx = (y as usize) * stride + (x as usize);
                if idx < z_buffer.len() {
                    let depth = v0.z + t * (v1.z - v0.z);
                    if depth < z_buffer[idx] {
                        z_buffer[idx] = depth;
                        let intensity =
                            (v0.intensity + t * (v1.intensity - v0.intensity)).clamp(0.0, 1.0);
                        let specular =
                            (v0.specular + t * (v1.specular - v0.specular)).clamp(0.0, 1.0);
                        // Premultiplied source, same convention as triangle fills.
                        // Wireframes stay flat-shaded; the far rim term belongs to
                        // the filled pass where it reads as a silhouette glow.
                        color_buffer[idx] = [
                            (base_color[0] * intensity + specular).clamp(0.0, 1.0) * alpha,
                            (base_color[1] * intensity + specular).clamp(0.0, 1.0) * alpha,
                            (base_color[2] * intensity + specular).clamp(0.0, 1.0) * alpha,
                            alpha,
                        ];
                    }
                }
            }

            if x == x1 && y == y1 {
                break;
            }
            let e2 = 2 * err;
            if e2 > -dy {
                err -= dy;
                x += sx;
            }
            if e2 < dx {
                err += dx;
                y += sy;
            }
            t += 1.0 / steps;
        }
    }
}

#[cfg(feature = "exp-3d")]
fn parse_color(hex: &str) -> Option<[f32; 4]> {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
    } else if hex.len() == 8 {
        let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
        let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
        let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
        let a = u8::from_str_radix(&hex[6..8], 16).ok()?;
        Some([
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        ])
    } else if hex.len() == 3 {
        let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
        let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
        let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
        Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0])
    } else {
        None
    }
}

#[cfg(test)]
#[cfg(feature = "exp-3d")]
mod tests {
    use super::*;

    #[test]
    fn test_cube_mesh() {
        let mesh = Mesh::cube();
        assert_eq!(mesh.vertices.len(), 24); // 6 faces * 4 vertices
        assert_eq!(mesh.indices.len(), 36); // 12 triangles * 3
    }

    #[test]
    fn test_sphere_mesh() {
        let mesh = Mesh::sphere(12, 24);
        assert!(mesh.vertices.len() > 0);
        assert!(mesh.indices.len() > 0);
        assert_eq!(mesh.indices.len() % 3, 0);
    }

    #[test]
    fn test_pyramid_mesh() {
        let mesh = Mesh::pyramid();
        assert_eq!(mesh.vertices.len(), 18); // 6 faces * 3 vertices
        assert_eq!(mesh.indices.len(), 18); // 6 triangles * 3
    }

    #[test]
    fn test_torus_mesh() {
        let mesh = Mesh::torus(0.5, 0.2, 16, 12);
        assert!(mesh.vertices.len() > 0);
        assert!(mesh.indices.len() > 0);
        assert_eq!(mesh.indices.len() % 3, 0);
    }
}
