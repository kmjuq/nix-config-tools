use std::{fs::File, io::Write, path::PathBuf, process::Command};

use anyhow::{Context, Result};
use clap::Args;

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
    let output = Command::new("nix")
        .arg("eval")
        .arg("--file")
        .arg(file)
        .output()
        .context("failed to execute `nix eval`")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("nix eval failed: {stderr}");
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn replace_inputs(expression_path: PathBuf, flake_path: PathBuf) -> Result<()> {
    let expr_path_str = expression_path
        .to_str()
        .context("expression path is not valid UTF-8")?;
    let flake_path_str = flake_path
        .to_str()
        .context("flake.nix path is not valid UTF-8")?;

    let expr_content = nix_eval_content(expr_path_str)?;
    let expr_syntax = syntax(&expr_content);

    let flake_syntax = parse_nix_file(flake_path_str)?;

    apply_syntax_replace(&flake_syntax, &expr_syntax)?;

    // 将变更后的语法树写回 flake.nix
    let mut flake_file = File::create(flake_path)?;
    flake_file.write_all(flake_syntax.to_string().as_bytes())?;
    Ok(())
}

/// 在 flake 语法树中查找 `inputs` 属性并替换其值（属性集）为新的语法节点。
///
/// 此函数抽取核心替换逻辑，使其可独立于文件 I/O 和外部命令进行单元测试。
fn apply_syntax_replace(flake_syntax: &rnix::SyntaxNode, new_value_syntax: &rnix::SyntaxNode) -> Result<()> {
    let attr_path_value = find_attr_inputs_node(flake_syntax, "inputs")
        .context("'inputs' attribute not found in flake.nix")?;

    let attr_set = attr_path_value
        .children()
        .find(|n| n.kind() == rnix::SyntaxKind::NODE_ATTR_SET)
        .context("expected an attribute set for 'inputs'")?;

    let children: Vec<_> = attr_path_value.children_with_tokens().collect();
    let pos = children
        .iter()
        .position(|child| child.as_node() == Some(&attr_set))
        .context("attr set node not found in attribute path value")?;

    attr_path_value.splice_children(
        pos..pos + 1,
        new_value_syntax.children_with_tokens().collect(),
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    // ==================== apply_syntax_replace 测试 ====================

    /// 正常替换：flake.nix 中有 `inputs = { ... }`，替换为新的属性集
    #[test]
    fn test_apply_syntax_replace_basic() -> Result<()> {
        let flake_content = "{\n  inputs = {\n    nixpkgs.url = \"github:old/url\";\n  };\n}";
        let new_inputs = "{ nixpkgs.url = \"github:new/url\"; }";

        let flake_syntax = syntax(flake_content);
        let new_syntax = syntax(new_inputs);

        apply_syntax_replace(&flake_syntax, &new_syntax)?;

        let result = flake_syntax.to_string();
        assert!(result.contains("github:new/url"));
        assert!(!result.contains("github:old/url"));
        Ok(())
    }

    /// 替换后保持其他属性不变
    #[test]
    fn test_apply_syntax_replace_preserves_other_attrs() -> Result<()> {
        let flake_content = "{\n  description = \"test\";\n  inputs = {\n    nixpkgs.url = \"old\";\n  };\n  outputs = { ... };\n}";
        let new_inputs = "{ nixpkgs.url = \"new\"; }";

        let flake_syntax = syntax(flake_content);
        let new_syntax = syntax(new_inputs);

        apply_syntax_replace(&flake_syntax, &new_syntax)?;

        let result = flake_syntax.to_string();
        assert!(result.contains("description"));
        assert!(result.contains("outputs"));
        assert!(result.contains("nixpkgs.url = \"new\""));
        assert!(!result.contains("\"old\""));
        Ok(())
    }

    /// 多个 inputs 被整体替换
    #[test]
    fn test_apply_syntax_replace_multiple_inputs() -> Result<()> {
        let flake_content = "{\n  inputs = {\n    a.url = \"a\";\n    b.url = \"b\";\n  };\n}";
        let new_inputs = "{ x.url = \"x\"; y.url = \"y\"; z.url = \"z\"; }";

        let flake_syntax = syntax(flake_content);
        let new_syntax = syntax(new_inputs);

        apply_syntax_replace(&flake_syntax, &new_syntax)?;

        let result = flake_syntax.to_string();
        assert!(result.contains("x.url"));
        assert!(result.contains("y.url"));
        assert!(result.contains("z.url"));
        assert!(!result.contains("a.url"));
        assert!(!result.contains("b.url"));
        Ok(())
    }

    /// 找不到 `inputs` 属性时返回错误
    #[test]
    fn test_apply_syntax_replace_missing_inputs() {
        let flake_content = "{\n  description = \"no inputs here\";\n}";
        let flake_syntax = syntax(flake_content);
        let new_syntax = syntax("{ nixpkgs.url = \"x\"; }");

        let result = apply_syntax_replace(&flake_syntax, &new_syntax);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("inputs"));
    }

    /// `inputs` 的值不是属性集（如直接是字符串）时返回错误
    #[test]
    fn test_apply_syntax_replace_inputs_not_attr_set() {
        let flake_content = "{\n  inputs = \"not an attr set\";\n}";
        let flake_syntax = syntax(flake_content);
        let new_syntax = syntax("{ x = 1; }");

        let result = apply_syntax_replace(&flake_syntax, &new_syntax);
        assert!(result.is_err());
    }

    // ==================== replace_inputs 集成测试 ====================

    /// 端到端测试：通过临时文件测试 replace_inputs 的完整流程（跳过 nix eval）
    /// 注：由于 replace_inputs 依赖 `nix eval`，此测试在 nix 不可用时会被跳过
    #[test]
    fn test_replace_inputs_integration() -> Result<()> {
        // 检查 nix 是否可用
        if Command::new("nix").arg("--version").output().is_err() {
            eprintln!("skipping integration test: nix not available");
            return Ok(());
        }

        // 创建一个返回固定 inputs 的 nix 表达式
        let mut expr_file = NamedTempFile::new()?;
        write!(
            expr_file,
            r#"{{ nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable"; }}"#
        )?;
        let expr_path = expr_file.path().to_path_buf();

        // 创建目标 flake.nix
        let mut flake_file = NamedTempFile::new()?;
        write!(
            flake_file,
            "{{\n  inputs = {{\n    old.url = \"old\";\n  }};\n}}"
        )?;
        let flake_path = flake_file.path().to_path_buf();

        replace_inputs(expr_path, flake_path.clone())?;

        let result = std::fs::read_to_string(&flake_path)?;
        assert!(!result.contains("old.url"));
        Ok(())
    }

    /// replace_inputs 文件不存在时报错
    #[test]
    fn test_replace_inputs_file_not_found() {
        let result = replace_inputs(
            PathBuf::from("/nonexistent/expr.nix"),
            PathBuf::from("/nonexistent/flake.nix"),
        );
        assert!(result.is_err());
    }

    // ==================== nix_eval_content 测试 ====================

    /// 测试 nix_eval_content 文件不存在时报错
    #[test]
    fn test_nix_eval_content_file_not_found() {
        let result = nix_eval_content("/nonexistent/file.nix");
        assert!(result.is_err());
    }
}
