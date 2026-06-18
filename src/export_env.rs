use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use clap::{Args, ValueEnum};

use crate::util::value_parser_for_pathbuf;

/// 导出模式枚举
#[derive(ValueEnum, Clone, PartialEq, Debug)]
pub(crate) enum ExportMode {
    /// Parse INI format file, export each key as an environment variable
    Ini,
    /// Read entire file content as a single environment variable value
    File,
}

/// `export-env` 子命令的参数
#[derive(Args)]
pub(crate) struct ExportEnvArgs {
    /// 要读取的文件路径
    #[arg(
        short,
        long,
        help = "Path to the file to read",
        value_parser = value_parser_for_pathbuf
    )]
    pub(crate) file: PathBuf,

    /// 环境变量名的参数前缀（位于固定前缀之后），为空则省略
    #[arg(
        short,
        long,
        help = "Prefix for exported environment variable names (optional)",
        default_value = ""
    )]
    pub(crate) prefix: String,

    /// 导出模式：ini 或 file
    #[arg(
        short,
        long,
        help = "Mode: 'ini' for key=value format, 'file' for whole file content",
        default_value = "ini"
    )]
    pub(crate) mode: ExportMode,
}

/// 固定前缀，始终出现在导出环境变量名的最前面
const FIXED_PREFIX: &str = "nct";

/// 对字符串进行清洗，使其符合环境变量命名规范。
/// 将非字母数字和下划线的字符替换为 `_`。
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

/// 从文件中导出环境变量。
///
/// 支持两种模式：
/// - `ini` 模式：读取 INI 格式文件（key=value），
///   每个 key 导出为 `nct_[_{prefix}]_{清洗后的文件名}_{清洗后的section}_{清洗后的key}={value}`，
///   没有 section 的 key 归入 "default" section。prefix 为空时省略。
/// - `file` 模式：读取整个文件内容，
///   导出单个环境变量 `nct_[_{prefix}]_{清洗后的文件名}={文件内容}`。prefix 为空时省略。
pub(crate) fn export_env(file: PathBuf, prefix: String, mode: ExportMode) -> Result<()> {
    // 从文件路径中提取文件名（不含扩展名）
    let file_stem = file
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    let vars = match mode {
        ExportMode::Ini => export_ini_mode(&file, &prefix, file_stem),
        ExportMode::File => export_file_mode(&file, &prefix, file_stem),
    }?;

    eprintln!("generate export envs command:");
    for (name, value) in &vars {
        // 输出 export 语句，用户可复制执行，也可通过 eval/source 自动注入
        // eval 会捕获 std out, 然后忽略 std err，因此即想要提示也想要被执行，则两者都需要输出
        println!("  export {}={};", name, value);
        eprintln!("  export {}={};", name, value);
    }

    // 输出可复制的 eval / source 命令，方便用户一键注入
    let file_path = file.display();
    let mode_str = match mode {
        ExportMode::Ini => "ini",
        ExportMode::File => "file",
    };
    let prefix_opt = if prefix.is_empty() {
        String::new()
    } else {
        format!(" -p {}", prefix)
    };
    eprintln!();
    eprintln!("To inject into current shell, copy and run:");
    eprintln!("   eval $(nix-config-tools export-env -f {}{} -m {})", file_path, prefix_opt, mode_str);

    Ok(())
}

/// 以 ini 模式导出：读取文件，解析 key=value 对，生成 (变量名, 值) 列表
fn export_ini_mode(file: &PathBuf, prefix: &str, file_stem: &str) -> Result<Vec<(String, String)>> {
    let content = fs::read_to_string(file)
        .context(format!("Failed to read file: {:?}", file))?;

    Ok(build_ini_lines(&content, prefix, file_stem))
}

/// 以 file 模式导出：读取文件全部内容，作为单个环境变量的值
fn export_file_mode(file: &PathBuf, prefix: &str, file_stem: &str) -> Result<Vec<(String, String)>> {
    let content = fs::read_to_string(file)
        .context(format!("Failed to read file: {:?}", file))?;

    let content = content.trim().to_string();
    let var_name = build_var_name(prefix, &sanitize(file_stem));

    Ok(vec![(var_name, content)])
}

/// 构建完整的环境变量名（不含 key 部分）：
/// `{固定前缀}[_{参数前缀}]_{清洗后的文件名}`，全部转为小写。
/// 当 prefix 为空时，省略参数前缀部分。
fn build_var_name(prefix: &str, file_stem: &str) -> String {
    if prefix.is_empty() {
        format!("{}_{}", FIXED_PREFIX, file_stem).to_lowercase()
    } else {
        format!("{}_{}_{}", FIXED_PREFIX, prefix, file_stem).to_lowercase()
    }
}

/// 构建完整的环境变量名（含 section 和 key 部分）：
/// `{固定前缀}[_{参数前缀}]_{清洗后的文件名}_{清洗后的section}_{清洗后的key}`，全部转为小写。
/// 当 prefix 为空时，省略参数前缀部分。
fn build_var_name_with_section(prefix: &str, file_stem: &str, section: &str, key: &str) -> String {
    if prefix.is_empty() {
        format!(
            "{}_{}_{}_{}",
            FIXED_PREFIX,
            file_stem,
            sanitize(section),
            sanitize(key)
        )
        .to_lowercase()
    } else {
        format!(
            "{}_{}_{}_{}_{}",
            FIXED_PREFIX,
            prefix,
            file_stem,
            sanitize(section),
            sanitize(key)
        )
        .to_lowercase()
    }
}

/// 解析 INI 内容并构建 (变量名, 值) 列表。
/// 遍历所有 section，没有 section 的 key 归入默认的 "default" section。
/// 变量名格式：`nct_{prefix}_{file_stem}_{section}_{key}`
fn build_ini_lines(content: &str, prefix: &str, file_stem: &str) -> Vec<(String, String)> {
    let mut config = configparser::ini::Ini::new_cs(); // 大小写敏感
    let map = config.read(String::from(content)).unwrap_or_default();

    let file_stem = sanitize(file_stem);
    let mut result = Vec::new();
    for (section, keys) in &map {
        for (key, value) in keys {
            let val = value.as_deref().unwrap_or("").to_string();
            let var_name = build_var_name_with_section(prefix, &file_stem, section, key);
            result.push((var_name, val));
        }
    }
    // 按变量名排序
    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== sanitize 函数测试 ====================

    /// 测试正常字符串不受影响
    #[test]
    fn test_sanitize_normal() {
        assert_eq!(sanitize("hello_world"), "hello_world");
    }

    /// 测试特殊字符被替换为下划线
    #[test]
    fn test_sanitize_special_chars() {
        assert_eq!(sanitize("my-file.config"), "my_file_config");
    }

    /// 测试点和横线被替换
    #[test]
    fn test_sanitize_dots_and_dashes() {
        assert_eq!(sanitize("app.env.dev"), "app_env_dev");
    }

    /// 测试混合特殊字符
    #[test]
    fn test_sanitize_mixed() {
        assert_eq!(sanitize("DB-Password@123!"), "DB_Password_123_");
    }

    // ==================== ini 模式测试 ====================

    /// 测试基本的 key=value 解析（无 section，归入 default）
    #[test]
    fn test_ini_mode_basic() {
        let content = "PORT=8080\nHOST=localhost\n";
        let result = build_ini_lines(content, "nixos", "config");
        assert_eq!(
            result,
            vec![
                ("nct_nixos_config_default_host".to_string(), "localhost".to_string()),
                ("nct_nixos_config_default_port".to_string(), "8080".to_string()),
            ]
        );
    }

    /// 测试跳过注释行（# 和 ;）
    #[test]
    fn test_ini_mode_skip_comments() {
        let content = "# 这是一行注释\n; 这也是注释\nPORT=8080\nHOST=localhost\n";
        let result = build_ini_lines(content, "nixos", "config");
        assert_eq!(
            result,
            vec![
                ("nct_nixos_config_default_host".to_string(), "localhost".to_string()),
                ("nct_nixos_config_default_port".to_string(), "8080".to_string()),
            ]
        );
    }

    /// 测试去除 key 和 value 两端的空白字符
    #[test]
    fn test_ini_mode_trims_whitespace() {
        let content = "  PORT = 8080  \n  HOST  =  localhost  \n";
        let result = build_ini_lines(content, "nixos", "config");
        assert_eq!(
            result,
            vec![
                ("nct_nixos_config_default_host".to_string(), "localhost".to_string()),
                ("nct_nixos_config_default_port".to_string(), "8080".to_string()),
            ]
        );
    }

    /// 测试自定义参数前缀
    #[test]
    fn test_ini_mode_custom_prefix() {
        let content = "PORT=8080\n";
        let result = build_ini_lines(content, "myapp", "config");
        assert_eq!(
            result,
            vec![("nct_myapp_config_default_port".to_string(), "8080".to_string())]
        );
    }

    /// 测试空内容返回空列表
    #[test]
    fn test_ini_mode_empty_content() {
        let result = build_ini_lines("", "nixos", "config");
        assert!(result.is_empty());
    }

    /// 测试只有注释时返回空列表
    #[test]
    fn test_ini_mode_only_comments() {
        let result = build_ini_lines("# 注释\n; 注释\n", "nixos", "config");
        assert!(result.is_empty());
    }

    /// 测试包含 section 的解析，每个 section 的 key 都会导出
    #[test]
    fn test_ini_mode_with_sections() {
        let content = "global_key=global_val\n[section1]\nkey1=val1\nkey2=val2\n";
        let result = build_ini_lines(content, "nixos", "cfg");
        assert_eq!(
            result,
            vec![
                ("nct_nixos_cfg_default_global_key".to_string(), "global_val".to_string()),
                ("nct_nixos_cfg_section1_key1".to_string(), "val1".to_string()),
                ("nct_nixos_cfg_section1_key2".to_string(), "val2".to_string()),
            ]
        );
    }

    /// 测试多个 section
    #[test]
    fn test_ini_mode_multiple_sections() {
        let content = "[db]\nhost=localhost\nport=5432\n[redis]\nhost=127.0.0.1\nport=6379\n";
        let result = build_ini_lines(content, "nixos", "config");
        assert_eq!(
            result,
            vec![
                ("nct_nixos_config_db_host".to_string(), "localhost".to_string()),
                ("nct_nixos_config_db_port".to_string(), "5432".to_string()),
                ("nct_nixos_config_redis_host".to_string(), "127.0.0.1".to_string()),
                ("nct_nixos_config_redis_port".to_string(), "6379".to_string()),
            ]
        );
    }

    /// 测试文件名中的特殊字符被清洗
    #[test]
    fn test_ini_mode_sanitize_file_stem() {
        let content = "PORT=8080\n";
        let result = build_ini_lines(content, "nixos", "my-config.env");
        assert_eq!(
            result,
            vec![("nct_nixos_my_config_env_default_port".to_string(), "8080".to_string())]
        );
    }

    /// 测试 key 名中的特殊字符被清洗
    #[test]
    fn test_ini_mode_sanitize_key() {
        let content = "DB-Password=secret\n";
        let result = build_ini_lines(content, "nixos", "config");
        assert_eq!(
            result,
            vec![("nct_nixos_config_default_db_password".to_string(), "secret".to_string())]
        );
    }

    /// 测试 section 名中的特殊字符被清洗
    #[test]
    fn test_ini_mode_sanitize_section() {
        let content = "[my-section]\nkey=value\n";
        let result = build_ini_lines(content, "nixos", "config");
        assert_eq!(
            result,
            vec![("nct_nixos_config_my_section_key".to_string(), "value".to_string())]
        );
    }

    /// 测试空 prefix 时不包含参数前缀部分（ini 模式，无 section）
    #[test]
    fn test_ini_mode_empty_prefix_no_section() {
        let content = "PORT=8080\n";
        let result = build_ini_lines(content, "", "config");
        assert_eq!(
            result,
            vec![("nct_config_default_port".to_string(), "8080".to_string())]
        );
    }

    /// 测试空 prefix 时不包含参数前缀部分（ini 模式，有 section）
    #[test]
    fn test_ini_mode_empty_prefix_with_section() {
        let content = "[db]\nhost=localhost\n";
        let result = build_ini_lines(content, "", "config");
        assert_eq!(
            result,
            vec![("nct_config_db_host".to_string(), "localhost".to_string())]
        );
    }

    // ==================== file 模式测试 ====================

    /// 测试基本的文件内容导出
    #[test]
    fn test_file_mode_basic() {
        let content = "some secret token value\n";
        let result = parse_file_content(content, "nixos", "token");
        assert_eq!(
            result,
            vec![("nct_nixos_token".to_string(), "some secret token value".to_string())]
        );
    }

    /// 测试去除文件内容两端的空白字符
    #[test]
    fn test_file_mode_trims_content() {
        let content = "\n\n  hello world  \n\n";
        let result = parse_file_content(content, "nixos", "greeting");
        assert_eq!(
            result,
            vec![("nct_nixos_greeting".to_string(), "hello world".to_string())]
        );
    }

    /// 测试自定义参数前缀
    #[test]
    fn test_file_mode_custom_prefix() {
        let result = parse_file_content("abc123\n", "secret", "key");
        assert_eq!(result, vec![("nct_secret_key".to_string(), "abc123".to_string())]);
    }

    /// 测试空文件内容
    #[test]
    fn test_file_mode_empty_content() {
        let result = parse_file_content("", "nixos", "empty");
        assert_eq!(result, vec![("nct_nixos_empty".to_string(), "".to_string())]);
    }

    /// 测试文件名中的特殊字符被清洗
    #[test]
    fn test_file_mode_sanitize_file_stem() {
        let result = parse_file_content("value\n", "nixos", "my-file.config");
        assert_eq!(
            result,
            vec![("nct_nixos_my_file_config".to_string(), "value".to_string())]
        );
    }

    /// 测试空 prefix 时不包含参数前缀部分（file 模式）
    #[test]
    fn test_file_mode_empty_prefix() {
        let result = parse_file_content("hello\n", "", "token");
        assert_eq!(result, vec![("nct_token".to_string(), "hello".to_string())]);
    }

    /// 测试辅助函数：模拟 file 模式解析，返回 (变量名, 值) 列表
    fn parse_file_content(content: &str, prefix: &str, file_stem: &str) -> Vec<(String, String)> {
        let content = content.trim().to_string();
        let var_name = build_var_name(prefix, &sanitize(file_stem));
        vec![(var_name, content)]
    }
}


