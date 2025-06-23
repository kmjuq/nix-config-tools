use std::{env, fs::File, io::Write, path::PathBuf};

use anyhow::{Ok, Result, bail};
use clap::Args;
use rnix::{SyntaxNode, ast};

use crate::{
    util::{find_attr_inputs_node, parse_nix_file, value_parser_for_pathbuf},
};

#[derive(Args)]
pub(crate) struct FlakeHomeArgs {
    #[arg(
        short,
        long,
        help = "self attribute set path",
        default_value = "./self.nix",
        value_parser = value_parser_for_pathbuf
    )]
    pub(crate) self_var_path: PathBuf,

    #[arg(
        short,
        long,
        help = "self attribute set property",
        default_value = "flakeHome"
    )]
    pub(crate) property: String,

    #[arg(short, long, help = "flake home dir path", required = false, value_parser = value_parser_for_pathbuf)]
    pub(crate) flake_home_dir: Option<PathBuf>,
}

pub(crate) fn replace_flake_home(
    self_var_path: PathBuf,
    property: String,
    flake_home_dir: Option<PathBuf>,
) -> Result<()> {
    match self_var_path.to_str() {
        Some(self_var_path_str) => {
            let self_nix_syntax = parse_nix_file(self_var_path_str)?;
            find_attr_inputs_node(&self_nix_syntax, property.as_str())
                .and_then(|attr_path_value| {
                    attr_path_value
                        .children()
                        .find(|n| n.kind() == rnix::SyntaxKind::NODE_STRING)
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
                        flake_home_syntax_node(flake_home_dir).children_with_tokens().collect(),
                    );
                    Some(attr_path_value)
                });
            
            // 将变更后的语法树写入flake.nix文件
            let mut flake_file = File::create(self_var_path_str)?;
            let _ = flake_file.write_all(self_nix_syntax.to_string().as_bytes());
            Ok(())
        }
        None => bail!("self var path is not exits"),
    }
}

fn flake_home_syntax_node(flake_home_dir_opt: Option<PathBuf>) -> SyntaxNode {
    let flake_home_dir =
        flake_home_dir_opt.unwrap_or_else(|| env::current_dir().expect("无法获取当前目录"));
    let expr = format!("{:?}", flake_home_dir);
    let parsed = rnix::Root::parse(&expr);
    parsed.syntax().clone_for_update()
}
