//! Checks that a gltfpack output still has what an app looks things up by (SPEC-FIX-001 FR-005):
//! the scenes, every named node under the same named parents, every named material, and the
//! `extras` on each of them.

use std::collections::BTreeMap;

use crate::gltf::Document;

/// Named things of one document: a key (`Door/Handle` for nodes, the name for materials) and the
/// `extras` of each, as JSON text, sorted so duplicates compare as a multiset.
type Names = BTreeMap<String, Vec<String>>;

fn extras_text(extras: Option<&serde_json::Value>) -> String {
    extras.map(ToString::to_string).unwrap_or_default()
}

/// Every named node keyed by the names of its named ancestors and its own (`Door/Handle`).
/// Unnamed nodes in between are left out: gltfpack adds and removes those freely.
fn named_nodes(doc: &Document) -> Names {
    let mut parent = vec![None; doc.nodes.len()];
    for (i, node) in doc.nodes.iter().enumerate() {
        for &child in &node.children {
            if let Some(slot) = parent.get_mut(child) {
                *slot = Some(i);
            }
        }
    }
    let mut out = Names::new();
    for (i, node) in doc.nodes.iter().enumerate() {
        let Some(name) = &node.name else { continue };
        let mut path = vec![name.as_str()];
        let mut at = parent[i];
        // a broken file can loop; no real chain is longer than the node list
        for _ in 0..doc.nodes.len() {
            let Some(p) = at else { break };
            if let Some(n) = &doc.nodes[p].name {
                path.push(n);
            }
            at = parent[p];
        }
        path.reverse();
        out.entry(path.join("/"))
            .or_default()
            .push(extras_text(node.extras.as_ref()));
    }
    for list in out.values_mut() {
        list.sort();
    }
    out
}

fn named_materials(doc: &Document) -> Names {
    let mut out = Names::new();
    for m in &doc.materials {
        if let Some(name) = &m.name {
            out.entry(name.clone())
                .or_default()
                .push(extras_text(m.extras.as_ref()));
        }
    }
    for list in out.values_mut() {
        list.sort();
    }
    out
}

fn compare(what: &str, before: &Names, after: &Names) -> Result<(), String> {
    for (key, extras) in before {
        match after.get(key) {
            None => return Err(format!("{what} `{key}` is missing from the output")),
            Some(got) if got.len() != extras.len() => {
                return Err(format!(
                    "{what} `{key}`: {} in the input, {} in the output",
                    extras.len(),
                    got.len()
                ));
            }
            Some(got) if got != extras => {
                return Err(format!("{what} `{key}` lost or changed its extras"));
            }
            Some(_) => {}
        }
    }
    let before_count: usize = before.values().map(Vec::len).sum();
    let after_count: usize = after.values().map(Vec::len).sum();
    if before_count != after_count {
        return Err(format!(
            "{before_count} {what}s in the input, {after_count} in the output"
        ));
    }
    Ok(())
}

/// Whether `after` keeps what `before` had. The error says what is missing.
///
/// # Errors
/// The first difference found.
pub fn same_structure(before: &Document, after: &Document) -> Result<(), String> {
    if before.scenes.len() != after.scenes.len() {
        return Err(format!(
            "{} scenes in the input, {} in the output",
            before.scenes.len(),
            after.scenes.len()
        ));
    }
    compare("named node", &named_nodes(before), &named_nodes(after))?;
    compare(
        "named material",
        &named_materials(before),
        &named_materials(after),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(json: &str) -> Document {
        Document::parse(json.as_bytes()).unwrap_or_default()
    }

    const DOOR: &str = r#"{"asset":{"version":"2.0"},"scenes":[{"nodes":[0]}],
        "nodes":[{"name":"Door","children":[1],"extras":{"id":7}},{"name":"Handle"}],
        "materials":[{"name":"wood","extras":{"sku":"A1"}}]}"#;

    #[test]
    fn unnamed_nodes_in_between_do_not_matter() {
        let after = doc(r#"{"asset":{"version":"2.0"},"scenes":[{"nodes":[1]}],
            "nodes":[{"mesh":0},{"name":"Door","children":[2,0],"extras":{"id":7}},
                     {"children":[3]},{"name":"Handle"}],
            "materials":[{"name":"wood","extras":{"sku":"A1"}}]}"#);
        assert_eq!(same_structure(&doc(DOOR), &after), Ok(()));
    }

    #[test]
    fn a_lost_name_parent_or_extra_is_reported() {
        let renamed = DOOR.replace("Handle", "Knob");
        let err = same_structure(&doc(DOOR), &doc(&renamed));
        assert!(err.is_err_and(|e| e.contains("Door/Handle")));

        let flat = DOOR.replace(r#""children":[1],"#, "");
        assert!(same_structure(&doc(DOOR), &doc(&flat)).is_err());

        let no_extras = DOOR.replace(r#","extras":{"sku":"A1"}"#, "");
        let err = same_structure(&doc(DOOR), &doc(&no_extras));
        assert!(err.is_err_and(|e| e.contains("wood")));

        let two_scenes = DOOR.replace(r#""scenes":[{"nodes":[0]}]"#, r#""scenes":[{},{}]"#);
        assert!(same_structure(&doc(DOOR), &doc(&two_scenes)).is_err());
    }
}
