use anyhow::{Result, bail};

pub const MAGIC: u32 = 0xA3F00;
pub const VERSION: u16 = 1;

// game::enums::InstanceType
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum InstanceType {
    Model = 3,
    Folder = 4,
    Part = 5,
    Decal = 7,
}

// game::enums::PartType
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum PartType {
    Part = 0,
    SpawnLocation = 2,
}

// game::enums::PartShape
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum PartShape {
    Ball = 0,
    Block = 1,
    Cylinder = 2,
    Wedge = 3,
}

// game::enums::SurfaceType
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum SurfaceType {
    Smooth = 0,
    Studs = 1,
    Inlets = 2,
    Glue = 3,
}

// game::enums::MaterialType
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum MaterialType {
    SmoothPlastic = 0,
    Grass = 1,
    Wood = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum Face {
    Top = 0,
}

#[derive(Clone, Debug)]
pub struct PartData {
    pub position: [f32; 3],
    pub orientation: [f32; 4],
    pub size: [f32; 3],
    pub color: [u8; 3],
    pub transparency: f32,
    pub anchored: bool,
    pub part_type: PartType,
    pub shape: PartShape,
    pub material: MaterialType,
    pub top: SurfaceType,
    pub bottom: SurfaceType,
    pub left: SurfaceType,
    pub right: SurfaceType,
    pub front: SurfaceType,
    pub back: SurfaceType,
}

#[derive(Clone, Debug)]
pub enum NodeData {
    Folder,
    Model { pivot: [f32; 16] },
    Part(PartData),
    Decal { source: String, face: Face },
}

#[derive(Clone, Debug)]
pub struct Node {
    pub id: u32,
    pub parent_id: u32,
    pub name: String,
    pub data: NodeData,
}

impl Node {
    pub fn instance_type(&self) -> InstanceType {
        match self.data {
            NodeData::Folder => InstanceType::Folder,
            NodeData::Model { .. } => InstanceType::Model,
            NodeData::Part(_) => InstanceType::Part,
            NodeData::Decal { .. } => InstanceType::Decal,
        }
    }

    fn write_payload(&self, w: &mut Writer) -> Result<()> {
        // Instance::serialize
        w.i32(self.instance_type() as i32);
        w.u32(self.id);
        w.u32(0);
        w.u32(self.parent_id);
        w.string(&self.name)?;

        match &self.data {
            NodeData::Folder => {}
            NodeData::Model { pivot } => {
                for v in pivot {
                    w.f32(*v);
                }
            }
            NodeData::Part(p) => {
                for v in p.position {
                    w.f32(v);
                }
                for v in p.orientation {
                    w.f32(v);
                }
                for v in p.size {
                    w.f32(v);
                }
                w.bytes(&p.color);
                w.f32(p.transparency);
                w.u8(p.anchored as u8);
                w.i32(p.part_type as i32);
                w.i32(p.shape as i32);
                w.i32(p.material as i32);
                for s in [p.top, p.bottom, p.left, p.right, p.front, p.back] {
                    w.i32(s as i32);
                }
            }
            NodeData::Decal { source, face } => {
                w.string(source)?;
                w.i32(*face as i32);
            }
        }
        Ok(())
    }
}

#[derive(Default)]
struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn bytes(&mut self, v: &[u8]) {
        self.buf.extend_from_slice(v);
    }
    fn string(&mut self, s: &str) -> Result<()> {
        let Ok(len) = u16::try_from(s.len()) else {
            bail!("string longer than 65535 bytes ({} byte): {:.40}…", s.len(), s);
        };
        self.u16(len);
        self.bytes(s.as_bytes());
        Ok(())
    }
}

pub fn encode(nodes: &[Node]) -> Result<Vec<u8>> {
    let mut out = Writer::default();
    out.u32(MAGIC);
    out.u16(VERSION);
    out.u32(u32::try_from(nodes.len())?);

    for node in nodes {
        let mut record = Writer::default();
        node.write_payload(&mut record)?;
        out.u32(u32::try_from(record.buf.len())?);
        out.bytes(&record.buf);
    }
    Ok(out.buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_part(part_type: PartType) -> PartData {
        PartData {
            position: [1.0, 2.0, 3.0],
            orientation: [0.0, 0.0, 0.0, 1.0],
            size: [4.0, 1.0, 2.0],
            color: [10, 20, 30],
            transparency: 0.5,
            anchored: true,
            part_type,
            shape: PartShape::Block,
            material: MaterialType::Grass,
            top: SurfaceType::Studs,
            bottom: SurfaceType::Inlets,
            left: SurfaceType::Smooth,
            right: SurfaceType::Smooth,
            front: SurfaceType::Glue,
            back: SurfaceType::Smooth,
        }
    }

    #[test]
    fn payload_sizes_match_nbx_layout() {
        let base = 4 + 4 + 4 + 4 + 2; // type, id, netId, parentId, strlen(u16) + name
        let cases = [
            (NodeData::Folder, base + 1),
            (NodeData::Model { pivot: [0.0; 16] }, base + 1 + 64),
            // vec3 + quat + vec3 + u8vec3 + float + bool + 3 enum + 6 surface
            (NodeData::Part(sample_part(PartType::Part)), base + 1 + 12 + 16 + 12 + 3 + 4 + 1 + 4 * 3 + 4 * 6),
            (NodeData::Decal { source: "s".into(), face: Face::Top }, base + 1 + 2 + 1 + 4),
        ];
        for (data, expected) in cases {
            let node = Node { id: 1, parent_id: 0, name: "n".into(), data };
            let bytes = encode(&[node]).unwrap();
            // header 10 byte + u32 size
            let size = u32::from_le_bytes(bytes[10..14].try_into().unwrap()) as usize;
            assert_eq!(size, expected);
            assert_eq!(bytes.len(), 14 + size);
        }
    }

    #[test]
    fn header() {
        let bytes = encode(&[]).unwrap();
        assert_eq!(bytes, [0x00, 0x3F, 0x0A, 0x00, 1, 0, 0, 0, 0, 0]);
    }
}