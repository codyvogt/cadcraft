//! Saving a dynamic block under another name (BSAVEAS): the copy gets its own evaluation graph,
//! made from the original's objects with new handles, and its own copies of the block's objects
//! (no two blocks share a handle), so references to either change as AutoCAD would change them.

use std::collections::{HashMap, HashSet};

use crate::foreign::{ForeignExtra, ForeignObject};
use crate::{Drawing, EntityKind, EntityStore, Handle, RawTag};

use super::write::{dict_entry, guid, record_handle};
use super::{MAX_NODES, dynamic_block};

/// Most objects of one block's dynamic data that are copied.
const MAX_COPIED: usize = 8 * MAX_NODES;

/// Group codes whose values are handles: the object's own (5), owners and pointers (330 to 369)
/// and 390 to 399.
fn is_handle_code(code: i32) -> bool {
    code == 5 || (330..=369).contains(&code) || (390..=399).contains(&code)
}

/// The owner of an object: its first 330 group outside the `{ACAD_REACTORS` ... `}` groups.
fn owner_of(o: &ForeignObject) -> Option<Handle> {
    let mut inside = false;
    for t in &o.tags {
        match t.code {
            102 => inside = t.value.starts_with('{'),
            330 if !inside => return Handle::parse_hex(&t.value),
            _ => {}
        }
    }
    None
}

/// `root` and the objects it owns, directly or not (at most [`MAX_COPIED`]).
fn owned_subtree(d: &Drawing, root: Handle) -> Vec<Handle> {
    let mut children: HashMap<Handle, Vec<Handle>> = HashMap::new();
    for o in &d.foreign.objects {
        if let Some(owner) = owner_of(o) {
            children.entry(owner).or_default().push(o.handle);
        }
    }
    let mut out = vec![root];
    let mut seen: HashSet<Handle> = HashSet::from([root]);
    let mut at = 0;
    while let Some(h) = out.get(at).copied() {
        at += 1;
        for c in children.get(&h).into_iter().flatten() {
            if out.len() < MAX_COPIED && seen.insert(*c) {
                out.push(*c);
            }
        }
    }
    out
}

/// The handle `h` names, as `map` has it.
fn mapped(map: &HashMap<Handle, Handle>, value: &str) -> Option<Handle> {
    Handle::parse_hex(value).and_then(|h| map.get(&h)).copied()
}

/// Every handle in `tags` that `map` has, as the new one.
fn retarget(tags: &mut [RawTag], map: &HashMap<Handle, Handle>) {
    for t in tags.iter_mut().filter(|t| is_handle_code(t.code)) {
        if let Some(n) = mapped(map, &t.value) {
            t.value = n.hex();
        }
    }
}

/// The objects of `store` again with new handles (added to `map`): each one's element tag names
/// the object itself, and a hatch's sources are the new copies.
fn rehandle(d: &mut Drawing, store: &EntityStore, map: &mut HashMap<Handle, Handle>) -> EntityStore {
    for e in store.iter() {
        let n = d.new_handle();
        map.insert(e.handle, n);
    }
    let mut out = EntityStore::new();
    for e in store.iter() {
        let mut c = (**e).clone();
        c.handle = map.get(&e.handle).copied().unwrap_or(e.handle);
        for t in c.common.xdata.iter_mut().filter(|t| t.code == 1005) {
            if let Some(n) = mapped(map, &t.value) {
                t.value = n.hex();
            }
        }
        if let EntityKind::Hatch(h) = &mut c.kind {
            for l in &mut h.loops {
                l.sources = l.sources.iter().map(|s| map.get(s).copied().unwrap_or(*s)).collect();
            }
        }
        out.push(c);
    }
    out
}

/// The kept extras of the objects of `store` (reactors within the block; not their extension
/// dictionaries), for their new handles.
fn entity_extras(d: &Drawing, store: &EntityStore, map: &HashMap<Handle, Handle>) -> Vec<(Handle, ForeignExtra)> {
    store
        .iter()
        .filter_map(|e| {
            let mut x = d.foreign.entities.get(&e.handle)?.clone();
            x.xdict = None;
            x.reactors = x.reactors.iter().filter_map(|r| map.get(r).copied()).collect();
            Some((map.get(&e.handle).copied()?, x))
        })
        .collect()
}

/// Block record xdata `xdata` with the dynamic block's true name and GUID replaced.
fn rename_in_xdata(xdata: &mut [RawTag], name: &str, guid: &str) {
    let mut app = String::new();
    for t in xdata {
        match t.code {
            1001 => app = t.value.to_ascii_uppercase(),
            1000 if app == "ACDBDYNAMICBLOCKTRUENAME" => t.value = name.to_string(),
            1000 if app == "ACDBDYNAMICBLOCKGUID" => t.value = guid.to_string(),
            _ => {}
        }
    }
}

/// Make block `to` a copy of dynamic block `from` (BSAVEAS): `store`, the objects it is saved
/// with, get new handles; the copy has its own evaluation graph and objects, made from the
/// original's with new handles, and its own true name and GUID. Nothing changes when `from` has no
/// dynamic data or is `to`.
pub fn copy_definition(d: &mut Drawing, from: &str, to: &str, store: &mut EntityStore) {
    let (from_name, to_name) = (from.trim().to_ascii_uppercase(), to.trim().to_ascii_uppercase());
    let key = |n: &str| ("BLOCK_RECORD".to_string(), n.to_string());
    if from_name == to_name || dynamic_block(d, from).is_none() {
        return;
    }
    let Some(source) = d.foreign.records.get(&key(&from_name)).cloned() else { return };
    let Some(xdict) = source.xdict else { return };
    let graph = dict_entry(d, xdict, "ACAD_ENHANCEDBLOCK");
    let owned = owned_subtree(d, xdict);
    let owned_set: HashSet<Handle> = owned.iter().copied().collect();
    let mut map: HashMap<Handle, Handle> = owned.iter().map(|h| (*h, d.new_handle())).collect();
    let old_record = d.foreign.targets.iter().find(|(_, (k, n))| k == "BLOCK_RECORD" && *n == from_name).map(|(h, _)| *h);
    let new_record = record_handle(d, &to_name);
    map.extend(old_record.map(|h| (h, new_record)));
    let entities = rehandle(d, store, &mut map);
    let extras = entity_extras(d, store, &map);
    let copies: Vec<ForeignObject> = d
        .foreign
        .objects
        .iter()
        .filter(|o| owned_set.contains(&o.handle))
        .map(|o| {
            let mut c = o.clone();
            retarget(&mut c.tags, &map);
            c.handle = map.get(&o.handle).copied().unwrap_or(o.handle);
            c
        })
        .collect();
    let mut record = source;
    record.xdict = map.get(&xdict).copied();
    let new_graph = graph.and_then(|g| map.get(&g).copied()).unwrap_or(new_record);
    rename_in_xdata(&mut record.xdata, to.trim(), &guid(to.trim(), new_graph));
    let f = std::sync::Arc::make_mut(&mut d.foreign);
    f.objects.extend(copies);
    f.records.insert(key(&to_name), record);
    f.entities.extend(extras);
    *store = entities;
}
