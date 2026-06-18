use std::{fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use rnix::SyntaxNode;

/// 命令行参数解析器：将字符串转换为 `PathBuf`，同时校验路径是否存在且为文件。
///
/// 用于 clap 的 `value_parser`，在参数解析阶段提前验证文件有效性。
pub(crate) fn value_parser_for_pathbuf(path_str: &str) -> Result<PathBuf> {
    let path = PathBuf::from(path_str);
    if path.is_file() {
        Ok(path)
    } else {
        bail!(
            "The specified [{path_str}] path does not exist or is not a file"
        )
    }
}

/// 将 Nix 表达式字符串解析为可修改的语法树节点。
///
/// 返回的 `SyntaxNode` 已经 `clone_for_update()`，可以直接进行节点替换操作。
pub(crate) fn syntax(content: &str) -> SyntaxNode {
    rnix::Root::parse(content).syntax().clone_for_update()
}

/// 读取并解析一个 `.nix` 文件，返回可修改的语法树根节点。
///
/// 封装了文件读取和 Nix 语法解析，失败时通过 `anyhow::Context` 提供上下文错误信息。
pub(crate) fn parse_nix_file(file: &str) -> Result<SyntaxNode> {
    let content = fs::read_to_string(file)
        .with_context(|| format!("failed to read nix file: {file}"))?;
    Ok(syntax(&content))
}

/// 在语法树中查找指定属性名的 `NODE_ATTRPATH_VALUE` 节点。
///
/// 遍历根节点下所有属性集（`NODE_ATTR_SET`），在其中查找属性路径值节点（`NODE_ATTRPATH_VALUE`），
/// 并匹配其子孙节点中属性路径（`NODE_ATTRPATH`）的文本是否等于 `property`。
/// 找到则返回对应的 `NODE_ATTRPATH_VALUE` 节点，否则返回 `None`。
pub(crate) fn find_attr_inputs_node(root: &SyntaxNode, property: &str) -> Option<SyntaxNode> {
    root.children()
        .filter(|n| n.kind() == rnix::SyntaxKind::NODE_ATTR_SET)
        .flat_map(|attr_set| {
            attr_set
                .children()
                .filter(|n| n.kind() == rnix::SyntaxKind::NODE_ATTRPATH_VALUE)
        })
        .find(|attr_path_value| {
            attr_path_value
                .descendants()
                .any(|n| n.kind() == rnix::SyntaxKind::NODE_ATTRPATH && n.text() == property)
        })
}
