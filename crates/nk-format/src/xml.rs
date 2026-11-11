use crate::Error;
use quick_xml::{events::Event, Reader, XmlVersion};

/// A lossless-enough DOM for the K2 XML dialect.
///
/// Kontakt programs store almost everything in `<V name="..." value="..."/>`
/// elements, so attributes are first-class here and text is only accumulated
/// for completeness.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Element>,
    pub text: String,
}

impl Element {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name == name)
    }

    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Element> {
        self.children.iter().filter(move |c| c.name == name)
    }

    /// Depth-first search for the first descendant with `name` (excluding self).
    pub fn find(&self, name: &str) -> Option<&Element> {
        for child in &self.children {
            if child.name == name {
                return Some(child);
            }
            if let Some(found) = child.find(name) {
                return Some(found);
            }
        }
        None
    }

    /// Depth-first collection of every descendant with `name` (excluding self).
    pub fn find_all<'a>(&'a self, name: &'a str, out: &mut Vec<&'a Element>) {
        for child in &self.children {
            if child.name == name {
                out.push(child);
            }
            child.find_all(name, out);
        }
    }

    pub fn descendants<'a>(&'a self, name: &'a str) -> Vec<&'a Element> {
        let mut out = Vec::new();
        self.find_all(name, &mut out);
        out
    }

    pub fn parse(xml: &str) -> Result<Element, Error> {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(true);

        let mut stack: Vec<Element> = Vec::new();
        let mut root: Option<Element> = None;
        let mut nodes = 0usize;

        loop {
            match reader.read_event()? {
                Event::Start(e) => {
                    nodes += 1;
                    if stack.len() >= 128 || nodes > 262_144 {
                        return Err(Error::Malformed("XML nesting/node budget exceeded".into()));
                    }
                    stack.push(start_element(&e)?);
                }
                Event::Empty(e) => {
                    nodes += 1;
                    if nodes > 262_144 {
                        return Err(Error::Malformed("XML node budget exceeded".into()));
                    }
                    let el = start_element(&e)?;
                    attach(&mut stack, &mut root, el)?;
                }
                Event::End(_) => {
                    let el = stack
                        .pop()
                        .ok_or_else(|| Error::Malformed("unbalanced closing tag".into()))?;
                    attach(&mut stack, &mut root, el)?;
                }
                Event::Text(t) => {
                    if let Some(top) = stack.last_mut() {
                        top.text.push_str(&t.xml_content(XmlVersion::Implicit1_0));
                    }
                }
                Event::CData(t) => {
                    if let Some(top) = stack.last_mut() {
                        top.text.push_str(&t.into_inner());
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }

        if !stack.is_empty() {
            return Err(Error::Malformed(
                "unclosed elements at end of document".into(),
            ));
        }
        root.ok_or_else(|| Error::Malformed("empty document".into()))
    }
}

fn attach(stack: &mut [Element], root: &mut Option<Element>, el: Element) -> Result<(), Error> {
    match stack.last_mut() {
        Some(parent) => parent.children.push(el),
        None => {
            if root.is_some() {
                return Err(Error::Malformed("multiple root elements".into()));
            }
            *root = Some(el);
        }
    }
    Ok(())
}

fn start_element(e: &quick_xml::events::BytesStart<'_>) -> Result<Element, Error> {
    let mut attrs = Vec::new();
    for attr in e.attributes() {
        let attr = attr.map_err(|err| Error::Malformed(format!("attribute: {err}")))?;
        let key = attr.key.as_ref().to_string();
        let value = attr
            .normalized_value(XmlVersion::Implicit1_0)
            .map_err(|err| Error::Malformed(format!("attribute value: {err}")))?
            .into_owned();
        attrs.push((key, value));
    }
    Ok(Element {
        name: e.name().as_ref().to_string(),
        attrs,
        children: Vec::new(),
        text: String::new(),
    })
}
