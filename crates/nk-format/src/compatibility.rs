//! Structural notices. Presence does not establish whether a script is active.
use crate::{
    preset_chunk::{self, PresetChunk},
    xml::Element,
};

pub(crate) fn binary_issues(chunks: &[PresetChunk]) -> Vec<String> {
    fn scripts(chunks: &[PresetChunk]) -> bool {
        chunks
            .iter()
            .any(|c| c.id == preset_chunk::ID_PAR_SCRIPT || scripts(&c.children))
    }
    let mut issues = Vec::new();
    if scripts(chunks) {
        issues.push("This instrument contains script slots. KSP scripts and scripted controls are not executed; articulations and behavior may differ.".into());
    }
    issues
}

pub(crate) fn xml_issues(root: &Element) -> Vec<String> {
    fn scripts(e: &Element) -> bool {
        (e.name.to_ascii_lowercase().contains("script")
            && (!e.children.is_empty() || !e.text.trim().is_empty()))
            || e.children.iter().any(scripts)
    }
    let mut issues = Vec::new();
    if scripts(root) {
        issues.push("This instrument contains script data. KSP scripts and scripted controls are not executed; articulations and behavior may differ.".into());
    }
    fn effects(e: &Element) -> bool {
        matches!(
            e.name.as_str(),
            "InsertEffects" | "SendEffects" | "MasterEffects" | "Effects" | "Effect"
        ) && !e.children.is_empty()
            || e.children.iter().any(effects)
    }
    if effects(root) {
        issues.push("XML effect arrays are not played. Only supported XML group filter parameters are applied.".into());
    }
    issues
}

pub(crate) fn program_count(chunks: &[PresetChunk]) -> usize {
    chunks
        .iter()
        .map(|c| usize::from(c.id == preset_chunk::ID_PROGRAM) + program_count(&c.children))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_binary_script_slots_are_reported_and_programs_counted() {
        let script = PresetChunk {
            id: preset_chunk::ID_PAR_SCRIPT,
            ..Default::default()
        };
        let program = PresetChunk {
            id: preset_chunk::ID_PROGRAM,
            children: vec![script],
            ..Default::default()
        };
        let nested = PresetChunk {
            children: vec![program.clone(), program],
            ..Default::default()
        };
        assert_eq!(program_count(std::slice::from_ref(&nested)), 2);
        let issues = binary_issues(&[nested]);
        assert_eq!(issues.len(), 1);
        assert!(issues[0].contains("script slots") && issues[0].contains("not executed"));
    }
}
