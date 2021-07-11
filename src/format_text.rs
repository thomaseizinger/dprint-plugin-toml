use super::configuration::Configuration;
use super::parser::parse_items;
use dprint_core::configuration::resolve_new_line_kind;
use dprint_core::formatting::PrintOptions;
use dprint_core::types::ErrBox;
use std::cmp::Ordering;
use std::iter::Peekable;
use std::path::Path;
use taplo::rowan::SyntaxElement;
use taplo::syntax::{SyntaxKind, SyntaxNode};

pub fn format_text(file_path: &Path, text: &str, config: &Configuration) -> Result<String, ErrBox> {
    let node = parse_taplo(text)?;

    let node = if file_path.ends_with("Cargo.toml") {
        apply_cargo_toml_conventions(node)
    } else {
        node
    };

    Ok(dprint_core::formatting::format(
        || parse_items(node, text, config),
        config_to_print_options(text, config),
    ))
}

#[cfg(feature = "tracing")]
pub fn trace_file(text: &str, config: &Configuration) -> dprint_core::formatting::TracingResult {
    let node = parse_taplo(text).unwrap();

    dprint_core::formatting::trace_printing(
        || parse_items(node, text, config),
        config_to_print_options(text, config),
    )
}

fn parse_taplo(text: &str) -> Result<SyntaxNode, String> {
    let parse_result = taplo::parser::parse(text);

    if let Some(err) = parse_result.errors.get(0) {
        Err(
            dprint_core::formatting::utils::string_utils::format_diagnostic(
                Some((err.range.start().into(), err.range.end().into())),
                &err.message,
                text,
            ),
        )
    } else {
        Ok(parse_result.into_syntax())
    }
}

fn apply_cargo_toml_conventions(node: SyntaxNode) -> SyntaxNode {
    let node = node.clone_for_update(); // use mutable API to make updates easier
    let mut children = node.children().peekable();

    while let Some(child) = children.next() {
        if child.text() == "[package]" {
            let mut package_section = Section::new(&child, &mut children);
            package_section.apply_formatting_conventions(sort_cargo_package_section);
            package_section.insert(&node);
        }
        if child.text() == "[dependencies]" || child.text() == "[dev-dependencies]" {
            let mut package_section = Section::new(&child, &mut children);
            package_section.apply_formatting_conventions(|left, right| {
                left.entry_key_text().cmp(&right.entry_key_text())
            });
            package_section.insert(&node);
        }
    }

    node
}

fn sort_cargo_package_section(left: &SyntaxNode, right: &SyntaxNode) -> Ordering {
    match (
        left.entry_key_text().as_str(),
        right.entry_key_text().as_str(),
    ) {
        ("name", _) => Ordering::Less,
        ("version", "name") => Ordering::Greater,
        ("version", _) => Ordering::Less,
        ("description", _) => Ordering::Greater,
        (_, "name") => Ordering::Greater,
        (_, "version") => Ordering::Greater,

        (left, right) => left.cmp(right),
    }
}

#[derive(Debug)]
struct Section {
    nodes: Vec<SyntaxNode>,
    table_header_index: usize,
}

impl Section {
    fn new(
        table_header: &SyntaxNode,
        tree: &mut Peekable<impl Iterator<Item = SyntaxNode>>,
    ) -> Self {
        let mut nodes = vec![];

        while let Some(entry) = tree.next_if(|child| child.kind() == SyntaxKind::ENTRY) {
            nodes.push(entry);
        }

        Self {
            nodes,
            table_header_index: table_header.index(),
        }
    }

    fn apply_formatting_conventions(
        &mut self,
        cmp: impl FnMut(&SyntaxNode, &SyntaxNode) -> Ordering,
    ) {
        self.nodes.sort_by(cmp);
    }

    fn insert(self, node: &SyntaxNode) {
        let start = self.table_header_index + 1;
        let end = start + self.nodes.len();

        node.splice_children(
            start..end,
            self.nodes.into_iter().map(SyntaxElement::Node).collect(),
        )
    }
}

fn config_to_print_options(text: &str, config: &Configuration) -> PrintOptions {
    PrintOptions {
        indent_width: config.indent_width,
        max_width: config.line_width,
        use_tabs: config.use_tabs,
        new_line_text: resolve_new_line_kind(text, config.new_line_kind),
    }
}

trait SyntaxNodeExt {
    fn entry_key_text(&self) -> String;
}

impl SyntaxNodeExt for SyntaxNode {
    fn entry_key_text(&self) -> String {
        let key = self
            .children()
            .find(|child| child.kind() == SyntaxKind::KEY)
            .expect("ENTRY should contain KEY");

        let ident = key
            .children_with_tokens()
            .find_map(|child| match child {
                SyntaxElement::Token(token) if token.kind() == SyntaxKind::IDENT => Some(token),
                _ => None,
            })
            .expect("KEY should contain IDENT");

        ident.to_string()
    }
}
