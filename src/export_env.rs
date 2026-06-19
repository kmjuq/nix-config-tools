use std::{collections::BTreeMap, fs, path::PathBuf};

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};

/// 导出模式枚举
#[derive(ValueEnum, Clone, PartialEq, Debug)]
pub(crate) enum ExportMode {
    /// Parse INI format file, export each key as an environment variable
    Ini,
    /// Read entire file content as a single environment variable value
    File,
}

/// 文件路径与模式的绑定，由 value_parser 解析 `path:mode` 格式
#[derive(Clone, Debug)]
pub(crate) struct FileWithMode {
    pub(crate) path: PathBuf,
    pub(crate) mode: ExportMode,
}

/// 解析 `-f` 参数值：支持 `path`（默认 ini）或 `path:mode` 格式
fn parse_file_with_mode(s: &str) -> Result<FileWithMode> {
    match s.rsplit_once(':') {
        Some((path, "ini")) => Ok(FileWithMode {
            path: PathBuf::from(path),
            mode: ExportMode::Ini,
        }),
        Some((path, "file")) => Ok(FileWithMode {
            path: PathBuf::from(path),
            mode: ExportMode::File,
        }),
        Some((_, mode)) => bail!("unknown mode '{}', expected 'ini' or 'file'", mode),
        None => Ok(FileWithMode {
            path: PathBuf::from(s),
            mode: ExportMode::Ini,
        }),
    }
}

/// `export-env` 子命令的参数
#[derive(Args)]
pub(crate) struct ExportEnvArgs {
    /// 要读取的文件路径，支持 `path` 或 `path:mode` 格式（mode 为 ini 或 file，默认 ini）。
    /// 可多次指定以处理多个文件。
    #[arg(
        short,
        long = "file",
        help = "File to read: 'path' (default ini) or 'path:mode' where mode is 'ini' or 'file'",
        value_parser = parse_file_with_mode,
        required = true
    )]
    pub(crate) files: Vec<FileWithMode>,

    /// 环境变量名的参数前缀（位于固定前缀之后），为空则省略
    #[arg(
        short,
        long,
        help = "Prefix for exported environment variable names (optional)",
        default_value = ""
    )]
    pub(crate) prefix: String,

    /// 以 JSON 格式输出所有环境变量，方便其他语言解析
    #[arg(
        short = 'j',
        long,
        help = "Output all environment variables as a single JSON object",
        default_value = "false"
    )]
    pub(crate) json: bool,
}

/// 固定前缀，始终出现在导出环境变量名的最前面
const FIXED_PREFIX: &str = "nct";

const NCT_ENV_NAME: &str = "NCT_ENV_JSON";

/// 对字符串进行清洗，使其符合环境变量命名规范。
/// 将非字母数字和下划线的字符替换为 `_`。
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

/// 从文件中导出环境变量。
///
/// 每个文件可以独立指定模式（ini 或 file）：
/// - `ini` 模式：读取 INI 格式文件（key=value），
///   每个 key 导出为 `nct_[_{prefix}]_{清洗后的文件名}_{清洗后的section}_{清洗后的key}={value}`，
///   没有 section 的 key 归入 "default" section。prefix 为空时省略。
/// - `file` 模式：读取整个文件内容，
///   导出单个环境变量 `nct_[_{prefix}]_{清洗后的文件名}={文件内容}`。prefix 为空时省略。
///
/// `files` 支持传入多个文件，每个文件独立解析和导出。
pub(crate) fn export_env(files: Vec<FileWithMode>, prefix: String, json: bool) -> Result<()> {
    let mut all_vars: Vec<(String, String)> = Vec::new();

    for fwm in &files {
        let file = &fwm.path;
        let file_stem = file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let vars = match fwm.mode {
            ExportMode::Ini => export_ini_mode(file, &prefix, file_stem),
            ExportMode::File => export_file_mode(file, &prefix, file_stem),
        }
        .with_context(|| format!("failed to export file: {}", file.display()))?;

        all_vars.extend(vars);
    }

    eprintln!("Exported {} environment variable(s):", all_vars.len());
    for (name, _) in &all_vars {
        eprintln!("  {}", name);
    }

    if json {
        // JSON 模式：将所有环境变量打包为一个大 JSON，只输出一个 export 语句
        let mut map: BTreeMap<&str, &str> = BTreeMap::new();
        for (name, value) in &all_vars {
            map.insert(name.as_str(), value.as_str());
        }
        let json_output = serde_json::to_string(&map)?;
        println!("export {}={};", NCT_ENV_NAME,json_output);
    } else {
        // 非 JSON 模式：每个变量输出一个 export 语句
        for (name, value) in &all_vars {
            println!("export {}={};", name, value);
        }
    }

    // 输出可复制的 eval 命令到 stderr
    let json_flag = if json { " --json" } else { "" };
    let prefix_opt = if prefix.is_empty() {
        String::new()
    } else {
        format!(" -p {}", prefix)
    };
    let files_args: Vec<String> = files
        .iter()
        .map(|fwm| {
            let mode_str = match fwm.mode {
                ExportMode::Ini => "ini",
                ExportMode::File => "file",
            };
            format!("-f {}:{}", fwm.path.display(), mode_str)
        })
        .collect();
    let files_str = files_args.join(" ");

    eprintln!();
    eprintln!("To inject into current shell, copy and run:");
    eprintln!(
        "   eval $(nix-config-tools export-env {} {}{})",
        files_str, prefix_opt, json_flag
    );

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
    use std::io::Write;
    use tempfile::NamedTempFile;

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

    // ==================== 多文件集成测试 ====================

    /// 辅助函数：创建临时文件并写入内容
    fn create_temp_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "{}", content).unwrap();
        file
    }

    /// 辅助函数：从 NamedTempFile 构建 FileWithMode
    fn fwm(file: &NamedTempFile, mode: ExportMode) -> FileWithMode {
        FileWithMode {
            path: file.path().to_path_buf(),
            mode,
        }
    }

    /// 测试多文件导出：两个 ini 文件的变量应该合并输出
    #[test]
    fn test_export_env_multiple_files_ini() -> Result<()> {
        let file1 = create_temp_file("PORT=8080\nHOST=localhost\n");
        let file2 = create_temp_file("DB_USER=admin\nDB_PASS=secret\n");

        let files = vec![fwm(&file1, ExportMode::Ini), fwm(&file2, ExportMode::Ini)];
        let result = export_env(files, "app".to_string(), false);
        assert!(result.is_ok());
        Ok(())
    }

    /// 测试多文件导出：file 模式下多个文件的变量应该合并输出
    #[test]
    fn test_export_env_multiple_files_file_mode() -> Result<()> {
        let file1 = create_temp_file("token-value-1\n");
        let file2 = create_temp_file("token-value-2\n");

        let files = vec![fwm(&file1, ExportMode::File), fwm(&file2, ExportMode::File)];
        let result = export_env(files, "app".to_string(), false);
        assert!(result.is_ok());
        Ok(())
    }

    /// 测试多文件导出：其中一个文件不存在时返回错误
    #[test]
    fn test_export_env_multiple_files_one_missing() {
        let file1 = create_temp_file("PORT=8080\n");
        let missing = FileWithMode {
            path: PathBuf::from("/nonexistent/file.ini"),
            mode: ExportMode::Ini,
        };

        let files = vec![fwm(&file1, ExportMode::Ini), missing];
        let result = export_env(files, "app".to_string(), false);
        assert!(result.is_err());
    }

    /// 测试多文件导出：空文件列表应该正常处理（无输出）
    #[test]
    fn test_export_env_empty_file_list() -> Result<()> {
        let result = export_env(vec![], "app".to_string(), false);
        assert!(result.is_ok());
        Ok(())
    }

    /// 测试多文件导出：单文件仍然正常工作（向后兼容）
    #[test]
    fn test_export_env_single_file() -> Result<()> {
        let file = create_temp_file("PORT=8080\n");
        let files = vec![fwm(&file, ExportMode::Ini)];
        let result = export_env(files, "app".to_string(), false);
        assert!(result.is_ok());
        Ok(())
    }

    /// 测试多文件导出：不同文件名产生不同前缀
    #[test]
    fn test_export_env_multiple_files_distinct_names() -> Result<()> {
        let file1 = create_temp_file("KEY1=val1\n");
        let file2 = create_temp_file("KEY2=val2\n");

        let files = vec![fwm(&file1, ExportMode::Ini), fwm(&file2, ExportMode::Ini)];
        let result = export_env(files, "test".to_string(), false);
        assert!(result.is_ok());
        Ok(())
    }

    /// 测试混合模式：同时使用 ini 和 file 模式
    #[test]
    fn test_export_env_mixed_modes() -> Result<()> {
        let ini_file = create_temp_file("PORT=8080\n");
        let file_file = create_temp_file("my-secret-token\n");

        let files = vec![fwm(&ini_file, ExportMode::Ini), fwm(&file_file, ExportMode::File)];
        let result = export_env(files, "app".to_string(), false);
        assert!(result.is_ok());
        Ok(())
    }

    /// 测试 JSON 输出模式：所有环境变量合并为一个 JSON 对象
    #[test]
    fn test_export_env_json_mode() -> Result<()> {
        let file1 = create_temp_file("PORT=8080\nHOST=localhost\n");
        let file2 = create_temp_file("DB_USER=admin\n");

        let files = vec![fwm(&file1, ExportMode::Ini), fwm(&file2, ExportMode::Ini)];
        let result = export_env(files, "app".to_string(), true);
        assert!(result.is_ok());
        Ok(())
    }

    /// 测试 JSON 输出模式：file 模式在 JSON 中正常
    #[test]
    fn test_export_env_json_mode_file() -> Result<()> {
        let file = create_temp_file("my-secret-token\n");

        let files = vec![fwm(&file, ExportMode::File)];
        let result = export_env(files, "app".to_string(), true);
        assert!(result.is_ok());
        Ok(())
    }

    /// 测试 JSON 输出模式：空文件列表输出空 JSON
    #[test]
    fn test_export_env_json_mode_empty() -> Result<()> {
        let result = export_env(vec![], "app".to_string(), true);
        assert!(result.is_ok());
        Ok(())
    }

    /// 测试 JSON 输出模式：混合 ini 和 file 模式
    #[test]
    fn test_export_env_json_mode_mixed() -> Result<()> {
        let ini_file = create_temp_file("PORT=8080\n");
        let file_file = create_temp_file("token-value\n");

        let files = vec![fwm(&ini_file, ExportMode::Ini), fwm(&file_file, ExportMode::File)];
        let result = export_env(files, "app".to_string(), true);
        assert!(result.is_ok());
        Ok(())
    }

    // ==================== parse_file_with_mode 测试 ====================

    /// 测试解析不带 mode 后缀的路径，默认返回 ini
    #[test]
    fn test_parse_file_with_mode_default() -> Result<()> {
        let fwm = parse_file_with_mode("/path/to/config.ini")?;
        assert_eq!(fwm.path, PathBuf::from("/path/to/config.ini"));
        assert_eq!(fwm.mode, ExportMode::Ini);
        Ok(())
    }

    /// 测试解析 `path:ini` 格式
    #[test]
    fn test_parse_file_with_mode_explicit_ini() -> Result<()> {
        let fwm = parse_file_with_mode("/path/to/file:ini")?;
        assert_eq!(fwm.path, PathBuf::from("/path/to/file"));
        assert_eq!(fwm.mode, ExportMode::Ini);
        Ok(())
    }

    /// 测试解析 `path:file` 格式
    #[test]
    fn test_parse_file_with_mode_explicit_file() -> Result<()> {
        let fwm = parse_file_with_mode("/path/to/token:file")?;
        assert_eq!(fwm.path, PathBuf::from("/path/to/token"));
        assert_eq!(fwm.mode, ExportMode::File);
        Ok(())
    }

    /// 测试解析未知 mode 返回错误
    #[test]
    fn test_parse_file_with_mode_unknown() {
        let result = parse_file_with_mode("/path/to/file:unknown");
        assert!(result.is_err());
    }
}


