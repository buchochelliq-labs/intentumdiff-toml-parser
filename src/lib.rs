//! TOML parser plugin — full-parse mode on tree-sitter-toml-ng (issue #48). TOML is a
//! KEYED config format: review identity lives in table headers (`[server]`,
//! `[[replicas]]`) and key paths, so tables are labeled by their dotted header and
//! pairs by their key — the same identity model the json/yaml keyed profile uses.

use intentumdiff_plugin_sdk::{
    cst::CstNode,
    ts_convert::{convert_semantic, node_to_cst},
    tree::SemanticNodeBuilder,
};

wit_bindgen::generate!({
    path: "wit/plugin.wit",
    world: "parser-plugin",
});

use crate::exports::intentumdiff::plugin::parser::ExamplePair;
use crate::exports::intentumdiff::plugin::parser::Guest;
use crate::exports::intentumdiff::plugin::parser::LanguageInfoRecord;
use crate::exports::intentumdiff::plugin::parser::ParserMode;

const LANGUAGE_ID: &str = "toml";
const PLUGIN_METADATA: &str = include_str!("../plugin_metadata.info");

const DEFAULT_OLD: &str = "title = \"demo\"\n\n[server]\nhost = \"0.0.0.0\"\nport = 8080\n";
const DEFAULT_NEW: &str =
    "title = \"demo\"\n\n[server]\nhost = \"0.0.0.0\"\nport = 9090\nworkers = 4\n";

// Grammar node types that carry review meaning. Brackets, equals signs, dots and
// comments are dropped (not listed, no semantic children).
const SEMANTIC_TYPES: &[&str] = &[
    "document",
    "table",
    "table_array_element",
    "pair",
    "array",
    "inline_table",
    "string",
    "integer",
    "float",
    "boolean",
    "offset_date_time",
    "local_date_time",
    "local_date",
    "local_time",
];

fn is_semantic(node_type: &str) -> bool {
    SEMANTIC_TYPES.contains(&node_type)
}

fn language_info_for(ids: Vec<String>) -> Vec<LanguageInfoRecord> {
    let metadata = intentumdiff_plugin_sdk::metadata::parse_plugin_metadata(PLUGIN_METADATA);
    ids.into_iter()
        .map(|language_id| {
            let info = metadata.language_or_default(&language_id);
            LanguageInfoRecord {
                language_id: info.language_id,
                language_name: info.language_name,
                language_short_name: info.language_short_name,
                monaco_language: info.monaco_language,
                default_filename: info.default_filename,
                language_file_extensions: info.language_file_extensions,
                author: metadata.author().to_string(),
                plugin_version: metadata.plugin_version().to_string(),
                last_updated: metadata.last_updated().to_string(),
            }
        })
        .collect()
}

fn basename(path: &str) -> &str {
    path.rsplit(|ch| ch == '/' || ch == '\\')
        .next()
        .unwrap_or(path)
}

fn detect_language_impl(filename: &str, _content: &str) -> String {
    let name = basename(filename).to_lowercase();
    if name.ends_with(".toml") {
        LANGUAGE_ID.to_string()
    } else {
        String::new()
    }
}

/// A pair's key or a table's dotted header: the text of the first `bare_key`,
/// `quoted_key` or `dotted_key` child (before the `=` / inside the brackets).
fn key_text(node: &CstNode) -> Option<String> {
    fn find_key(node: &CstNode) -> Option<String> {
        if matches!(node.node_type.as_str(), "bare_key" | "quoted_key" | "dotted_key") {
            let text = node.text_or_empty().trim().trim_matches('"').trim_matches('\'');
            if !text.is_empty() {
                return Some(text.chars().take(120).collect());
            }
        }
        for child in &node.children {
            if let Some(text) = find_key(child) {
                return Some(text);
            }
        }
        None
    }
    find_key(node)
}

fn label_for(node: &CstNode) -> String {
    if node.is_leaf() {
        return node.text_or_empty().trim().chars().take(120).collect();
    }
    match node.node_type.as_str() {
        // Tables and array-of-table elements are identified by their header path.
        "table" | "table_array_element" => {
            key_text(node).unwrap_or_else(|| node.node_type.clone())
        }
        // A pair is identified by its key — the value stays a separate child node so
        // value edits pair as MODIFICATIONs under a stable key identity.
        "pair" => key_text(node).unwrap_or_else(|| node.node_type.clone()),
        "string" => {
            let text = node.text_or_empty();
            let trimmed = text.trim().trim_matches('"').trim_matches('\'');
            trimmed.chars().take(120).collect()
        }
        _ => node.node_type.clone(),
    }
}

fn parse_source(source: &str) -> Result<CstNode, String> {
    let mut parser = tree_sitter::Parser::new();
    let lang = tree_sitter_toml_ng::LANGUAGE.into();
    parser
        .set_language(&lang)
        .map_err(|_| "Failed to load TOML grammar".to_string())?;
    let tree = parser
        .parse(source, None)
        .ok_or_else(|| "tree-sitter failed to parse TOML".to_string())?;
    Ok(node_to_cst(tree.root_node(), source.as_bytes()))
}

fn process_impl(source: &str) -> String {
    let cst = match parse_source(source) {
        Ok(cst) => cst,
        Err(err) => return format!(r#"{{"error":"{}"}}"#, err),
    };
    let mut memo = std::collections::HashMap::new();
    let node = convert_semantic(&cst, "0", &mut memo, &is_semantic, &label_for).unwrap_or_else(|| {
        SemanticNodeBuilder::new("0", "document", LANGUAGE_ID, 0, 0, 0, 0, "0").build()
    });
    match serde_json::to_string(&node) {
        Ok(serialized) => serialized,
        Err(err) => format!(r#"{{"error":"Serialisation error: {}"}}"#, err),
    }
}

struct TomlParser;

impl Guest for TomlParser {
    fn get_parser_mode() -> ParserMode {
        ParserMode::FullParse
    }

    fn grammar_id() -> String {
        LANGUAGE_ID.to_string()
    }

    fn detect_language(filename: String, content: String) -> String {
        detect_language_impl(&filename, &content)
    }

    fn preprocess_source(source: String) -> String {
        source
    }

    fn example(_language: String) -> ExamplePair {
        ExamplePair {
            old: DEFAULT_OLD.to_string(),
            new: DEFAULT_NEW.to_string(),
        }
    }

    fn process(input: String, _language: String, _filename: String) -> String {
        process_impl(&input)
    }

    fn trivia_node_types() -> Vec<String> {
        vec![]
    }

    fn language_ids() -> Vec<String> {
        vec![LANGUAGE_ID.to_string()]
    }

    fn language_info() -> Vec<LanguageInfoRecord> {
        language_info_for(Self::language_ids())
    }

    fn priority() -> i32 {
        5
    }
}

export!(TomlParser);

#[cfg(test)]
mod tests {
    use super::*;
    use intentumdiff_plugin_sdk::tree::SemanticNode;

    fn labels_by_type(node: &SemanticNode, node_type: &str, out: &mut Vec<String>) {
        if node.node_type == node_type {
            out.push(node.label.clone());
        }
        for child in &node.children {
            labels_by_type(child, node_type, out);
        }
    }

    #[test]
    fn parser_mode_is_full_parse() {
        assert_eq!(TomlParser::get_parser_mode(), ParserMode::FullParse);
    }

    #[test]
    fn detects_toml_extension_and_filenames() {
        assert_eq!(detect_language_impl("config.toml", ""), LANGUAGE_ID);
        assert_eq!(detect_language_impl("sub/dir/Cargo.toml", ""), LANGUAGE_ID);
        assert_eq!(detect_language_impl("main.rs", ""), "");
    }

    #[test]
    fn tables_and_pairs_are_labeled_by_their_keys() {
        let parsed = process_impl(DEFAULT_NEW);
        intentumdiff_plugin_sdk::testing::assert_valid_json(&parsed, LANGUAGE_ID);
        let root: SemanticNode = serde_json::from_str(&parsed).unwrap();
        let mut tables = Vec::new();
        labels_by_type(&root, "table", &mut tables);
        assert_eq!(tables, vec!["server".to_string()], "tables: {tables:?}");
        let mut pairs = Vec::new();
        labels_by_type(&root, "pair", &mut pairs);
        assert!(pairs.contains(&"title".to_string()), "pairs: {pairs:?}");
        assert!(pairs.contains(&"port".to_string()), "pairs: {pairs:?}");
        assert!(pairs.contains(&"workers".to_string()), "pairs: {pairs:?}");
    }

    #[test]
    fn value_edit_changes_the_root_hash() {
        let old: SemanticNode = serde_json::from_str(&process_impl(DEFAULT_OLD)).unwrap();
        let new: SemanticNode = serde_json::from_str(&process_impl(DEFAULT_NEW)).unwrap();
        assert_ne!(old.structural_hash, new.structural_hash);
    }

    #[test]
    fn array_of_tables_labels_each_element_by_header() {
        let parsed = process_impl("[[replicas]]\nzone = \"eu\"\n\n[[replicas]]\nzone = \"us\"\n");
        let root: SemanticNode = serde_json::from_str(&parsed).unwrap();
        let mut elements = Vec::new();
        labels_by_type(&root, "table_array_element", &mut elements);
        assert_eq!(
            elements,
            vec!["replicas".to_string(), "replicas".to_string()],
            "elements: {elements:?}"
        );
    }
}
