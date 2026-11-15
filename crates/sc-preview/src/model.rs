//! 3D models (STL, OBJ, PLY, glTF/GLB) and a small software renderer that
//! draws them shaded from any angle, so terminals and windows show them
//! without a GPU.

use std::io::Cursor;

use image::{Rgba, RgbaImage};

/// A point or direction in space.
pub type Vec3 = [f32; 3];

/// A model as triangles.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    /// Corners of the triangles.
    pub triangles: Vec<[Vec3; 3]>,
}

/// The model formats this module reads, by file extension.
pub fn is_model_name(name: &str) -> bool {
    let extension = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    matches!(extension.as_deref(), Some("stl" | "obj" | "ply" | "gltf" | "glb"))
}

impl Mesh {
    /// Reads a model; `name` tells the format.
    pub fn load(name: &str, data: &[u8]) -> Result<Mesh, String> {
        let extension =
            name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        let mesh = match extension.as_str() {
            "stl" => stl(data)?,
            "obj" => obj(data)?,
            "ply" => ply(data)?,
            "gltf" | "glb" => gltf(data)?,
            _ => return Err(format!("{name}: not a 3D model this viewer reads")),
        };
        if mesh.triangles.is_empty() {
            return Err("the model has no triangles".into());
        }
        Ok(mesh)
    }

    /// The smallest box holding the model: (minimum, maximum) corners.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let mut low = [f32::INFINITY; 3];
        let mut high = [f32::NEG_INFINITY; 3];
        for point in self.triangles.iter().flatten() {
            for axis in 0..3 {
                low[axis] = low[axis].min(point[axis]);
                high[axis] = high[axis].max(point[axis]);
            }
        }
        (low, high)
    }

    /// Number of distinct corners, roughly (for the status line).
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }
}

fn stl(data: &[u8]) -> Result<Mesh, String> {
    let model = stl_io::read_stl(&mut Cursor::new(data))
        .map_err(|e| format!("cannot read this STL: {e}"))?;
    let point = |index: usize| -> Vec3 {
        let v = model.vertices[index];
        [v[0], v[1], v[2]]
    };
    let triangles = model
        .faces
        .iter()
        .map(|face| [point(face.vertices[0]), point(face.vertices[1]), point(face.vertices[2])])
        .collect();
    Ok(Mesh { triangles })
}

fn obj(data: &[u8]) -> Result<Mesh, String> {
    let options = tobj::LoadOptions { triangulate: true, single_index: true, ..Default::default() };
    // Materials live in other files, which a preview does not need.
    let (models, _) =
        tobj::load_obj_buf(&mut Cursor::new(data), &options, |_| Ok(Default::default()))
            .map_err(|e| format!("cannot read this OBJ: {e}"))?;
    let mut triangles = Vec::new();
    for model in models {
        let mesh = model.mesh;
        let point = |index: u32| -> Vec3 {
            let i = index as usize * 3;
            [mesh.positions[i], mesh.positions[i + 1], mesh.positions[i + 2]]
        };
        for face in mesh.indices.chunks_exact(3) {
            triangles.push([point(face[0]), point(face[1]), point(face[2])]);
        }
    }
    Ok(Mesh { triangles })
}

fn gltf(data: &[u8]) -> Result<Mesh, String> {
    let (document, buffers, _) =
        gltf::import_slice(data).map_err(|e| format!("cannot read this glTF: {e}"))?;
    let mut triangles = Vec::new();
    // Meshes are placed by their nodes' transforms.
    for scene in document.scenes() {
        for node in scene.nodes() {
            collect_node(&node, identity(), &buffers, &mut triangles);
        }
    }
    if triangles.is_empty() {
        // No scene: take the meshes as they are.
        for mesh in document.meshes() {
            add_mesh(&mesh, identity(), &buffers, &mut triangles);
        }
    }
    Ok(Mesh { triangles })
}

type Matrix = [[f32; 4]; 4];

fn identity() -> Matrix {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

/// `a × b` for column-major matrices, as glTF stores them.
fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    let mut out = [[0.0; 4]; 4];
    for column in 0..4 {
        for row in 0..4 {
            out[column][row] = (0..4).map(|k| a[k][row] * b[column][k]).sum();
        }
    }
    out
}

fn transform(m: &Matrix, p: [f32; 3]) -> Vec3 {
    let mut out = [0.0; 3];
    for (row, value) in out.iter_mut().enumerate() {
        *value = m[0][row] * p[0] + m[1][row] * p[1] + m[2][row] * p[2] + m[3][row];
    }
    out
}

fn collect_node(
    node: &gltf::Node<'_>,
    parent: Matrix,
    buffers: &[gltf::buffer::Data],
    out: &mut Vec<[Vec3; 3]>,
) {
    let matrix = multiply(&parent, &node.transform().matrix());
    if let Some(mesh) = node.mesh() {
        add_mesh(&mesh, matrix, buffers, out);
    }
    for child in node.children() {
        collect_node(&child, matrix, buffers, out);
    }
}

fn add_mesh(
    mesh: &gltf::Mesh<'_>,
    matrix: Matrix,
    buffers: &[gltf::buffer::Data],
    out: &mut Vec<[Vec3; 3]>,
) {
    for primitive in mesh.primitives() {
        if primitive.mode() != gltf::mesh::Mode::Triangles {
            continue;
        }
        let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(|d| &d.0[..]));
        let Some(positions) = reader.read_positions() else { continue };
        let points: Vec<Vec3> = positions.map(|p| transform(&matrix, p)).collect();
        let indices: Vec<u32> = match reader.read_indices() {
            Some(indices) => indices.into_u32().collect(),
            None => (0..points.len() as u32).collect(),
        };
        for face in indices.chunks_exact(3) {
            let corner = |i: u32| points.get(i as usize).copied();
            if let (Some(a), Some(b), Some(c)) = (corner(face[0]), corner(face[1]), corner(face[2]))
            {
                out.push([a, b, c]);
            }
        }
    }
}

/// A PLY property: its name, its type, and for lists the types of the
/// count and of the items.
type PlyProperty = (String, String, Option<(String, String)>);
/// A PLY element: its name, how many there are, and its properties.
type PlyElement = (String, usize, Vec<PlyProperty>);

/// PLY, ASCII or binary (either byte order), vertices and faces only.
fn ply(data: &[u8]) -> Result<Mesh, String> {
    let bad = |why: &str| format!("cannot read this PLY: {why}");
    let end = data.windows(10).position(|w| w == b"end_header").ok_or_else(|| bad("no header"))?;
    let header = std::str::from_utf8(&data[..end]).map_err(|_| bad("header is not text"))?;
    let mut body = &data[end + 10..];
    // Skip the line end after end_header.
    while let [b'\r' | b'\n', rest @ ..] = body {
        body = rest;
        if body.first() != Some(&b'\n') {
            break;
        }
    }
    let mut format = "";
    let mut elements: Vec<PlyElement> = Vec::new();
    for line in header.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["format", f, ..] => format = f,
            ["element", name, count] => elements.push((
                name.to_string(),
                count.parse().map_err(|_| bad("element count"))?,
                Vec::new(),
            )),
            ["property", "list", count_type, item_type, name] => {
                if let Some(element) = elements.last_mut() {
                    element.2.push((
                        name.to_string(),
                        String::new(),
                        Some((count_type.to_string(), item_type.to_string())),
                    ));
                }
            }
            ["property", kind, name] => {
                if let Some(element) = elements.last_mut() {
                    element.2.push((name.to_string(), kind.to_string(), None));
                }
            }
            _ => {}
        }
    }
    let mut reader = PlyReader {
        data: body,
        ascii: format == "ascii",
        big: format == "binary_big_endian",
        words: None,
    };
    let mut vertices: Vec<Vec3> = Vec::new();
    let mut triangles = Vec::new();
    for (name, count, properties) in &elements {
        for _ in 0..*count {
            let mut point = [0.0f32; 3];
            for (property, kind, list) in properties {
                match list {
                    Some((count_type, item_type)) => {
                        let n =
                            reader.number(count_type).ok_or_else(|| bad("short file"))? as usize;
                        let mut items = Vec::with_capacity(n);
                        for _ in 0..n {
                            items
                                .push(reader.number(item_type).ok_or_else(|| bad("short file"))?
                                    as usize);
                        }
                        if name == "face"
                            && (property == "vertex_indices" || property == "vertex_index")
                        {
                            // A fan of triangles for polygons.
                            for i in 1..items.len().saturating_sub(1) {
                                let corner = |k: usize| vertices.get(items[k]).copied();
                                if let (Some(a), Some(b), Some(c)) =
                                    (corner(0), corner(i), corner(i + 1))
                                {
                                    triangles.push([a, b, c]);
                                }
                            }
                        }
                    }
                    None => {
                        let value = reader.number(kind).ok_or_else(|| bad("short file"))? as f32;
                        if name == "vertex" {
                            match property.as_str() {
                                "x" => point[0] = value,
                                "y" => point[1] = value,
                                "z" => point[2] = value,
                                _ => {}
                            }
                        }
                    }
                }
            }
            if name == "vertex" {
                vertices.push(point);
            }
        }
    }
    Ok(Mesh { triangles })
}

struct PlyReader<'a> {
    data: &'a [u8],
    ascii: bool,
    big: bool,
    words: Option<std::str::SplitAsciiWhitespace<'a>>,
}

impl PlyReader<'_> {
    fn number(&mut self, kind: &str) -> Option<f64> {
        if self.ascii {
            let words = self.words.get_or_insert_with(|| {
                std::str::from_utf8(self.data).unwrap_or("").split_ascii_whitespace()
            });
            return words.next()?.parse().ok();
        }
        let size = match kind {
            "char" | "uchar" | "int8" | "uint8" => 1,
            "short" | "ushort" | "int16" | "uint16" => 2,
            "int" | "uint" | "float" | "int32" | "uint32" | "float32" => 4,
            "double" | "float64" => 8,
            _ => return None,
        };
        if self.data.len() < size {
            return None;
        }
        let (bytes, rest) = self.data.split_at(size);
        self.data = rest;
        let mut buffer = [0u8; 8];
        buffer[..size].copy_from_slice(bytes);
        if self.big {
            buffer[..size].reverse();
        }
        let b = buffer;
        Some(match kind {
            "char" | "int8" => f64::from(b[0] as i8),
            "uchar" | "uint8" => f64::from(b[0]),
            "short" | "int16" => f64::from(i16::from_le_bytes([b[0], b[1]])),
            "ushort" | "uint16" => f64::from(u16::from_le_bytes([b[0], b[1]])),
            "int" | "int32" => f64::from(i32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            "uint" | "uint32" => f64::from(u32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            "float" | "float32" => f64::from(f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            _ => f64::from_le_bytes(b),
        })
    }
}

/// Where the model is seen from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    /// Turn around the vertical axis, in degrees.
    pub yaw: f32,
    /// Tilt up or down, in degrees.
    pub pitch: f32,
    /// 1.0 fits the model in the picture; larger is closer.
    pub zoom: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Camera { yaw: 35.0, pitch: 25.0, zoom: 1.0 }
    }
}

fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn normalize(v: Vec3) -> Vec3 {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if length == 0.0 { v } else { [v[0] / length, v[1] / length, v[2] / length] }
}

/// Draws `mesh` seen from `camera` in a `width` × `height` picture: an
/// orthographic view, lit from the viewer's upper left, with a depth buffer.
/// Models whose "up" is Z (most STL files) are turned to stand upright.
pub fn render(mesh: &Mesh, camera: Camera, width: u32, height: u32) -> RgbaImage {
    let (width, height) = (width.max(8), height.max(8));
    let mut picture = RgbaImage::from_fn(width, height, |_, y| {
        // A soft vertical gradient behind the model.
        let t = y as f32 / height as f32;
        let shade = (52.0 + 30.0 * t) as u8;
        Rgba([shade, shade, (shade as f32 * 1.1) as u8, 255])
    });
    let mut depth = vec![f32::INFINITY; (width * height) as usize];
    let (low, high) = mesh.bounds();
    let center = [(low[0] + high[0]) / 2.0, (low[1] + high[1]) / 2.0, (low[2] + high[2]) / 2.0];
    let extent = sub(high, low);
    let radius =
        (extent[0] * extent[0] + extent[1] * extent[1] + extent[2] * extent[2]).sqrt() / 2.0;
    let radius = if radius > 0.0 { radius } else { 1.0 };
    // Z-up models (flat in Z, tall in Y is unusual for STL) stand upright.
    let z_up = extent[2] > extent[1] * 1.2;
    let (sy, cy) = camera.yaw.to_radians().sin_cos();
    let (sp, cp) = camera.pitch.to_radians().sin_cos();
    let scale = f32::min(width as f32, height as f32) / 2.0 / radius * 0.92 * camera.zoom.max(0.05);
    let project = |p: Vec3| -> Vec3 {
        let mut q = sub(p, center);
        if z_up {
            q = [q[0], q[2], -q[1]];
        }
        // Yaw around Y, then pitch around X.
        let x = q[0] * cy + q[2] * sy;
        let z = -q[0] * sy + q[2] * cy;
        let y = q[1] * cp - z * sp;
        let z = q[1] * sp + z * cp;
        [width as f32 / 2.0 + x * scale, height as f32 / 2.0 - y * scale, -z]
    };
    let light = normalize([-0.4, 0.6, 0.7]);
    for triangle in &mesh.triangles {
        let [a, b, c] = triangle.map(project);
        // Normal in view space (screen y points down, so flip it back).
        let normal = normalize(cross(
            sub([b[0], -b[1], -b[2]], [a[0], -a[1], -a[2]]),
            sub([c[0], -c[1], -c[2]], [a[0], -a[1], -a[2]]),
        ));
        // Both sides are lit: models are not always closed or wound alike.
        let lambert = (normal[0] * light[0] + normal[1] * light[1] + normal[2] * light[2]).abs();
        let intensity = 0.25 + 0.75 * lambert;
        let color = Rgba([
            (215.0 * intensity) as u8,
            (175.0 * intensity) as u8,
            (110.0 * intensity) as u8,
            255,
        ]);
        fill(&mut picture, &mut depth, [a, b, c], color);
    }
    picture
}

/// Fills one projected triangle, keeping the nearest surface per pixel.
fn fill(picture: &mut RgbaImage, depth: &mut [f32], [a, b, c]: [Vec3; 3], color: Rgba<u8>) {
    let (width, height) = (picture.width() as i64, picture.height() as i64);
    let min_x = a[0].min(b[0]).min(c[0]).floor().max(0.0) as i64;
    let max_x = (a[0].max(b[0]).max(c[0]).ceil() as i64).min(width - 1);
    let min_y = a[1].min(b[1]).min(c[1]).floor().max(0.0) as i64;
    let max_y = (a[1].max(b[1]).max(c[1]).ceil() as i64).min(height - 1);
    let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    if area.abs() < 1e-6 || min_x > max_x || min_y > max_y {
        return;
    }
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
            let w1 = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let z = w0 * a[2] + w1 * b[2] + w2 * c[2];
            let index = (y * width + x) as usize;
            if z < depth[index] {
                depth[index] = z;
                picture.put_pixel(x as u32, y as u32, color);
            }
        }
    }
}
