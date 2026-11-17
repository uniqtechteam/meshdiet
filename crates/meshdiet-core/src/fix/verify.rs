//! Checks that a gltfpack output still has what an app looks things up by (SPEC-FIX-001 FR-005):
//! the scenes, every named node under the same named parents, every named material, the `extras`
//! on each of them, every extension the input used and every custom (`_NAME`) vertex attribute.

use std::collections::{BTreeMap, BTreeSet};

use crate::gltf::Document;

/// Named things of one document: a key (`Door/Handle` for nodes, the name for materials) and the
/// `extras` of each, as JSON text, sorted so duplicates compare as a multiset.
type Names = BTreeMap<String, Vec<String>>;

fn extras_text(extras: Option<&serde_json::Value>) -> String {
    extras.map(ToString::to_string).unwrap_or_default()
}

/// A name an app can look a node or material up by. gltfpack treats `""` as no name, and so does
/// this check.
fn name_of(name: Option<&String>) -> Option<&str> {
    name.map(String::as_str).filter(|n| !n.is_empty())
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
        let Some(name) = name_of(node.name.as_ref()) else {
            continue;
        };
        let mut path = vec![name];
        let mut at = parent[i];
        // a broken file can loop; no real chain is longer than the node list
        for _ in 0..doc.nodes.len() {
            let Some(p) = at else { break };
            if let Some(n) = name_of(doc.nodes[p].name.as_ref()) {
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
        if let Some(name) = name_of(m.name.as_ref()) {
            out.entry(name.to_owned())
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
    )?;
    if let Some(lost) = before.extensions_used.iter().find(|e| !after.uses(e)) {
        return Err(format!(
            "gltfpack dropped the {lost} extension, and the data that came with it"
        ));
    }
    let kept = custom_attributes(after);
    if let Some(lost) = custom_attributes(before).difference(&kept).next() {
        return Err(format!(
            "gltfpack dropped the custom vertex attribute {lost}"
        ));
    }
    Ok(())
}

/// Application-specific vertex attributes (`_INTENSITY`, `_BATCHID`): gltfpack only keeps the
/// attributes it knows.
fn custom_attributes(doc: &Document) -> BTreeSet<&str> {
    doc.meshes
        .iter()
        .flat_map(|m| &m.primitives)
        .flat_map(|p| p.attributes.keys())
        .map(String::as_str)
        .filter(|a| a.starts_with('_'))
        .collect()
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

    #[test]
    fn empty_names_are_no_names() {
        // gltfpack drops `"name": ""` and empty leaf nodes, as it does for unnamed ones
        let before = doc(r#"{"asset":{"version":"2.0"},"scenes":[{"nodes":[0]}],
            "nodes":[{"name":"Root","children":[1]},{"name":"","children":[2,3]},
                     {"name":"Drive"},{"name":""}],
            "materials":[{"name":""}]}"#);
        let after = doc(r#"{"asset":{"version":"2.0"},"scenes":[{"nodes":[0]}],
            "nodes":[{"name":"Root","children":[1]},{"children":[2]},{"name":"Drive"}]}"#);
        assert_eq!(same_structure(&before, &after), Ok(()));
    }

    #[test]
    fn a_dropped_extension_or_custom_attribute_is_reported() {
        let points = |extensions: &str, attributes: &str| {
            doc(&format!(
                r#"{{"asset":{{"version":"2.0"}},"scenes":[{{"nodes":[0]}}],"nodes":[{{"mesh":0}}],
                "extensionsUsed":[{extensions}],
                "meshes":[{{"primitives":[{{"mode":0,"attributes":{{{attributes}}}}}]}}]}}"#
            ))
        };
        let before = points(
            r#""EXT_structural_metadata""#,
            r#""POSITION":0,"_INTENSITY":1"#,
        );
        let after = points(r#""EXT_meshopt_compression""#, r#""POSITION":0"#);
        let err = same_structure(&before, &after);
        assert!(err.is_err_and(|e| e.contains("EXT_structural_metadata")));
        let kept = points(
            r#""EXT_structural_metadata","EXT_meshopt_compression""#,
            r#""POSITION":0"#,
        );
        let err = same_structure(&before, &kept);
        assert!(err.is_err_and(|e| e.contains("_INTENSITY")));
        let all = points(
            r#""EXT_structural_metadata","EXT_meshopt_compression""#,
            r#""POSITION":0,"_INTENSITY":1"#,
        );
        assert_eq!(same_structure(&before, &all), Ok(()));
    }
}
