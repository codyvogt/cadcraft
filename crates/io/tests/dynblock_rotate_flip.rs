//! Rotation and flip parameters of dynamic blocks: their rotate and flip actions turn and mirror
//! the objects (text stays readable), carry the parameters in their selection, record the
//! change as AutoCAD does, and give the reference a rotation grip and a flip grip.
//!
//! The fixture is synthetic: block Symbol (a circle, a line from the origin to (0, 3) and an
//! attribute TAG at (1, 2) turned 90°) with Angle1 (base at the origin, radius to (2, 0)) and its
//! rotate action, Flip state1 (reflection line from the origin to (-10, 0)) and its flip action,
//! and Position1 at (0, 3) with a move action.

use cadcraft_doc::dynblock::{ActionKind, GripPart, GripShape, ParameterKind, custom_properties, drag_grip, dynamic_block, grips, set_property};
use cadcraft_doc::geom::{Vec2, Vec3};
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

/// Symbol and a reference to it at (100, 100) with its attribute.
fn symbol() -> (Drawing, Handle) {
    let src = include_str!("data/dynblock_symbol.txt").replace("\r\n", "\n");
    let mut d = read_dxf(dxf(&src).as_bytes()).unwrap();
    let def =
        d.block("Symbol").unwrap().entities.iter().find_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some(a.clone()) } else { None }).unwrap();
    let mut a = def.clone();
    a.text.insert = a.text.insert + Vec3::new(100.0, 100.0, 0.0);
    let ins = cadcraft_doc::Insert {
        block: "Symbol".into(),
        insert: Vec3::new(100.0, 100.0, 0.0),
        scale: Vec3::new(1.0, 1.0, 1.0),
        rotation: 0.0,
        attribs: vec![a],
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

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
}

/// The reference's block: its line's end and attribute definition (insertion, rotation in degrees).
fn shown(d: &Drawing, r: Handle) -> (Vec2, Vec2, f64) {
    let Some(EntityKind::Insert(i)) = d.entity(r).map(|e| &e.kind) else { panic!("no reference") };
    let b = d.block(&i.block).unwrap();
    let line = b.entities.iter().find_map(|e| if let EntityKind::Line(l) = &e.kind { Some(l.b.xy()) } else { None }).unwrap();
    let (at, rot) =
        b.entities.iter().find_map(|e| if let EntityKind::AttDef(a) = &e.kind { Some((a.text.insert.xy(), a.text.rotation)) } else { None }).unwrap();
    (line, at, rot.to_degrees())
}

fn near(a: Vec2, b: (f64, f64)) -> bool {
    (a.x - b.0).abs() < 1e-6 && (a.y - b.1).abs() < 1e-6
}

#[test]
fn rotation_and_flip_are_read() {
    let (d, _) = symbol();
    let m = dynamic_block(&d, "Symbol").unwrap();
    let p = |node: i32| &m.parameters.iter().find(|p| p.node == node).unwrap().kind;
    assert!(matches!(p(15), ParameterKind::Rotation { label, values, .. } if label == "Angle1" && values.maximum > 6.0));
    assert!(
        matches!(p(20), ParameterKind::Flip { label, states, .. } if label == "Flip state1" && states == &["Not flipped".to_string(), "Flipped".to_string()])
    );
    let a = |node: i32| m.actions.iter().find(|a| a.node == node).unwrap();
    assert!(matches!(&a(19).kind, ActionKind::Rotate { base } if base.dependent));
    assert_eq!(a(19).connections.last(), Some(&(15, "AngleDelta".to_string())));
    assert!(matches!(a(25).kind, ActionKind::Flip));
    assert_eq!(a(25).connections.len(), 4);
}

#[test]
fn an_angle_turns_and_a_flip_mirrors_what_their_actions_hold() {
    let (mut d, r) = symbol();
    assert_eq!(props(&d, r), pairs(&[("Angle1", "0"), ("Flip state1", "Not flipped"), ("Position1 X", "0.0000"), ("Position1 Y", "3.0000")]));

    // 30°: the line, the attribute and Position1 turn about the origin; the text turns with them.
    set_property(&mut d, r, "angle1", "30").unwrap();
    let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let (line, at, rot) = shown(&d, r);
    assert!(near(line, (-3.0 * s, 3.0 * c)), "{line:?}");
    assert!(near(at, (c - 2.0 * s, s + 2.0 * c)), "{at:?}");
    assert!((rot - 120.0).abs() < 1e-9);
    assert_eq!(props(&d, r), pairs(&[("Angle1", "30"), ("Flip state1", "Not flipped"), ("Position1 X", "-1.5000"), ("Position1 Y", "2.5981")]));
    // The reference's attribute follows its definition.
    let Some(EntityKind::Insert(i)) = d.entity(r).map(|e| &e.kind) else { panic!("no reference") };
    assert!(near(i.attribs[0].text.insert.xy(), (100.0 + at.x, 100.0 + at.y)));

    // Flipped: mirrored across the reflection line, which turned with them (now at 30°); the
    // text keeps reading left to right.
    set_property(&mut d, r, "Flip state1", "Flipped").unwrap();
    let (s2, c2) = (60f64.to_radians().sin(), 60f64.to_radians().cos());
    let mirror = |p: Vec2| (p.x * c2 + p.y * s2, p.x * s2 - p.y * c2);
    let (line2, at2, rot) = shown(&d, r);
    assert!(near(line2, mirror(line)), "{line2:?}");
    assert!(near(at2, mirror(at)), "{at2:?}");
    assert!((rot - 120.0).abs() < 1e-9, "readable");
    assert_eq!(props(&d, r)[1..], pairs(&[("Flip state1", "Flipped"), ("Position1 X", "1.5000"), ("Position1 Y", "-2.5981")]));

    // Both survive a save.
    let again = read_dxf(write_dxf(&d).as_bytes()).unwrap();
    assert_eq!(props(&again, r), props(&d, r));
    // Back to not flipped: back to the turned geometry.
    set_property(&mut d, r, "Flip state1", "0").unwrap();
    assert!(near(shown(&d, r).1, (at.x, at.y)));
}

#[test]
fn rotation_and_flip_grips() {
    let (mut d, r) = symbol();
    set_property(&mut d, r, "Angle1", "30").unwrap();
    let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let g = grips(&d, r);
    let rot = g.iter().find(|g| g.shape == GripShape::Rotation).unwrap();
    assert_eq!((rot.node, rot.part), (15, GripPart::End));
    assert!(near(rot.at, (100.0 + 2.0 * c, 100.0 + 2.0 * s)), "at the radius' end: {:?}", rot.at);
    let flip = g.iter().find(|g| g.shape == GripShape::Flip).unwrap();
    assert_eq!((flip.node, flip.part, flip.choices.len()), (20, GripPart::Base, 2));
    // The flip grip object at (-2, 0) turned with the reflection line.
    assert!(near(flip.at, (100.0 - 2.0 * c, 100.0 - 2.0 * s)), "{:?}", flip.at);

    // Dragging the rotation grip straight up from the base: 90°.
    drag_grip(&mut d, r, 15, GripPart::End, Vec2::new(100.0, 105.0)).unwrap();
    assert_eq!(props(&d, r)[0].1, "90");
    // A flip grip is clicked, not dragged; the base of the angle is not a point to aim at.
    assert!(drag_grip(&mut d, r, 20, GripPart::Base, Vec2::new(90.0, 90.0)).is_err());
    assert!(drag_grip(&mut d, r, 15, GripPart::End, Vec2::new(100.0, 100.0)).is_err());
}

#[test]
fn bad_angles_and_states_change_nothing() {
    let (mut d, r) = symbol();
    let before = d.clone();
    for (name, value) in [("Angle1", "steep"), ("Angle1", "inf"), ("Flip state1", "Sideways"), ("Flip state1", "2")] {
        assert!(set_property(&mut d, r, name, value).is_err(), "{name}={value}");
    }
    assert_eq!(d, before);
}
