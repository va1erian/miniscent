//! What a block reads, in words. Blocks connect by position with no wires, so the
//! page has to say what it connected: a shape below another block draws over that
//! block's image, not over its own background, and nothing else would tell.

use mscent_texgen::ops::{Role, Value};

use crate::model::texdoc::TexDoc;

/// The name a `store` keeps or a `load` reads, from its text parameter.
pub fn slot_name(doc: &TexDoc, block: usize) -> Option<&str> {
    let b = doc
        .recipe()
        .blocks
        .get(block)
        .filter(|b| matches!(b.spec.role, Role::Load | Role::Store))?;
    b.params.values().iter().find_map(|v| match v {
        Value::Text(t) => Some(t.as_str()),
        _ => None,
    })
}

/// One line on what block `block` reads, as the page compiled.
pub fn describe(doc: &TexDoc, block: usize) -> String {
    let Some(b) = doc.recipe().blocks.get(block) else {
        return String::new();
    };
    let Ok(graph) = doc.page() else {
        return "Inputs show once the page compiles".to_string();
    };
    if let Some(name) = slot_name(doc, block).filter(|_| b.spec.role == Role::Load) {
        return format!("reads \"{name}\"");
    }
    let sources = graph.inputs_of(block);
    if sources.is_empty() {
        return if b.spec.inputs.max == 0 {
            "reads no input".to_string()
        } else {
            "reads no input: draws on its own background".to_string()
        };
    }
    let names: Vec<&str> = sources
        .iter()
        .filter_map(|&s| doc.recipe().blocks.get(s))
        .map(|s| s.spec.name)
        .collect();
    let plural = if names.len() == 1 { "" } else { "s" };
    format!("reads {} input{plural}: {}", names.len(), names.join(", "))
}

#[cfg(test)]
mod tests {
    use mscent_texgen::recipe;

    use super::*;

    fn doc(text: &str) -> TexDoc {
        let recipe = recipe::read(format!("miniscent-recipe 1\nsize 16 16\n{text}").as_bytes())
            .expect("reads");
        TexDoc::open("t", recipe, 40)
    }

    #[test]
    fn a_shape_says_whether_it_draws_over_the_block_above() {
        let d = doc("op 0 0 1 noise\nop 0 1 1 rect\nop 1 0 1 rect\nop 0 2 1 out-albedo\n");
        assert_eq!(describe(&d, 1), "reads 1 input: noise");
        assert_eq!(
            describe(&d, 2),
            "reads no input: draws on its own background"
        );
        assert_eq!(describe(&d, 0), "reads no input");
        assert_eq!(describe(&d, 3), "reads 1 input: rect");
    }

    #[test]
    fn a_merge_lists_both_and_a_load_names_what_it_reads() {
        let d = doc(
            "op 0 0 1 noise\nop 1 0 1 flat\nop 0 1 2 merge\nop 0 2 1 store name=a\n\
             op 3 0 1 load name=a\n",
        );
        assert_eq!(describe(&d, 2), "reads 2 inputs: noise, flat");
        assert_eq!(describe(&d, 4), "reads \"a\"");
        assert_eq!(slot_name(&d, 4), Some("a"));
        assert_eq!(slot_name(&d, 3), Some("a"), "a store has its name too");
        assert_eq!(slot_name(&d, 0), None);
    }

    #[test]
    fn a_page_with_an_error_does_not_pretend() {
        let d = doc("op 0 0 1 merge\n");
        assert!(describe(&d, 0).contains("compiles"), "{}", describe(&d, 0));
        assert_eq!(describe(&d, 9), "");
    }
}
