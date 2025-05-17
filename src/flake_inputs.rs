use std::{fs::{self, File}, io::Write, path::PathBuf, process::Command};

use anyhow::{Context, Ok, Result, bail};
use clap::Args;
use rnix::SyntaxNode;

#[derive(Args)]
pub(crate) struct FlakeInputsArgs {
    #[arg(
        short,
        long,
        help = "nix expression path",
        default_value = "./lib/select_device_build_inputs.nix",
        value_parser = value_parser_for_pathbuf
    )]
    pub(crate) expression_path: PathBuf,

    #[arg(
        short,
        long,
        help = "flake.nix path",
        default_value = "./flake.nix",
        value_parser = value_parser_for_pathbuf
    )]
    pub(crate) flake_path: PathBuf,
}

fn value_parser_for_pathbuf(path_str: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path_str);
    if path.is_file() {
        Ok(path)
    } else {
        bail!(format!("The specified [{}] path does not exist or is not a file",path_str))
    }
}

fn parse_nix_file(file: &str) -> Result<SyntaxNode> {
    let content = fs::read_to_string(file).context(format!(
        "parse_nix_file Failed to read config file: {}",
        file
    ))?;
    Ok(syntax(content))
}

fn syntax(content: String) -> SyntaxNode {
    let parse = rnix::Root::parse(&content);
    parse.syntax().clone_for_update()
}

fn find_attr_inputs_node(root: &SyntaxNode) -> Option<SyntaxNode> {
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
                .find(|n| n.kind() == rnix::SyntaxKind::NODE_ATTRPATH && n.text() == "inputs")
            {
                return Some(attr_path_value);
            }
        }
    }
    None
}

fn nix_eval_content(file: &str) -> Result<String> {
    let stdout = Command::new("nix")
        .arg("eval")
        .arg("--file")
        .arg(file)
        .output()?
        .stdout;
    Ok(format!("{}", String::from_utf8_lossy(&stdout)))
}

pub(crate) fn replace_inputs(nix_expression_path: PathBuf, flake_nix_path: PathBuf) -> Result<()> {
    match (nix_expression_path.to_str(), flake_nix_path.to_str()) {
        (Some(nix_expression_path_str), Some(flake_nix_path_str)) => {
            let nix_expression_content = nix_eval_content(nix_expression_path_str)?;
            let nix_expression_syntax = syntax(nix_expression_content);
            
            let flake_nix_syntax = parse_nix_file(flake_nix_path_str)?;

            if let Some(attr_path_value) = find_attr_inputs_node(&flake_nix_syntax) {
                if let Some(attr_set) = attr_path_value
                    .children()
                    .find(|n| n.kind() == rnix::SyntaxKind::NODE_ATTR_SET)
                {
                    let children: Vec<_> = attr_path_value.children_with_tokens().collect();
                    let range = children
                        .iter()
                        .position(|child| child.as_node() == Some(&attr_set))
                        .map(|start| start..start + 1)
                        .expect("找不到目标索引");
                    attr_path_value.splice_children(
                        range,
                        nix_expression_syntax.children_with_tokens().collect(),
                    );
                    println!("{}", attr_set.to_string());
                }
            }
            println!("{}", flake_nix_syntax.to_string());

            // 将变更后的语法树写入flake.nix文件
            let mut flake_file = File::create(flake_nix_path)?;
            let _ = flake_file.write_all(flake_nix_syntax.to_string().as_bytes());

            Ok(())
        }
        _ => bail!("nix expression file path or flake file path not exits"),
    }
}
