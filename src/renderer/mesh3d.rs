//! Software 3D Mesh Rasterizer (CPU-only, deterministic)
//!
//! Provides procedural mesh generation, perspective projection, view-space backface
//! culling, barycentric triangle rasterization with a z-buffer, and half-Lambert shading.

#[cfg(feature = "exp-3d")]
use glam::{Mat4, Vec3, Vec4};
#[cfg(feature = "exp-3d")]
use tiny_skia::PixmapMut;

#[cfg(feature = "exp-3d")]
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub position: Vec3,
    pub normal: Vec3,
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
            0, 1, 2, 2, 3, 0,
            // +Z face
            4, 6, 5, 6, 4, 7,
            // -Y face
            0, 4, 5, 5, 1, 0,
            // +Y face
            3, 2, 6, 6, 7, 3,
            // -X face
            0, 3, 7, 7, 4, 0,
            // +X face
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
            for &vi in &face_vertices {
                vertices.push(Vertex {
                    position: positions[vi],
                    normal: *face_normal,
                });
            }
            // Two triangles per face
            flat_indices.extend_from_slice(&[
                base_idx, base_idx + 1, base_idx + 2,
                base_idx + 2, base_idx + 3, base_idx,
            ]);
        }

        Self { vertices, indices: flat_indices }
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

                vertices.push(Vertex { position, normal });
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
            for &vi in &face_vertices[face_idx] {
                vertices.push(Vertex {
                    position: positions[vi],
                    normal: *face_normal,
                });
            }
            indices.extend_from_slice(&[base_idx, base_idx + 1, base_idx + 2]);
        }

        Self { vertices, indices }
    }

    /// Generate a torus mesh
    pub fn torus(major_radius: f32, minor_radius: f32, major_segments: u32, minor_segments: u32) -> Self {
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

                vertices.push(Vertex { position, normal });
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
}

#[cfg(feature = "exp-3d")]
#[derive(Debug, Clone, Copy)]
struct ProjectedVertex {
    x: f32,
    y: f32,
    z: f32, // depth for z-buffer
    w: f32, // homogeneous w
    normal_z: f32, // view-space normal z: negative faces the camera
    light_intensity: f32,
}

#[cfg(feature = "exp-3d")]
fn project_vertex(
    vertex: &Vertex,
    mvp: Mat4,
    normal_matrix: glam::Mat3,
    light_dir: Vec3,
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
    let light_view = light_dir.normalize();

    // Half-Lambert shading: keeps a smooth terminator instead of clipping every
    // back-facing normal to the flat ambient floor, which reads as hard banding.
    let half = (normal_view.dot(light_view) + 1.0) * 0.5;
    let diffuse = half * half;
    let intensity = ambient + (1.0 - ambient) * diffuse;

    ProjectedVertex {
        x,
        y,
        z,
        w: clip.w,
        normal_z: normal_view.z,
        light_intensity: intensity.clamp(0.0, 1.0),
    }
}

#[cfg(feature = "exp-3d")]
fn barycentric(p: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> (f32, f32, f32) {
    let (px, py) = p;
    let (ax, ay) = a;
    let (bx, by) = b;
    let (cx, cy) = c;

    let v0 = (bx - ax, by - ay);
    let v1 = (cx - ax, cy - ay);
    let v2 = (px - ax, py - ay);

    let d00 = v0.0 * v0.0 + v0.1 * v0.1;
    let d01 = v0.0 * v1.0 + v0.1 * v1.1;
    let d11 = v1.0 * v1.0 + v1.1 * v1.1;
    let d20 = v2.0 * v0.0 + v2.1 * v0.1;
    let d21 = v2.0 * v1.0 + v2.1 * v1.1;

    let denom = d00 * d11 - d01 * d01;
    if denom.abs() < 1e-10 {
        return (0.0, 0.0, 0.0);
    }

    let v = (d11 * d20 - d01 * d21) / denom;
    let w = (d00 * d21 - d01 * d20) / denom;
    let u = 1.0 - v - w;

    (u, v, w)
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
        projected.push(project_vertex(vertex, mvp, normal_matrix, light_view, ambient));
    }

    // Convert NDC [-1,1] to screen coordinates [x, x+width] x [y, y+height]
    // NDC: x in [-1,1] (left to right), y in [-1,1] (bottom to top)
    // Screen: x in [x, x+width] (left to right), y in [y, y+height] (top to bottom)
    for v in &mut projected {
        v.x = (v.x + 1.0) * 0.5 * width + x;
        v.y = (1.0 - v.y) * 0.5 * height + y;
        // z remains in NDC depth range for z-buffer
    }

    // Z-buffer
    let mut z_buffer = vec![f32::INFINITY; (width as usize) * (height as usize)];

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

        // Bounding box in screen coordinates
        let min_x = v0.x.min(v1.x).min(v2.x).ceil() as i32;
        let max_x = v0.x.max(v1.x).max(v2.x).floor() as i32;
        let min_y = v0.y.min(v1.y).min(v2.y).ceil() as i32;
        let max_y = v0.y.max(v1.y).max(v2.y).floor() as i32;

        // Clamp to mesh bounds (local coordinates within mesh area)
        let min_x = min_x.max(x as i32).min((x + width) as i32);
        let max_x = max_x.max(x as i32).min((x + width) as i32);
        let min_y = min_y.max(y as i32).min((y + height) as i32);
        let max_y = max_y.max(y as i32).min((y + height) as i32);

        if min_x > max_x || min_y > max_y {
            continue;
        }

        // Screen space coordinates of vertices
        let a = (v0.x, v0.y);
        let b = (v1.x, v1.y);
        let c = (v2.x, v2.y);

        if wireframe {
            // Draw wireframe edges
            draw_line(pixmap, &mut z_buffer, v0, v1, base_color, opacity, width as usize, x as i32, y as i32);
            draw_line(pixmap, &mut z_buffer, v1, v2, base_color, opacity, width as usize, x as i32, y as i32);
            draw_line(pixmap, &mut z_buffer, v2, v0, base_color, opacity, width as usize, x as i32, y as i32);
        } else {
            // Filled triangle
            for py in min_y..=max_y {
                for px in min_x..=max_x {
                    let (u, v, w) = barycentric((px as f32, py as f32), a, b, c);

                    if u >= -0.01 && v >= -0.01 && w >= -0.01 {
                        // Interpolate depth
                        let depth = u * v0.z + v * v1.z + w * v2.z;

                        // Z-buffer test - use LOCAL coordinates within mesh area
                        let local_px = px - x as i32;
                        let local_py = py - y as i32;
                        let idx = (local_py as usize) * (width as usize) + (local_px as usize);
                        if idx < z_buffer.len() && depth < z_buffer[idx] {
                            z_buffer[idx] = depth;

                            // Interpolate light intensity
                            let intensity = u * v0.light_intensity + v * v1.light_intensity + w * v2.light_intensity;

                            // Apply color
                            let r = (base_color[0] * intensity * 255.0) as u8;
                            let g = (base_color[1] * intensity * 255.0) as u8;
                            let b = (base_color[2] * intensity * 255.0) as u8;
                            let a = (base_color[3] * opacity * 255.0) as u8;

                            // Blend with existing pixel - direct buffer access (alpha over)
                            let pix_idx = (py as usize * pixmap.width() as usize + px as usize) * 4;
                            let data = pixmap.data_mut();
                            if pix_idx + 3 < data.len() {
                                // Source color (non-premultiplied)
                                let src_r = r as f32 / 255.0;
                                let src_g = g as f32 / 255.0;
                                let src_b = b as f32 / 255.0;
                                let src_a = a as f32 / 255.0;

                                // Destination color (from buffer, premultiplied)
                                let dst_r = data[pix_idx] as f32 / 255.0;
                                let dst_g = data[pix_idx + 1] as f32 / 255.0;
                                let dst_b = data[pix_idx + 2] as f32 / 255.0;
                                let dst_a = data[pix_idx + 3] as f32 / 255.0;

                                // Alpha over blending
                                let out_a = src_a + dst_a * (1.0 - src_a);
                                if out_a > 0.0 {
                                    let out_r = (src_r * src_a + dst_r * dst_a * (1.0 - src_a)) / out_a;
                                    let out_g = (src_g * src_a + dst_g * dst_a * (1.0 - src_a)) / out_a;
                                    let out_b = (src_b * src_a + dst_b * dst_a * (1.0 - src_a)) / out_a;

                                    data[pix_idx] = (out_r * out_a * 255.0).round().clamp(0.0, 255.0) as u8;
                                    data[pix_idx + 1] = (out_g * out_a * 255.0).round().clamp(0.0, 255.0) as u8;
                                    data[pix_idx + 2] = (out_b * out_a * 255.0).round().clamp(0.0, 255.0) as u8;
                                    data[pix_idx + 3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(feature = "exp-3d")]
#[allow(clippy::too_many_arguments)]
fn draw_line(
    pixmap: &mut PixmapMut,
    z_buffer: &mut [f32],
    v0: &ProjectedVertex,
    v1: &ProjectedVertex,
    base_color: [f32; 4],
    opacity: f32,
    stride: usize,
    offset_x: i32,
    offset_y: i32,
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

    let mut t = 0.0;
    let steps = dx.max(dy) as f32;
    if steps > 0.0 {
        loop {
            if x >= 0 && y >= 0 && (x as u32) < pixmap.width() && (y as u32) < pixmap.height() {
                // Convert screen coords to local z_buffer coords
                let local_x = x - offset_x;
                let local_y = y - offset_y;
                if local_x >= 0 && local_y >= 0 {
                    let idx = (local_y as usize) * stride + (local_x as usize);
                    if idx < z_buffer.len() {
                        let depth = v0.z + t * (v1.z - v0.z);
                        if depth < z_buffer[idx] {
                            z_buffer[idx] = depth;
                            let intensity = v0.light_intensity + t * (v1.light_intensity - v0.light_intensity);
                            let r = (base_color[0] * intensity * 255.0) as u8;
                            let g = (base_color[1] * intensity * 255.0) as u8;
                            let b = (base_color[2] * intensity * 255.0) as u8;
                            let a = (base_color[3] * opacity * 255.0) as u8;

                            let pix_idx = (y as usize * pixmap.width() as usize + x as usize) * 4;
                            let data = pixmap.data_mut();
                            if pix_idx + 3 < data.len() {
                                // Source color (non-premultiplied)
                                let src_r = r as f32 / 255.0;
                                let src_g = g as f32 / 255.0;
                                let src_b = b as f32 / 255.0;
                                let src_a = a as f32 / 255.0;

                                // Destination color (from buffer, premultiplied)
                                let dst_r = data[pix_idx] as f32 / 255.0;
                                let dst_g = data[pix_idx + 1] as f32 / 255.0;
                                let dst_b = data[pix_idx + 2] as f32 / 255.0;
                                let dst_a = data[pix_idx + 3] as f32 / 255.0;

                                // Alpha over blending
                                let out_a = src_a + dst_a * (1.0 - src_a);
                                if out_a > 0.0 {
                                    let out_r = (src_r * src_a + dst_r * dst_a * (1.0 - src_a)) / out_a;
                                    let out_g = (src_g * src_a + dst_g * dst_a * (1.0 - src_a)) / out_a;
                                    let out_b = (src_b * src_a + dst_b * dst_a * (1.0 - src_a)) / out_a;

                                    data[pix_idx] = (out_r * out_a * 255.0).round().clamp(0.0, 255.0) as u8;
                                    data[pix_idx + 1] = (out_g * out_a * 255.0).round().clamp(0.0, 255.0) as u8;
                                    data[pix_idx + 2] = (out_b * out_a * 255.0).round().clamp(0.0, 255.0) as u8;
                                    data[pix_idx + 3] = (out_a * 255.0).round().clamp(0.0, 255.0) as u8;
                                }
                            }
                        }
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
        Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, a as f32 / 255.0])
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
        assert_eq!(mesh.indices.len(), 36);  // 12 triangles * 3
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
        assert_eq!(mesh.indices.len(), 18);  // 6 triangles * 3
    }

    #[test]
    fn test_torus_mesh() {
        let mesh = Mesh::torus(0.5, 0.2, 16, 12);
        assert!(mesh.vertices.len() > 0);
        assert!(mesh.indices.len() > 0);
        assert_eq!(mesh.indices.len() % 3, 0);
    }
}