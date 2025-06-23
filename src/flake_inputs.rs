use std::{fs::File, io::Write, path::PathBuf, process::Command};

use anyhow::{Ok, Result, bail};
use clap::Args;
use rnix::SyntaxNode;

use crate::util::{find_attr_inputs_node, parse_nix_file, syntax, value_parser_for_pathbuf};

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

            find_attr_inputs_node(&flake_nix_syntax,"inputs")
                .and_then(|attr_path_value| {
                    attr_path_value.children()
                        .find(|n| n.kind() == rnix::SyntaxKind::NODE_ATTR_SET)
                        .map(|attr_set| (attr_path_value, attr_set))
                })
                .map(|(attr_path_value, attr_set)| {
                    // 处理节点替换逻辑
                    let children: Vec<_> = attr_path_value.children_with_tokens().collect();
                    let range = children
                        .iter()
                        .position(|child| child.as_node() == Some(&attr_set))
                        .map(|start| start..start + 1)
                        .expect("node not found");
                    attr_path_value.splice_children(
                        range,
                        nix_expression_syntax.children_with_tokens().collect(),
                    );
                });
            // 将变更后的语法树写入flake.nix文件
            let mut flake_file = File::create(flake_nix_path)?;
            let _ = flake_file.write_all(flake_nix_syntax.to_string().as_bytes());

            Ok(())
        }
        _ => bail!("nix expression file path or flake file path not exits"),
    }
}
