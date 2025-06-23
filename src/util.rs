use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use rnix::SyntaxNode;

pub(crate) fn value_parser_for_pathbuf(path_str: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path_str);
    if path.is_file() {
        Ok(path)
    } else {
        bail!(format!(
            "The specified [{}] path does not exist or is not a file",
            path_str
        ))
    }
}

pub(crate) fn syntax(content: String) -> SyntaxNode {
    let parse = rnix::Root::parse(&content);
    parse.syntax().clone_for_update()
}

pub(crate) fn parse_nix_file(file: &str) -> Result<SyntaxNode> {
    let content = fs::read_to_string(file).context(format!(
        "parse_nix_file Failed to read config file: {}",
        file
    ))?;
    Ok(syntax(content))
}

pub(crate) fn find_attr_inputs_node(root: &SyntaxNode, property: &str) -> Option<SyntaxNode> {
    for attr_set in root
        .children()
        .filter(|n| n.kind() == rnix::SyntaxKind::NODE_ATTR_SET)
    {
        for attr_path_value in attr_set
            .children()
            .filter(|n| n.kind() == rnix::SyntaxKind::NODE_ATTRPATH_VALUE)
        {
            if let Some(_) = attr_path_value
                .descendants()
                .find(|n| n.kind() == rnix::SyntaxKind::NODE_ATTRPATH && n.text() == property)
            {
                return Some(attr_path_value);
            }
        }
    }
    None
}
