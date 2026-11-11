//! Typed, read-only views over the K2 XML model.
//!
//! The container/format layer is fully recovery-backed. The program layer is
//! intentionally permissive: Kontakt stores most state as `<V name value>`
//! pairs, and not every parameter name has been verified against a
//! zone-bearing file yet. Accessors return `Option` rather than guessing.

use crate::xml::Element;

/// `<V name="..." value="..."/>` pairs found under an element's `<Parameters>`.
#[derive(Debug, Clone, Default)]
pub struct Params<'a> {
    values: Vec<(&'a str, &'a str)>,
}

impl<'a> Params<'a> {
    pub fn from_element(el: &'a Element) -> Self {
        let mut values = Vec::new();
        if let Some(params) = el.child("Parameters") {
            for v in params.children_named("V") {
                if let (Some(name), Some(value)) = (v.attr("name"), v.attr("value")) {
                    values.push((name, value));
                }
            }
        }
        Params { values }
    }

    pub fn get(&self, name: &str) -> Option<&'a str> {
        self.values
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, v)| *v)
    }

    pub fn f32(&self, name: &str) -> Option<f32> {
        self.get(name)?.trim().parse().ok()
    }

    pub fn i32(&self, name: &str) -> Option<i32> {
        self.get(name)?.trim().parse().ok()
    }

    pub fn bool(&self, name: &str) -> Option<bool> {
        match self.get(name)?.trim() {
            "yes" | "true" | "1" => Some(true),
            "no" | "false" | "0" => Some(false),
            _ => None,
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (&'a str, &'a str)> + '_ {
        self.values.iter().copied()
    }

    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

/// The `<K2_Container>` root.
#[derive(Debug, Clone, Copy)]
pub struct Document<'a> {
    pub root: &'a Element,
}

impl<'a> Document<'a> {
    pub fn new(root: &'a Element) -> Self {
        Document { root }
    }

    pub fn name(&self) -> &'a str {
        self.root.attr("name").unwrap_or("")
    }

    pub fn kind(&self) -> Option<&'a str> {
        self.root.attr("type")
    }

    pub fn version(&self) -> Option<&'a str> {
        self.root.attr("version")
    }

    pub fn programs(&self) -> Vec<Program<'a>> {
        self.root
            .child("Programs")
            .map(|p| {
                p.children_named("K2_Program")
                    .map(|element| Program { element })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Program<'a> {
    pub element: &'a Element,
}

impl<'a> Program<'a> {
    pub fn name(&self) -> &'a str {
        self.element.attr("name").unwrap_or("")
    }

    pub fn index(&self) -> Option<u32> {
        self.element.attr("index")?.parse().ok()
    }

    pub fn version(&self) -> Option<&'a str> {
        self.element.attr("version")
    }

    pub fn params(&self) -> Params<'a> {
        Params::from_element(self.element)
    }

    pub fn groups(&self) -> Vec<Group<'a>> {
        self.element
            .child("Groups")
            .map(|g| {
                g.children_named("K2_Group")
                    .map(|element| Group { element })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn zones(&self) -> Vec<Zone<'a>> {
        self.element
            .child("Zones")
            .map(|z| {
                z.children_named("K2_Zone")
                    .map(|element| Zone { element })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Group<'a> {
    pub element: &'a Element,
}

impl<'a> Group<'a> {
    pub fn name(&self) -> &'a str {
        self.element.attr("name").unwrap_or("")
    }

    pub fn index(&self) -> Option<u32> {
        self.element.attr("index")?.parse().ok()
    }

    pub fn params(&self) -> Params<'a> {
        Params::from_element(self.element)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Zone<'a> {
    pub element: &'a Element,
}

impl<'a> Zone<'a> {
    pub fn index(&self) -> Option<u32> {
        self.element.attr("index")?.parse().ok()
    }

    pub fn params(&self) -> Params<'a> {
        Params::from_element(self.element)
    }

    pub fn key_range(&self) -> Option<(i32, i32)> {
        let p = self.params();
        Some((p.i32("lowKey")?, p.i32("highKey")?))
    }

    pub fn velocity_range(&self) -> Option<(i32, i32)> {
        let p = self.params();
        Some((p.i32("lowVelocity")?, p.i32("highVelocity")?))
    }

    /// Best-effort scan for sample references. Returns raw strings; resolving
    /// them to files is the content engine's job.
    pub fn sample_refs(&self) -> Vec<&'a str> {
        let mut out = Vec::new();
        collect_sample_refs(self.element, &mut out);
        out
    }
}

fn collect_sample_refs<'a>(el: &'a Element, out: &mut Vec<&'a str>) {
    for (key, value) in &el.attrs {
        let key = key.to_ascii_lowercase();
        if key.contains("sample") || key == "file" || key == "path" {
            out.push(value);
        }
    }
    if el.name == "V" {
        if let (Some(name), Some(value)) = (el.attr("name"), el.attr("value")) {
            let name = name.to_ascii_lowercase();
            if name.contains("sample") || name.contains("file") || name.contains("path") {
                out.push(value);
            }
        }
    }
    for child in &el.children {
        collect_sample_refs(child, out);
    }
}
