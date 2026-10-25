use std::collections::BTreeMap;

use rbx_dom_weak::{Instance, WeakDom, types::{CFrame, Matrix3, Ref, Variant}, ustr};

use crate::nbx::{Face, MaterialType, Node, NodeData, PartData, PartShape, PartType, SurfaceType};

#[derive(Default, Debug)]
pub struct Stats {
    pub converted: BTreeMap<String, usize>,
    pub skipped: BTreeMap<String, usize>,
    pub fallbacks: BTreeMap<String, usize>,
}

struct Ctx<'a> {
    dom: &'a WeakDom,
    nodes: Vec<Node>,
    next_id: u32,
    stats: Stats,
}

impl Ctx<'_> {
    fn push(&mut self, parent_id: u32, name: String, data: NodeData) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(Node { id, parent_id, name, data });
        id
    }

    fn fallback(&mut self, what: String) {
        *self.stats.fallbacks.entry(what).or_default() += 1;
    }
}

pub fn find_root(dom: &WeakDom, from: &str) -> (Ref, bool) {
    for inst in dom.descendants() {
        if inst.class.as_str() == from {
            return (inst.referent(), true);
        }
    }
    (dom.root_ref(), false)
}

pub fn convert(dom: &WeakDom, root: Ref) -> (Vec<Node>, Stats) {
    let mut ctx = Ctx { dom, nodes: Vec::new(), next_id: 1, stats: Stats::default() };
    visit_children(&mut ctx, root, 0);
    (ctx.nodes, ctx.stats)
}

fn visit_children(ctx: &mut Ctx, parent: Ref, parent_id: u32) {
    let Some(inst) = ctx.dom.get_by_ref(parent) else { return };
    let children: Vec<Ref> = inst.children().to_vec();

    for child_ref in children {
        let Some(child) = ctx.dom.get_by_ref(child_ref) else { continue };
        let class = child.class.as_str();

        let new_id = match class {
            "Folder" => Some(ctx.push(parent_id, child.name.clone(), NodeData::Folder)),
            "Model" => {
                let pivot = model_pivot(child);
                Some(ctx.push(parent_id, child.name.clone(), NodeData::Model { pivot }))
            }
            "Part" | "WedgePart" | "SpawnLocation" => {
                let part = convert_part(ctx, child);
                let is_spawn = part.part_type == PartType::SpawnLocation;
                let id = ctx.push(parent_id, child.name.clone(), NodeData::Part(part));
                if is_spawn {
                    ctx.push(
                        id,
                        "Decal".into(),
                        NodeData::Decal { source: "spawnLocation".into(), face: Face::Top },
                    );
                    *ctx.stats.converted.entry("Decal (spawn)".into()).or_default() += 1;
                }
                Some(id)
            }
            _ => None,
        };

        match new_id {
            Some(id) => {
                *ctx.stats.converted.entry(class.to_string()).or_default() += 1;
                visit_children(ctx, child_ref, id);
            }
            None => {
                *ctx.stats.skipped.entry(class.to_string()).or_default() += 1;
                visit_children(ctx, child_ref, parent_id);
            }
        }
    }
}

fn prop<'a>(inst: &'a Instance, name: &str) -> Option<&'a Variant> {
    inst.properties.get(&ustr(name))
}

fn get_vec3(inst: &Instance, name: &str) -> Option<[f32; 3]> {
    match prop(inst, name)? {
        Variant::Vector3(v) => Some([v.x, v.y, v.z]),
        _ => None,
    }
}

fn get_f32(inst: &Instance, name: &str) -> Option<f32> {
    match prop(inst, name)? {
        Variant::Float32(v) => Some(*v),
        Variant::Float64(v) => Some(*v as f32),
        _ => None,
    }
}

fn get_bool(inst: &Instance, name: &str) -> Option<bool> {
    match prop(inst, name)? {
        Variant::Bool(v) => Some(*v),
        _ => None,
    }
}

fn get_enum(inst: &Instance, name: &str) -> Option<u32> {
    match prop(inst, name)? {
        Variant::Enum(v) => Some(v.to_u32()),
        _ => None,
    }
}

fn get_cframe(inst: &Instance, name: &str) -> Option<CFrame> {
    match prop(inst, name)? {
        Variant::CFrame(cf) => Some(*cf),
        Variant::OptionalCFrame(cf) => *cf,
        _ => None,
    }
}

fn get_color(inst: &Instance) -> Option<[u8; 3]> {
    for name in ["Color", "Color3uint8"] {
        match prop(inst, name) {
            Some(Variant::Color3uint8(c)) => return Some([c.r, c.g, c.b]),
            Some(Variant::Color3(c)) => {
                let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                return Some([q(c.r), q(c.g), q(c.b)]);
            }
            _ => {}
        }
    }
    None
}

fn map_surface(ctx: &mut Ctx, inst: &Instance, name: &str, default: SurfaceType) -> SurfaceType {
    match get_enum(inst, name) {
        None => default,
        Some(0) => SurfaceType::Smooth,
        Some(1) => SurfaceType::Glue,
        Some(3) => SurfaceType::Studs,
        Some(4) => SurfaceType::Inlets,
        Some(other) => {
            ctx.fallback(format!("{name}={other} -> Smooth"));
            SurfaceType::Smooth
        }
    }
}

fn map_material(ctx: &mut Ctx, inst: &Instance) -> MaterialType {
    match get_enum(inst, "Material") {
        None | Some(256) | Some(272) => MaterialType::SmoothPlastic,
        Some(1280) => MaterialType::Grass,
        Some(512) | Some(528) => MaterialType::Wood,
        Some(other) => {
            ctx.fallback(format!("Material={other} -> SmoothPlastic"));
            MaterialType::SmoothPlastic
        }
    }
}

fn convert_part(ctx: &mut Ctx, inst: &Instance) -> PartData {
    let class = inst.class.as_str();
    let is_spawn = class == "SpawnLocation";

    let (def_size, def_top, def_bottom, def_side) = if is_spawn {
        ([5.0, 1.0, 5.0], SurfaceType::Smooth, SurfaceType::Smooth, SurfaceType::Glue)
    } else {
        ([4.0, 1.0, 2.0], SurfaceType::Studs, SurfaceType::Inlets, SurfaceType::Smooth)
    };

    let cf = get_cframe(inst, "CFrame").unwrap_or(CFrame::identity());

    let shape = if class == "WedgePart" {
        PartShape::Wedge
    } else {
        match get_enum(inst, "Shape") {
            Some(0) => PartShape::Ball,
            Some(2) => PartShape::Cylinder,
            Some(3) => PartShape::Wedge,
            _ => PartShape::Block,
        }
    };

    PartData {
        position: [cf.position.x, cf.position.y, cf.position.z],
        orientation: matrix_to_quat(&cf.orientation),
        size: get_vec3(inst, "Size").unwrap_or(def_size),
        color: get_color(inst).unwrap_or([163, 162, 165]),
        transparency: get_f32(inst, "Transparency").unwrap_or(0.0).clamp(0.0, 1.0),
        anchored: get_bool(inst, "Anchored").unwrap_or(false),
        part_type: if is_spawn { PartType::SpawnLocation } else { PartType::Part },
        shape,
        material: map_material(ctx, inst),
        top: map_surface(ctx, inst, "TopSurface", def_top),
        bottom: map_surface(ctx, inst, "BottomSurface", def_bottom),
        left: map_surface(ctx, inst, "LeftSurface", def_side),
        right: map_surface(ctx, inst, "RightSurface", def_side),
        front: map_surface(ctx, inst, "FrontSurface", def_side),
        back: map_surface(ctx, inst, "BackSurface", def_side),
    }
}

fn model_pivot(inst: &Instance) -> [f32; 16] {
    let cf = ["WorldPivot", "WorldPivotData"]
        .iter()
        .find_map(|n| get_cframe(inst, n))
        .unwrap_or(CFrame::identity());
    cframe_to_mat4(&cf)
}

fn rows(m: &Matrix3) -> [[f32; 3]; 3] {
    [[m.x.x, m.x.y, m.x.z], [m.y.x, m.y.y, m.y.z], [m.z.x, m.z.y, m.z.z]]
}

pub fn matrix_to_quat(m: &Matrix3) -> [f32; 4] {
    let r = rows(m);
    let trace = r[0][0] + r[1][1] + r[2][2];
    let (x, y, z, w);
    if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        w = 0.25 * s;
        x = (r[2][1] - r[1][2]) / s;
        y = (r[0][2] - r[2][0]) / s;
        z = (r[1][0] - r[0][1]) / s;
    } else if r[0][0] > r[1][1] && r[0][0] > r[2][2] {
        let s = (1.0 + r[0][0] - r[1][1] - r[2][2]).sqrt() * 2.0;
        w = (r[2][1] - r[1][2]) / s;
        x = 0.25 * s;
        y = (r[0][1] + r[1][0]) / s;
        z = (r[0][2] + r[2][0]) / s;
    } else if r[1][1] > r[2][2] {
        let s = (1.0 + r[1][1] - r[0][0] - r[2][2]).sqrt() * 2.0;
        w = (r[0][2] - r[2][0]) / s;
        x = (r[0][1] + r[1][0]) / s;
        y = 0.25 * s;
        z = (r[1][2] + r[2][1]) / s;
    } else {
        let s = (1.0 + r[2][2] - r[0][0] - r[1][1]).sqrt() * 2.0;
        w = (r[1][0] - r[0][1]) / s;
        x = (r[0][2] + r[2][0]) / s;
        y = (r[1][2] + r[2][1]) / s;
        z = 0.25 * s;
    }
    let len = (x * x + y * y + z * z + w * w).sqrt();
    if len == 0.0 || !len.is_finite() {
        return [0.0, 0.0, 0.0, 1.0];
    }
    [x / len, y / len, z / len, w / len]
}

fn cframe_to_mat4(cf: &CFrame) -> [f32; 16] {
    let r = rows(&cf.orientation);
    let mut out = [0.0f32; 16];
    for c in 0..3 {
        for row in 0..3 {
            out[c * 4 + row] = r[row][c];
        }
    }
    out[12] = cf.position.x;
    out[13] = cf.position.y;
    out[14] = cf.position.z;
    out[15] = 1.0;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rbx_dom_weak::types::Vector3;

    fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
        let (qx, qy, qz, qw) = (q[0], q[1], q[2], q[3]);
        // v' = v + 2w(q*v) + 2 q*(q*v)
        let (cx, cy, cz) = (qy * v[2] - qz * v[1], qz * v[0] - qx * v[2], qx * v[1] - qy * v[0]);
        [
            v[0] + 2.0 * (qw * cx + qy * cz - qz * cy),
            v[1] + 2.0 * (qw * cy + qz * cx - qx * cz),
            v[2] + 2.0 * (qw * cz + qx * cy - qy * cx),
        ]
    }

    #[test]
    fn quat_matches_matrix() {
        let rot_y = |a: f32| {
            let (s, c) = a.sin_cos();
            Matrix3::new(Vector3::new(c, 0.0, s), Vector3::new(0.0, 1.0, 0.0), Vector3::new(-s, 0.0, c))
        };
        let rot_x = |a: f32| {
            let (s, c) = a.sin_cos();
            Matrix3::new(Vector3::new(1.0, 0.0, 0.0), Vector3::new(0.0, c, -s), Vector3::new(0.0, s, c))
        };
        let mul = |a: &Matrix3, b: &Matrix3| {
            let (a, b) = (rows(a), rows(b));
            let mut o = [[0.0f32; 3]; 3];
            for i in 0..3 {
                for j in 0..3 {
                    o[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
                }
            }
            Matrix3::new(Vector3::new(o[0][0], o[0][1], o[0][2]), Vector3::new(o[1][0], o[1][1], o[1][2]), Vector3::new(o[2][0], o[2][1], o[2][2]))
        };

        let mats = [
            Matrix3::identity(),
            rot_y(1.0),
            rot_x(2.5),
            rot_y(3.0),
            mul(&rot_y(0.7), &rot_x(2.9)),
            mul(&rot_x(3.1), &rot_y(2.2)),
        ];
        for m in mats {
            let q = matrix_to_quat(&m);
            let r = rows(&m);
            for (axis, e) in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].iter().enumerate() {
                let got = rotate(q, *e);
                let want = [r[0][axis], r[1][axis], r[2][axis]];
                for k in 0..3 {
                    assert!((got[k] - want[k]).abs() < 1e-4, "m={m:?} axis={axis} got={got:?} want={want:?}");
                }
            }
        }
    }
}