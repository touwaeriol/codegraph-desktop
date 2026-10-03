use crate::models::{AppError, Result};
use std::sync::{
    atomic::{AtomicU8, Ordering},
    OnceLock,
};
static LANGUAGE: AtomicU8 = AtomicU8::new(0);
static SYSTEM_LANGUAGE: OnceLock<String> = OnceLock::new();
pub fn system_language() -> &'static str {
    SYSTEM_LANGUAGE.get_or_init(|| from_locale(sys_locale::get_locale().as_deref()).into())
}
pub fn from_locale(locale: Option<&str>) -> &'static str {
    if locale
        .unwrap_or_default()
        .trim()
        .split(['-', '_', '.', '@'])
        .next()
        .is_some_and(|tag| tag.eq_ignore_ascii_case("zh"))
    {
        "zh-CN"
    } else {
        "en"
    }
}
pub fn effective(saved: Option<&str>, locale: Option<&str>) -> &'static str {
    match saved {
        Some("zh-CN") => "zh-CN",
        Some("en") => "en",
        _ => from_locale(locale),
    }
}
pub fn current() -> &'static str {
    match LANGUAGE.load(Ordering::Relaxed) {
        1 => "en",
        2 => "zh-CN",
        _ => system_language(),
    }
}
pub fn set(language: &str) {
    LANGUAGE.store(if language == "zh-CN" { 2 } else { 1 }, Ordering::Relaxed);
}
pub fn validate(language: &str) -> Result<()> {
    if ["en", "zh-CN"].contains(&language) {
        Ok(())
    } else {
        Err(AppError::new("INVALID_LANGUAGE", "语言仅支持 en 或 zh-CN"))
    }
}
pub fn tr<'a>(zh: &'a str, en: &'a str) -> &'a str {
    if current() == "zh-CN" {
        zh
    } else {
        en
    }
}
pub fn message(text: &str) -> String {
    translate(text, current())
}
pub fn serialize_message<S: serde::Serializer>(
    text: &str,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(&message(text))
}
pub fn translate(text: &str, language: &str) -> String {
    if language == "zh-CN" {
        return text.into();
    }
    let translated=match text {
        "语言仅支持 en 或 zh-CN"=>"Language must be en or zh-CN",
        "此目录已添加"=>"This directory has already been added",
        "项目不存在"=>"Project not found",
        "项目目录不存在"|"目录不存在"=>"The project directory does not exist",
        "请选择一个存在的目录"=>"Select an existing directory",
        "项目名称不能为空"=>"Project name cannot be empty",
        "设置值超出允许范围"=>"The settings are outside the allowed range",
        "应用数据库损坏，请保留文件并恢复备份"=>"The application database is damaged. Preserve the files and restore a backup",
        "数据库版本较新，请升级应用"=>"The database was created by a newer version. Update the application",
        "项目 HTTP 绑定记录损坏，请恢复应用数据库"=>"The project's HTTP binding is damaged. Restore the application database",
        "无法分配本机 HTTP 端口"=>"Unable to allocate a local HTTP port",
        "无法分配未被其他项目登记的 HTTP 端口"=>"Unable to allocate an HTTP port not reserved by another project",
        "未找到 CodeGraph，请选择安装入口"=>"CodeGraph was not found. Select its installed entry point",
        "入口不支持 CodeGraph serve --mcp --path"=>"The selected entry does not support CodeGraph serve --mcp --path",
        "CodeGraph 检测失败"=>"CodeGraph detection failed",
        "CodeGraph 会话已退出，请重新启动"|"CodeGraph 会话异常退出，请重启"|"项目会话已退出，请重启"=>"The CodeGraph session has ended. Restart the project",
        "读取索引超时"=>"Reading the index status timed out",
        "索引未完成，或索引项目路径不匹配"=>"The index is incomplete or belongs to a different project path",
        "请先初始化项目索引"=>"Initialize the project index first",
        "HTTP 配置已更新，请重启项目实例使固定地址生效"|"请重启项目实例使固定 HTTP 配置生效"=>"Restart the project instance to activate its persistent HTTP configuration",
        "CLI 已结束，但索引尚未就绪"=>"The CLI has finished, but the index is not ready",
        "排队任务已取消"=>"The queued task was cancelled",
        "未知任务"|"不支持的索引操作"=>"Unsupported indexing operation",
        "任务已取消，索引状态需重新检查"=>"The task was cancelled. The index status must be checked again",
        "应用正在退出；索引状态将在下次启动时重新检查"=>"The application is exiting. The index status will be checked on the next launch",
        "退出前已停止索引任务"=>"The indexing task was stopped before exit",
        "启动已取消"=>"Startup was cancelled",
        "任务已取消"=>"The task was cancelled",
        "任务已结束"=>"The task has already ended",
        "应用正在退出"=>"The application is exiting",
        "预览已失效，请重新预览"|"该预览已应用，请重新预览"=>"This preview has expired or was already applied. Generate a new preview",
        "项目目录记录已变化，请重新预览"=>"The registered project directory has changed. Generate a new preview",
        "文件已变化，请刷新预览"|"配置在写入时被修改"=>"The configuration file has changed. Refresh the preview before applying",
        "网关 MCP 初始化及工具列表通过；真实客户端连接需在客户端确认。"=>"Gateway MCP initialization and tool listing succeeded. Confirm the actual connection in your client.",
        "找不到此备份所属项目"=>"The project associated with this backup could not be found",
        "仅允许清理已登记的旧目录"=>"Only registered previous project directories can be cleaned up",
        "配置格式无效，请检查 TOML/JSON 语法和编码"=>"Invalid configuration format. Check the TOML/JSON syntax and encoding",
        "配置格式无效，请检查 TOML/JSON 语法"=>"Invalid configuration format. Check the TOML/JSON syntax",
        "无法验证配置路径"=>"Unable to validate the configuration path",
        "配置符号链接指向项目外部"=>"The configuration symlink points outside the project",
        "mcp_servers 必须是 TOML 表"|"mcp_servers 必须为 TOML 表"=>"mcp_servers must be a TOML table",
        "JSON 顶层必须为对象"=>"The top-level JSON value must be an object",
        "mcpServers 必须为对象"=>"mcpServers must be an object",
        "不支持的客户端"=>"Unsupported client",
        "无效配置操作"=>"Invalid configuration operation",
        "移除配置不支持内容覆盖"=>"Configuration removal does not support content overrides",
        "覆盖设置必须对应所选客户端，且不能重复"=>"Overrides must refer to selected clients without duplicates",
        "仅手动编辑模式可以提供全文内容"=>"Full content can only be supplied in edit mode",
        "手动编辑模式必须提供全文内容"=>"Edit mode requires the complete file content",
        "现有 Claude Code JSON 文件为空，不能作为空配置覆盖"=>"The existing Claude Code JSON file is empty and cannot be merged as an empty configuration",
        "现有 Claude Code JSON 文件为空"=>"The existing Claude Code JSON file is empty",
        "旧 CodeGraph 配置已修改或归属无法确认，请检查后再迁移"=>"The legacy CodeGraph entry has changed or its ownership cannot be verified. Review it before migrating",
        "受管条目已修改或不属于本应用，无法自动移除"=>"The entry has changed or is not managed by this application. It cannot be removed automatically",
        "至少选择一个客户端"=>"Select at least one client",
        "无父目录"=>"The configuration path has no parent directory",
        "模拟第二文件写入失败"=>"Simulated failure writing the second file",
        "写入失败后恢复原文件"=>"Restored the original file after a write failure",
        "未修改"|"内容未变化"=>"Unchanged",
        "配置已写入"=>"Configuration written",
        "检测到旧连接器配置，请重新预览并应用 HTTP 配置"=>"A legacy connector configuration was detected. Preview and apply the HTTP configuration",
        "无法设置备份私有目录权限"=>"Unable to set private permissions on the backup directory",
        "备份清单格式无效"=>"The backup manifest is invalid",
        "备份不属于该项目"=>"The backup does not belong to this project",
        "备份路径不属于当前或已登记的旧项目目录"=>"The backup path is outside the current or registered previous project directories",
        "备份之后配置已被修改，不能覆盖后续编辑"=>"The configuration changed after this backup. Later edits will not be overwritten",
        "文件已经恢复，无需修改"=>"The files have already been restored",
        "MCP command/url 不能为空"=>"MCP command/url cannot be empty",
        "每个 MCP 服务必须指定 command 或 url，不能同时指定两者"=>"Each MCP server must specify either command or url, but not both",
        "MCP type 与 command/url 不匹配"=>"MCP type does not match command/url",
        "MCP args 必须为字符串数组"=>"MCP args must be an array of strings",
        "启动 CodeGraph 失败"=>"Failed to start CodeGraph",
        "MCP 握手超时"=>"MCP handshake timed out",
        "没有通过隔离验证的工具"=>"No tools passed project-isolation validation",
        "PORT_BIND_FAILED: 无法绑定项目端口"=>"PORT_BIND_FAILED: Unable to bind the project port",
        "INVALID_HTTP_TOKEN: 持久令牌必须为 64 位十六进制字符串"=>"INVALID_HTTP_TOKEN: A persistent token must contain 64 hexadecimal characters",
        "INVALID_HTTP_PORT: 持久 HTTP 连接必须指定非零端口"=>"INVALID_HTTP_PORT: Persistent HTTP requires a nonzero port",
        "运行记录项目不匹配"=>"The runtime record belongs to a different project",
        "运行记录无效"=>"The runtime record is invalid",
        "运行记录已过期"=>"The runtime record has expired",
        "运行地址无效"=>"The runtime address is invalid",
        "设置运行记录私有权限失败"=>"Failed to set private permissions on the runtime record",
        "无法确定应用数据目录"=>"Unable to locate the application data directory",
        "不支持的 CodeGraph 平台"=>"Unsupported CodeGraph platform",
        "不支持的 CodeGraph 架构"=>"Unsupported CodeGraph architecture",
        "CodeGraph 入口没有父目录"=>"The CodeGraph entry has no parent directory",
        "创建 Job Object 失败"=>"Failed to create the process Job Object",
        "设置 Job Object 失败"=>"Failed to configure the process Job Object",
        "关联 Job Object 失败"=>"Failed to attach the process to its Job Object",
        "进程已退出"=>"The process has exited",
        "进程未启动"=>"The process did not start",
        "进程未进入独立进程组"=>"The process did not enter its own process group",
        "无法枚举初始线程"=>"Unable to enumerate initial process threads",
        "无法恢复受管进程线程"=>"Unable to resume the managed process thread",
        "终止进程树失败"=>"Failed to terminate the process tree",
        "读取进程树状态失败"=>"Failed to read the process tree status",
        "进程树停止超时"=>"Stopping the process tree timed out",
        "进程组仍含活进程，停止未确认"=>"The process group still contains live processes; shutdown is not confirmed",
        _=>return translate_dynamic(text),
    };
    translated.into()
}
fn translate_dynamic(text: &str) -> String {
    if let Some(package) = text
        .strip_prefix("无法找到当前平台的 CodeGraph bundle (")
        .and_then(|s| s.strip_suffix(")，请安装对应 optional dependency 或选择官方独立入口"))
    {
        return format!("The CodeGraph bundle for this platform ({package}) was not found. Install its optional dependency or select an official standalone entry");
    }
    for (prefix, en) in [
        (
            "索引状态不是有效 JSON：",
            "Index status is not valid JSON: ",
        ),
        ("索引命令退出：", "Index command exited: "),
        (
            "CodeGraph 版本检测失败：",
            "CodeGraph version detection failed: ",
        ),
    ] {
        if let Some(detail) = text.strip_prefix(prefix) {
            return format!("{en}{detail}");
        }
    }
    for (suffix, en) in [
        (" 必须为字符串", " must be a string"),
        (" 必须为字符串键值表", " must be a table of string values"),
        (
            " 必须为字符串键值对象",
            " must be an object of string values",
        ),
        (" 必须为布尔值", " must be a boolean"),
        (" 必须为非负数", " must be a non-negative number"),
        (" 必须为 TOML 表", " must be a TOML table"),
        (" 必须为对象", " must be an object"),
    ] {
        if let Some(field) = text
            .strip_prefix("MCP ")
            .and_then(|s| s.strip_suffix(suffix))
        {
            return format!("MCP {}{en}", field.strip_prefix("服务 ").unwrap_or(field));
        }
    }
    if let Some(version) = text
        .strip_prefix("版本 ")
        .and_then(|s| s.strip_suffix(" 尚未通过兼容性验证；当前基线为 1.6.2"))
    {
        return format!("Version {version} has not been verified for compatibility. The tested baseline is 1.6.2");
    }
    text.into()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locale_fallback_and_explicit_preference() {
        for locale in ["zh", "zh-CN", "zh_TW", "ZH-hant-HK", "zh_CN.UTF-8"] {
            assert_eq!(from_locale(Some(locale)), "zh-CN");
            assert_eq!(effective(Some("en"), Some(locale)), "en");
        }
        for locale in [
            None,
            Some(""),
            Some("en-US"),
            Some("fr-FR"),
            Some("zhwhatever"),
        ] {
            assert_eq!(from_locale(locale), "en");
        }
        assert_eq!(effective(Some("zh-CN"), Some("en-US")), "zh-CN");
        assert_eq!(effective(Some("invalid"), Some("zh-TW")), "zh-CN");
    }
    #[test]
    fn exact_supported_languages_only() {
        for language in ["en", "zh-CN"] {
            assert!(validate(language).is_ok());
        }
        for language in ["", "zh", "EN", "en-US", " zh-CN"] {
            assert!(validate(language).is_err());
        }
    }
    #[test]
    fn internal_messages_translate_without_changing_external_text_or_paths() {
        assert_eq!(translate("MCP 握手超时", "en"), "MCP handshake timed out");
        assert!(translate("无法找到当前平台的 CodeGraph bundle (codegraph-linux-arm64)，请安装对应 optional dependency 或选择官方独立入口","en").starts_with("The CodeGraph bundle for this platform (codegraph-linux-arm64)"));
        assert_eq!(
            translate("此目录已添加", "en"),
            "This directory has already been added"
        );
        assert_eq!(translate("此目录已添加", "zh-CN"), "此目录已添加");
        assert_eq!(
            translate("索引命令退出：exit code 2", "en"),
            "Index command exited: exit code 2"
        );
        let user_text = r"CodeGraph: D:\用户项目\索引 failed with upstream status";
        assert_eq!(translate(user_text, "en"), user_text);
        assert_eq!(
            translate("MCP 服务 用户服务 必须为对象", "en"),
            "MCP 用户服务 must be an object"
        );
    }
    #[test]
    fn saved_language_survives_restart_and_invalid_save_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut db = crate::persistence::open(dir.path()).unwrap();
        assert_eq!(
            crate::persistence::language(&db).unwrap(),
            system_language()
        );
        crate::persistence::save_preferences(&mut db, 2, "tray", Some("en")).unwrap();
        assert!(crate::persistence::save_preferences(&mut db, 4, "exit", Some("fr")).is_err());
        assert_eq!(
            crate::persistence::setting(&db, "indexConcurrency")
                .unwrap()
                .as_deref(),
            Some("2")
        );
        assert_eq!(
            crate::persistence::setting(&db, "closeBehavior")
                .unwrap()
                .as_deref(),
            Some("tray")
        );
        drop(db);
        let mut db = crate::persistence::open(dir.path()).unwrap();
        assert_eq!(crate::persistence::language(&db).unwrap(), "en");
        crate::persistence::save_preferences(&mut db, 3, "exit", None).unwrap();
        assert_eq!(crate::persistence::language(&db).unwrap(), "en");
        crate::persistence::save_preferences(&mut db, 3, "exit", Some("zh-CN")).unwrap();
        drop(db);
        let db = crate::persistence::open(dir.path()).unwrap();
        assert_eq!(crate::persistence::language(&db).unwrap(), "zh-CN");
    }
}
