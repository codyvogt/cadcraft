//! Scale actions of dynamic blocks: a reference scales the action's objects about its base point
//! by how much the distance it follows grew, as AutoCAD does (a 2×2 square scaled by a distance
//! going from 2 to 4 is 4×4), carries the parameters in its selection with them, and keeps the
//! result through a save.
//!
//! The fixture is synthetic: block Grow (a 2×2 square and a circle at (1, 1), Distance1 from the
//! origin to (2, 0), scaled about the distance's base; Position1 at (2, 2) is in the selection),
//! Pinned (the square, scaled about the fixed point (2, 2)) and Sheet (the square, scaled on each
//! axis by X Distance1 and Y Distance1, from the origin to (2, 2)).

use cadcraft_doc::dynblock::{ActionKind, custom_properties, dynamic_block, set_property};
use cadcraft_doc::geom::Vec3;
use cadcraft_doc::{Drawing, EntityKind, Handle};
use cadcraft_io::{read_dxf, write_dxf};

fn dxf(src: &str) -> String {
    src.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| {
            let (code, value) = l.split_once(' ').unwrap_or((l, ""));
            format!("{code}\n{value}\n")
        })
        .collect()
}

/// The fixture with a reference to `block` at (100, 100).
fn reference(block: &str) -> (Drawing, Handle) {
    let mut d = read_dxf(dxf(&include_str!("data/dynblock_scale.txt").replace("\r\n", "\n")).as_bytes()).unwrap();
    let ins = cadcraft_doc::Insert {
        block: block.into(),
        insert: Vec3::new(100.0, 100.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: Vec::new(),
        cols: 1,
        rows: 1,
        col_spacing: 0.0,
        row_spacing: 0.0,
    };
    let r = d.add(&cadcraft_doc::Space::Model, cadcraft_doc::Common::default(), EntityKind::Insert(ins)).unwrap();
    (d, r)
}

fn props(d: &Drawing, r: Handle) -> Vec<(String, String)> {
    custom_properties(d, r).unwrap().1.into_iter().map(|p| (p.name, p.value)).collect()
}

/// Each object of the reference's block, in block coordinates: a polyline's corners (lowest and
/// highest), a circle's centre and radius.
fn shapes(d: &Drawing, r: Handle) -> Vec<[f64; 4]> {
    let Some(EntityKind::Insert(i)) = d.entity(r).map(|e| &e.kind) else { panic!("a reference") };
    let round = |v: f64| (v * 1000.0).round() / 1000.0 + 0.0;
    d.block(&i.block)
        .unwrap()
        .entities
        .iter()
        .map(|e| match &e.kind {
            EntityKind::LwPolyline(p) => {
                let xs = p.vertices.iter().map(|v| v.p.x);
                let ys = p.vertices.iter().map(|v| v.p.y);
                [
                    round(xs.clone().fold(f64::MAX, f64::min)),
                    round(ys.clone().fold(f64::MAX, f64::min)),
                    round(xs.fold(f64::MIN, f64::max)),
                    round(ys.fold(f64::MIN, f64::max)),
                ]
            }
            EntityKind::Circle(c) => [round(c.center.x), round(c.center.y), round(c.radius), 0.0],
            k => panic!("unexpected {k:?}"),
        })
        .collect()
}

#[test]
fn reads_the_scale_actions_base_and_connections() {
    let (d, _) = reference("Grow");
    let m = dynamic_block(&d, "Grow").unwrap();
    let a = m.actions.iter().find(|a| matches!(a.kind, ActionKind::Scale { .. })).unwrap();
    let ActionKind::Scale { base } = a.kind else { unreachable!() };
    assert!(base.dependent);
    let names: Vec<&str> = a.connections.iter().map(|(_, c)| c.as_str()).collect();
    assert_eq!(names, ["UpdatedBaseX", "UpdatedBaseY", "Scale", "XScale", "YScale"]);
    let m = dynamic_block(&d, "Pinned").unwrap();
    let ActionKind::Scale { base } = m.actions[0].kind else { panic!("a scale action") };
    assert!(!base.dependent);
    assert_eq!((base.at.x, base.at.y), (2.0, 2.0));
}

#[test]
fn a_distance_scales_its_objects_about_the_base() {
    let (mut d, r) = reference("Grow");
    set_property(&mut d, r, "Distance1", "4").unwrap();
    assert_eq!(shapes(&d, r), vec![[0.0, 0.0, 4.0, 4.0], [2.0, 2.0, 1.0, 0.0]]);
    // The point parameter in the selection is scaled with them.
    let p = props(&d, r);
    assert!(p.contains(&("Position1 X".into(), "4.0000".into())), "{p:?}");
    assert!(p.contains(&("Position1 Y".into(), "4.0000".into())), "{p:?}");
    // From there, by the ratio of the new distance to the one it has now.
    set_property(&mut d, r, "Distance1", "1").unwrap();
    assert_eq!(shapes(&d, r), vec![[0.0, 0.0, 1.0, 1.0], [0.5, 0.5, 0.25, 0.0]]);
}

#[test]
fn a_fixed_base_point_stays() {
    let (mut d, r) = reference("Pinned");
    set_property(&mut d, r, "Distance1", "4").unwrap();
    assert_eq!(shapes(&d, r), vec![[-2.0, -2.0, 2.0, 2.0]]);
}

#[test]
fn an_xy_parameter_scales_each_axis_on_its_own() {
    let (mut d, r) = reference("Sheet");
    set_property(&mut d, r, "X Distance1", "4").unwrap();
    assert_eq!(shapes(&d, r), vec![[0.0, 0.0, 4.0, 2.0]]);
    set_property(&mut d, r, "Y Distance1", "1").unwrap();
    assert_eq!(shapes(&d, r), vec![[0.0, 0.0, 4.0, 1.0]]);
}

#[test]
fn scaled_objects_and_values_survive_a_save() {
    let (mut d, r) = reference("Grow");
    set_property(&mut d, r, "Distance1", "3").unwrap();
    let mut back = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(props(&back, r), props(&d, r));
    assert_eq!(shapes(&back, r), shapes(&d, r));
    set_property(&mut back, r, "Distance1", "6").unwrap();
    assert_eq!(shapes(&back, r), vec![[0.0, 0.0, 6.0, 6.0], [3.0, 3.0, 1.5, 0.0]]);
}

#[test]
fn hostile_scales_are_refused_and_change_nothing() {
    let (mut d, r) = reference("Grow");
    for v in ["0", "-3", "NaN", "inf", "1e-300", "1e200"] {
        assert!(set_property(&mut d, r, "Distance1", v).is_err(), "{v}");
    }
    assert_eq!(shapes(&d, r), vec![[0.0, 0.0, 2.0, 2.0], [1.0, 1.0, 0.5, 0.0]]);
    // A large factor within the limit is a large block.
    set_property(&mut d, r, "Distance1", "2e6").unwrap();
    assert_eq!(shapes(&d, r), vec![[0.0, 0.0, 2e6, 2e6], [1e6, 1e6, 5e5, 0.0]]);
    // An XY distance of 0 would scale the objects to nothing.
    let (mut d, r) = reference("Sheet");
    assert!(set_property(&mut d, r, "X Distance1", "0").is_err());
    assert_eq!(shapes(&d, r), vec![[0.0, 0.0, 2.0, 2.0]]);
}
