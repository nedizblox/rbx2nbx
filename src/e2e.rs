use rbx_dom_weak::{
    InstanceBuilder, WeakDom,
    types::{CFrame, Color3uint8, Enum, Matrix3, Variant, Vector3},
};

use crate::{
    convert::{convert, find_root},
    nbx::{self, NodeData, PartShape, PartType, SurfaceType},
};

fn roundtrip(dom: &WeakDom) -> WeakDom {
    let mut buf = Vec::new();
    let refs = dom.root().children().to_vec();
    rbx_binary::to_writer(&mut buf, dom, &refs).unwrap();
    assert!(buf.starts_with(b"<roblox!"));
    rbx_binary::from_reader(buf.as_slice()).unwrap()
}

#[test]
fn rbxl_to_nbxl() {
    let part = InstanceBuilder::new("Part")
        .with_name("Floor")
        .with_property("CFrame", CFrame::new(Vector3::new(1.0, 2.0, 3.0), Matrix3::identity()))
        .with_property("Size", Vector3::new(10.0, 2.0, 10.0))
        .with_property("Color3uint8", Color3uint8::new(1, 2, 3))
        .with_property("Anchored", true)
        .with_property("Transparency", 0.25f32)
        .with_property("Shape", Variant::Enum(Enum::from_u32(2)))
        .with_property("TopSurface", Variant::Enum(Enum::from_u32(0)));

    let spawn = InstanceBuilder::new("SpawnLocation").with_name("Spawn");

    let workspace = InstanceBuilder::new("Workspace")
        .with_child(part)
        .with_child(
            InstanceBuilder::new("Model")
                .with_name("House")
                .with_child(spawn)
                .with_child(InstanceBuilder::new("Script").with_child(InstanceBuilder::new("Folder").with_name("InScript"))),
        )
        .with_child(InstanceBuilder::new("Camera"));

    let dom = WeakDom::new(InstanceBuilder::new("DataModel").with_child(workspace));
    let dom = roundtrip(&dom);

    let (root, found) = find_root(&dom, "Workspace");
    assert!(found);
    let (nodes, stats) = convert(&dom, root);

    let names: Vec<_> = nodes.iter().map(|n| (n.name.as_str(), n.id, n.parent_id)).collect();
    println!("{names:?} {stats:?}");
    assert_eq!(nodes.len(), 5);

    let by_name = |n: &str| nodes.iter().find(|x| x.name == n).unwrap();
    let floor = by_name("Floor");
    let house = by_name("House");
    assert_eq!(floor.parent_id, 0);
    assert_eq!(house.parent_id, 0);
    assert_eq!(by_name("Spawn").parent_id, house.id);
    assert_eq!(by_name("Decal").parent_id, by_name("Spawn").id);
    assert_eq!(by_name("InScript").parent_id, house.id);
    assert_eq!(stats.skipped.get("Script"), Some(&1));
    assert_eq!(stats.skipped.get("Camera"), Some(&1));

    for (i, n) in nodes.iter().enumerate() {
        assert_ne!(n.id, 0);
        assert!(n.parent_id == 0 || nodes[..i].iter().any(|p| p.id == n.parent_id));
    }

    let NodeData::Part(p) = &floor.data else { panic!() };
    assert_eq!(p.position, [1.0, 2.0, 3.0]);
    assert_eq!(p.size, [10.0, 2.0, 10.0]);
    assert_eq!(p.color, [1, 2, 3]);
    assert!(p.anchored);
    assert_eq!(p.transparency, 0.25);
    assert_eq!(p.shape, PartShape::Cylinder);
    assert_eq!(p.top, SurfaceType::Smooth);
    assert_eq!(p.bottom, SurfaceType::Inlets);
    assert_eq!(p.part_type, PartType::Part);
    assert_eq!(p.orientation, [0.0, 0.0, 0.0, 1.0]);

    let NodeData::Part(s) = &by_name("Spawn").data else { panic!() };
    assert_eq!(s.part_type, PartType::SpawnLocation);

    let bytes = nbx::encode(&nodes).unwrap();
    assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), 0xA3F00);
    assert_eq!(u32::from_le_bytes(bytes[6..10].try_into().unwrap()), 5);
}