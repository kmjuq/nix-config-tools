use std::{env, fs::File, io::Write, path::PathBuf};

use anyhow::{Context, Result};
use clap::Args;
use rnix::SyntaxNode;

use crate::util::{find_attr_inputs_node, parse_nix_file, value_parser_for_pathbuf};

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
    let self_var_path_str = self_var_path
        .to_str()
        .context("self var path is not valid UTF-8")?;

    let self_nix_syntax = parse_nix_file(self_var_path_str)?;

    let attr_path_value = find_attr_inputs_node(&self_nix_syntax, &property)
        .context(format!("property '{property}' not found in {self_var_path_str}"))?;

    let attr_set = attr_path_value
        .children()
        .find(|n| n.kind() == rnix::SyntaxKind::NODE_STRING)
        .context(format!(
            "expected a string value for property '{property}'"
        ))?;

    // 替换旧字符串节点为新的目录路径
    let children: Vec<_> = attr_path_value.children_with_tokens().collect();
    let pos = children
        .iter()
        .position(|child| child.as_node() == Some(&attr_set))
        .context("string node not found in attribute path value")?;

    attr_path_value.splice_children(
        pos..pos + 1,
        flake_home_syntax_node(flake_home_dir)
            .children_with_tokens()
            .collect(),
    );

    // 将变更后的语法树写回文件
    let mut flake_file = File::create(self_var_path_str)?;
    flake_file.write_all(self_nix_syntax.to_string().as_bytes())?;
    Ok(())
}

fn flake_home_syntax_node(flake_home_dir: Option<PathBuf>) -> SyntaxNode {
    let dir = flake_home_dir.unwrap_or_else(|| {
        env::current_dir().expect("failed to get current directory")
    });
    // 直接用 Nix 字符串语法构造，避免 Debug 格式化引入引号问题
    let nix_str = format!("\"{}\"", dir.display());
    let parsed = rnix::Root::parse(&nix_str);
    parsed.syntax().clone_for_update()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// 测试 flake_home_syntax_node 在传入路径时生成正确的语法节点
    #[test]
    fn test_flake_home_syntax_node_with_path() {
        let path = PathBuf::from("/home/user/nixos-config");
        let node = flake_home_syntax_node(Some(path));
        let text = node.text().to_string();
        assert!(text.contains("/home/user/nixos-config"));
    }

    /// 测试 flake_home_syntax_node 在无参数时使用当前目录
    #[test]
    fn test_flake_home_syntax_node_default() {
        let node = flake_home_syntax_node(None);
        let text = node.text().to_string();
        let current = env::current_dir().unwrap();
        assert!(text.contains(&current.display().to_string()));
    }

    /// 测试 replace_flake_home 替换 self.nix 中的 flakeHome 属性
    #[test]
    fn test_replace_flake_home_basic() -> Result<()> {
        let mut tmp = NamedTempFile::new().unwrap();
        let content = "{\n  flakeHome = \"/old/path\";\n  otherProp = \"value\";\n}";
        write!(tmp, "{}", content).unwrap();
        let tmp_path = tmp.path().to_path_buf();

        let new_home = PathBuf::from("/new/flake/path");
        replace_flake_home(tmp_path.clone(), "flakeHome".to_string(), Some(new_home))?;

        let result = std::fs::read_to_string(&tmp_path)?;
        assert!(result.contains("/new/flake/path"));
        assert!(!result.contains("/old/path"));
        assert!(result.contains("otherProp"));
        Ok(())
    }

    /// 测试 replace_flake_home 无 flake_home_dir 时使用当前目录
    #[test]
    fn test_replace_flake_home_default_dir() -> Result<()> {
        let mut tmp = NamedTempFile::new().unwrap();
        let content = "{\n  flakeHome = \"/old/path\";\n}";
        write!(tmp, "{}", content).unwrap();
        let tmp_path = tmp.path().to_path_buf();

        replace_flake_home(tmp_path.clone(), "flakeHome".to_string(), None)?;

        let result = std::fs::read_to_string(&tmp_path)?;
        let current = env::current_dir().unwrap();
        assert!(result.contains(&current.display().to_string()));
        Ok(())
    }

    /// 测试 replace_flake_home 找不到属性时返回错误
    #[test]
    fn test_replace_flake_home_missing_property() {
        let mut tmp = NamedTempFile::new().unwrap();
        let content = "{\n  someOther = \"value\";\n}";
        write!(tmp, "{}", content).unwrap();
        let tmp_path = tmp.path().to_path_buf();

        let result = replace_flake_home(tmp_path, "flakeHome".to_string(), Some(PathBuf::from("/tmp")));
        assert!(result.is_err());
    }

    /// 测试 replace_flake_home 文件不存在时报错
    #[test]
    fn test_replace_flake_home_file_not_found() {
        let result = replace_flake_home(
            PathBuf::from("/nonexistent/path/self.nix"),
            "flakeHome".to_string(),
            Some(PathBuf::from("/tmp")),
        );
        assert!(result.is_err());
    }
}
