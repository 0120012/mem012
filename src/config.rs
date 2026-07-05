use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    database: BTreeMap<String, String>,
    #[serde(default)]
    categories: CategoriesConfig,
    #[serde(default)]
    rerank: RerankConfig,
    embeddings: EmbeddingsConfig,
    #[serde(default)]
    network: NetworkConfig,
    #[serde(default)]
    debug: DebugConfig,
    #[allow(dead_code)]
    cleanup: CleanupConfig,
    server: ServerConfig,
    #[serde(default)]
    client: ClientConfig,
}

#[derive(Default, Deserialize)]
struct CategoriesConfig {
    index_list: Vec<String>,
}

#[derive(Default, Deserialize)]
struct RerankConfig {
    enabled: bool,
    rerank_api: Option<String>,
    #[serde(alias = "rerand_api_type")]
    rerank_api_type: Option<String>,
    rerank_model: Option<String>,
    rerank_key: Option<String>,
}

#[allow(dead_code)]
pub struct RerankSettings {
    pub api: String,
    pub api_type: String,
    pub key: String,
    pub model: String,
    pub proxy: Option<String>,
}

pub struct EmbeddingSettings {
    pub api: String,
    pub api_type: String,
    pub key: String,
    pub model: String,
    pub dimension: usize,
    pub fallback_max_distance: f64,
    pub proxy: Option<String>,
}

#[derive(Deserialize)]
struct EmbeddingsConfig {
    embeddings_api: String,
    embeddings_api_type: Option<String>,
    embeddings_key: String,
    embeddings_model: Option<String>,
    embeddings_dimension: Option<usize>,
    embeddings_fallback_max_distance: Option<f64>,
}

#[derive(Default, Deserialize)]
struct NetworkConfig {
    enable_proxy: bool,
    proxy: Option<String>,
}

#[derive(Default, Deserialize)]
struct DebugConfig {
    reset_db: bool,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct CleanupConfig {
    trash_retention_minutes: u64,
    sweep_interval_minutes: u64,
}

#[derive(Deserialize)]
struct ServerConfig {
    addr: String,
    api_token: Option<String>,
}

#[derive(Default, Deserialize)]
struct ClientConfig {
    base_url: Option<String>,
}

impl Config {
    pub fn database_url(&self, profile: &str) -> Option<&str> {
        self.database.get(profile).map(String::as_str)
    }

    pub fn database_entries(&self) -> impl Iterator<Item = (&str, &str)> {
        // Why：项目白名单就是 [database] 配置本身，避免新增库时还要改 Rust 枚举。
        self.database
            .iter()
            .map(|(name, url)| (name.as_str(), url.as_str()))
    }

    pub fn server_addr(&self) -> &str {
        self.server.addr.as_str()
    }

    pub fn client_base_url(&self) -> Option<&str> {
        // Why：server.addr 是监听地址；远程客户端访问必须使用可请求的公网 base URL。
        let url = self.client.base_url.as_deref()?.trim();
        (!url.is_empty()).then_some(url)
    }

    pub fn api_token(&self) -> Option<&str> {
        // Why：认证密钥跟随 HTTP 服务配置，避免运行环境变量改变后端认证策略。
        let token = self.server.api_token.as_deref()?.trim();
        (!token.is_empty()).then_some(token)
    }

    pub fn reset_db(&self) -> bool {
        // Why：调试清库是破坏性动作，必须由配置显式打开，缺省永远关闭。
        self.debug.reset_db
    }

    #[allow(dead_code)]
    pub fn trash_retention_minutes(&self) -> u64 {
        // Why：cleanup 必须显式配置；这里只避免零值导致立即硬删。
        self.cleanup.trash_retention_minutes.max(1)
    }

    #[allow(dead_code)]
    pub fn cleanup_sweep_interval_minutes(&self) -> u64 {
        // Why：后台扫描间隔必须有下限，避免错误配置导致 tight loop 压垮数据库。
        self.cleanup.sweep_interval_minutes.max(1)
    }

    pub fn search_default_limit(&self) -> i32 {
        // Why：搜索默认条数已收回为后端固定策略，避免 TOML 暴露未使用调参入口。
        8
    }

    pub fn category_index_list(&self) -> &[String] {
        // Why：category 白名单只需要一组可写索引名，避免把展示描述和权限状态混进写入校验。
        &self.categories.index_list
    }

    #[allow(dead_code)]
    pub fn rerank_settings(&self) -> Option<RerankSettings> {
        // Why：rerank 是可选 provider 能力，关闭或缺少远程鉴权时不应阻塞基础搜索。
        if !self.rerank.enabled {
            return None;
        }
        let api = self.rerank.rerank_api.as_deref().unwrap_or("local").trim();
        let key = self.rerank.rerank_key.as_deref().unwrap_or("").trim();
        if api.is_empty() || (api != "local" && key.is_empty()) {
            return None;
        }
        Some(RerankSettings {
            api: api.to_string(),
            api_type: self
                .rerank
                .rerank_api_type
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("rerank")
                .to_string(),
            key: key.to_string(),
            model: self
                .rerank
                .rerank_model
                .as_deref()
                .unwrap_or("Qwen3-Reranker-4B")
                .to_string(),
            proxy: self.provider_proxy(),
        })
    }

    pub fn embedding_settings(&self) -> Option<EmbeddingSettings> {
        // Why：embedding 是派生索引能力，配置为空时应跳过生成而不是阻塞主流程。
        let api = self.embeddings.embeddings_api.trim();
        let key = self.embeddings.embeddings_key.trim();
        if api.is_empty() {
            return None;
        }
        if api != "local" && key.is_empty() {
            return None;
        }
        Some(EmbeddingSettings {
            api: api.to_string(),
            api_type: self
                .embeddings
                .embeddings_api_type
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .unwrap_or("embeddings")
                .to_string(),
            key: key.to_string(),
            model: self
                .embeddings
                .embeddings_model
                .as_deref()
                .unwrap_or("BAAI/bge-m3")
                .to_string(),
            dimension: self.embeddings.embeddings_dimension.unwrap_or(1024),
            // Why：默认值按 BAAI/bge-m3 实测标定（无关 score ≤ 0.36，相关 ≥ 0.54，取中值 0.45 即距离 0.55）；更换模型需在 config 重标。
            fallback_max_distance: self
                .embeddings
                .embeddings_fallback_max_distance
                .unwrap_or(0.55),
            proxy: self.provider_proxy(),
        })
    }

    fn provider_proxy(&self) -> Option<String> {
        if !self.network.enable_proxy {
            return None;
        }
        self.network
            .proxy
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    }
}

// Why：配置读取独立于数据库初始化，避免 IO 错误和数据库错误混在同一层。
pub fn load_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    // What：读取 MEM012_CONFIG 指向的配置；未设置时回退到调用方传入路径。
    // Why：agent 通常不在项目目录执行，配置路径必须能由宿主环境固定注入。
    let text = std::fs::read_to_string(config_path(path))?;

    parse_config_text(&text)
}

pub fn config_path(default_path: &str) -> std::path::PathBuf {
    config_path_from_env(default_path, std::env::var_os("MEM012_CONFIG"))
}

pub fn local_api_base_url(server_addr: &str) -> Result<String, Box<dyn std::error::Error>> {
    // What：把 server 监听地址转换成 CLI 可请求的本机 API 地址。
    // Why：0.0.0.0 是监听地址，不是可靠的客户端目标地址，CLI 必须改用 loopback 访问本机服务。
    let trimmed = server_addr.trim().trim_end_matches('/');
    if trimmed.is_empty() {
        return Err("server.addr 不能为空".into());
    }
    let (scheme, target) = if let Some(target) = trimmed.strip_prefix("http://") {
        ("http://", target)
    } else if let Some(target) = trimmed.strip_prefix("https://") {
        ("https://", target)
    } else {
        ("http://", trimmed)
    };
    let target = target
        .strip_prefix("0.0.0.0:")
        .map(|port| format!("127.0.0.1:{port}"))
        .unwrap_or_else(|| target.to_string());
    Ok(format!("{scheme}{target}"))
}

fn config_path_from_env(
    default_path: &str,
    env_path: Option<std::ffi::OsString>,
) -> std::path::PathBuf {
    env_path
        .filter(|value| !value.as_os_str().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(default_path))
}

fn parse_config_text(text: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let raw_config: toml::Value = toml::from_str(text)?;
    let cleanup = raw_config
        .get("cleanup")
        .and_then(toml::Value::as_table)
        .ok_or("配置缺少 [cleanup] 段")?;
    if !cleanup.contains_key("trash_retention_minutes") {
        return Err("配置缺少 [cleanup].trash_retention_minutes".into());
    }
    if !cleanup.contains_key("sweep_interval_minutes") {
        return Err("配置缺少 [cleanup].sweep_interval_minutes".into());
    }

    // 2、配置读取到struct config
    let config: Config = toml::from_str(text)?;

    Ok(config)
}

pub fn append_database_profile_text(
    text: &str,
    profile: &str,
    database_url: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    // What：在 TOML 文本的 [database] 内追加一个 profile 连接串。
    // Why：create_profile 需要保留用户配置文件里的注释和顺序，不能用反序列化重写整份配置。
    validate_database_profile_name(profile)?;
    let mut document = text.parse::<toml_edit::DocumentMut>()?;
    let database = document
        .get_mut("database")
        .and_then(toml_edit::Item::as_table_like_mut)
        .ok_or("配置缺少 [database] 段")?;
    if database.contains_key(profile) {
        return Err(format!("profile 已存在于 [database]: {profile}").into());
    }
    database.insert(profile, toml_edit::value(database_url));

    Ok(document.to_string())
}

pub fn write_config_text_atomic(
    path: &std::path::Path,
    text: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    // What：把配置文本写入同目录临时文件，再原子 rename 到目标路径。
    // Why：create_profile 成功后不能让配置文件停在半写入状态，否则会污染后续 CLI 启动。
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let file_name = path.file_name().ok_or("配置路径缺少文件名")?;
    let mut temp_name = std::ffi::OsString::from(".");
    temp_name.push(file_name);
    temp_name.push(format!(".{}.tmp", std::process::id()));
    let temp_path = parent.join(temp_name);
    let permissions = std::fs::metadata(path)?.permissions();
    std::fs::write(&temp_path, text)?;
    if let Err(error) = std::fs::set_permissions(&temp_path, permissions) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error.into());
    }
    if let Err(error) = std::fs::rename(&temp_path, path) {
        let _ = std::fs::remove_file(&temp_path);
        return Err(error.into());
    }
    Ok(())
}

pub fn derive_profile_database_url(
    admin_database_url: &str,
    profile: &str,
    password: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    validate_database_profile_name(profile)?;
    let mut url = url::Url::parse(admin_database_url)?;
    if !matches!(url.scheme(), "postgres" | "postgresql") {
        return Err("admin database URL 必须使用 postgres/postgresql scheme".into());
    }
    url.set_username(profile)
        .map_err(|_| "profile 名称不能写入 database URL")?;
    url.set_password(Some(password))
        .map_err(|_| "profile 密码不能写入 database URL")?;
    let database_path = format!("/mem_{profile}");
    url.set_path(&database_path);

    Ok(url.to_string())
}

pub fn derive_admin_profile_database_url(
    admin_database_url: &str,
    profile: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    validate_database_profile_name(profile)?;
    let mut url = url::Url::parse(admin_database_url)?;
    if !matches!(url.scheme(), "postgres" | "postgresql") {
        return Err("admin database URL 必须使用 postgres/postgresql scheme".into());
    }
    url.set_path(&format!("/mem_{profile}"));

    Ok(url.to_string())
}

fn validate_database_profile_name(profile: &str) -> Result<(), Box<dyn std::error::Error>> {
    let valid = profile
        .as_bytes()
        .first()
        .is_some_and(u8::is_ascii_lowercase)
        && profile
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_');
    if !valid {
        return Err("profile 名称必须匹配 [a-z][a-z0-9_]*".into());
    }
    if matches!(profile, "postgres" | "template0" | "template1") {
        return Err("profile 名称是保留名".into());
    }
    Ok(())
}

pub fn generate_profile_password() -> String {
    use rand::{Rng as _, seq::SliceRandom as _};

    const UPPER: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const LOWER: &[u8] = b"abcdefghijklmnopqrstuvwxyz";
    const DIGIT: &[u8] = b"0123456789";
    const ALL: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

    // What：生成固定 16 位、只含 ASCII 字母数字的 profile 密码。
    // Why：显式保留大小写和数字类别，避免下游数据库密码策略拒绝随机弱形态。
    let mut rng = rand::rngs::OsRng;
    let mut password = Vec::with_capacity(16);
    password.push(UPPER[rng.gen_range(0..UPPER.len())] as char);
    password.push(LOWER[rng.gen_range(0..LOWER.len())] as char);
    password.push(DIGIT[rng.gen_range(0..DIGIT.len())] as char);
    while password.len() < 16 {
        password.push(ALL[rng.gen_range(0..ALL.len())] as char);
    }
    password.shuffle(&mut rng);
    password.into_iter().collect()
}

pub fn admin_database_url_from_env_value(
    value: Option<std::ffi::OsString>,
) -> Result<String, Box<dyn std::error::Error>> {
    let value = value.ok_or("缺少环境变量 MEM012_ADMIN_DATABASE_URL")?;
    let value = value
        .into_string()
        .map_err(|_| "MEM012_ADMIN_DATABASE_URL 必须是 UTF-8")?;
    let value = value.trim();
    if value.is_empty() {
        return Err("MEM012_ADMIN_DATABASE_URL 不能为空".into());
    }
    Ok(value.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        admin_database_url_from_env_value, append_database_profile_text, config_path_from_env,
        derive_admin_profile_database_url, derive_profile_database_url, generate_profile_password,
        local_api_base_url, parse_config_text, write_config_text_atomic,
    };
    use std::{ffi::OsString, path::PathBuf};

    #[test]
    fn parse_config_text_rejects_missing_cleanup_section() {
        let result = parse_config_text(
            r#"
[database]
riko = "postgres://localhost/riko"

[search]
default_limit = 5
graph_expand_depth = 1
keyword = 1
fulltext = 1
semantic = 1
graph = 1
stale_penalty = 1

[embeddings]
embeddings_api = "local"

[server]
addr = "127.0.0.1:3012"
"#,
        );
        let error = match result {
            Ok(_) => panic!("expected missing cleanup error"),
            Err(error) => error.to_string(),
        };

        assert!(error.contains("配置缺少 [cleanup] 段"));
    }

    #[test]
    fn database_entries_include_share_and_all_profiles() {
        let config = parse_config_text(
            r#"
database = { riko = "postgres://localhost/riko", claude = "postgres://localhost/claude", share = "postgres://localhost/share" }
search = { default_limit = 5, graph_expand_depth = 1, keyword = 1, fulltext = 1, semantic = 1, graph = 1, stale_penalty = 1 }
embeddings = { embeddings_api = "local", embeddings_key = "" }
cleanup = { trash_retention_minutes = 10080, sweep_interval_minutes = 5 }
server = { addr = "127.0.0.1:3012" }
"#,
        )
        .unwrap();

        let entries: Vec<_> = config.database_entries().collect();
        assert_eq!(entries.len(), 3);
        assert!(entries.contains(&("riko", "postgres://localhost/riko")));
        assert!(entries.contains(&("claude", "postgres://localhost/claude")));
        assert!(entries.contains(&("share", "postgres://localhost/share")));
    }

    #[test]
    fn config_path_from_env_uses_default_without_override() {
        assert_eq!(
            config_path_from_env("config.toml", None),
            PathBuf::from("config.toml")
        );
        assert_eq!(
            config_path_from_env("config.toml", Some(OsString::new())),
            PathBuf::from("config.toml")
        );
    }

    #[test]
    fn config_path_from_env_prefers_non_empty_override() {
        assert_eq!(
            config_path_from_env("config.toml", Some(OsString::from("/tmp/mem012.toml"))),
            PathBuf::from("/tmp/mem012.toml")
        );
    }

    #[test]
    fn local_api_base_url_maps_wildcard_listener_to_loopback() {
        assert_eq!(
            local_api_base_url("0.0.0.0:37777").unwrap(),
            "http://127.0.0.1:37777"
        );
        assert_eq!(
            local_api_base_url("http://0.0.0.0:37777/").unwrap(),
            "http://127.0.0.1:37777"
        );
    }

    #[test]
    fn local_api_base_url_preserves_client_addresses() {
        assert_eq!(
            local_api_base_url("127.0.0.1:37777").unwrap(),
            "http://127.0.0.1:37777"
        );
        assert_eq!(
            local_api_base_url("https://example.com").unwrap(),
            "https://example.com"
        );
    }

    #[test]
    fn append_database_profile_text_preserves_existing_config_shape() {
        let updated = append_database_profile_text(
            r#"# keep top comment
[database]
# keep database comment
riko = "postgres://localhost/riko"

[server]
addr = "127.0.0.1:3012"
"#,
            "rikocodex",
            "postgres://localhost/mem_rikocodex",
        )
        .unwrap();

        assert!(updated.contains("# keep top comment"));
        assert!(updated.contains("# keep database comment"));
        assert!(
            updated.find("riko =").unwrap() < updated.find("rikocodex =").unwrap()
                && updated.find("rikocodex =").unwrap() < updated.find("[server]").unwrap()
        );
        let document = updated.parse::<toml_edit::DocumentMut>().unwrap();
        assert_eq!(
            document["database"]["rikocodex"].as_str(),
            Some("postgres://localhost/mem_rikocodex")
        );
    }

    #[test]
    fn append_database_profile_text_keeps_config_parseable() {
        let updated = append_database_profile_text(
            r#"[database]
riko = "postgres://localhost/riko"

[embeddings]
embeddings_api = "local"
embeddings_key = ""

[cleanup]
trash_retention_minutes = 10080
sweep_interval_minutes = 5

[server]
addr = "127.0.0.1:3012"
"#,
            "rikocodex",
            "postgres://localhost/mem_rikocodex",
        )
        .unwrap();

        let config = parse_config_text(&updated).unwrap();

        assert_eq!(
            config.database_url("rikocodex"),
            Some("postgres://localhost/mem_rikocodex")
        );
    }

    #[test]
    fn append_database_profile_text_accepts_inline_database_table() {
        let updated = append_database_profile_text(
            r#"
database = { riko = "postgres://localhost/riko", share = "postgres://localhost/share" }
embeddings = { embeddings_api = "local", embeddings_key = "" }
cleanup = { trash_retention_minutes = 10080, sweep_interval_minutes = 5 }
server = { addr = "127.0.0.1:3012" }
"#,
            "rikocodex",
            "postgres://localhost/mem_rikocodex",
        )
        .unwrap();

        let config = parse_config_text(&updated).unwrap();

        assert!(updated.contains("database = {"));
        assert_eq!(
            config.database_url("rikocodex"),
            Some("postgres://localhost/mem_rikocodex")
        );
    }

    #[test]
    fn append_database_profile_text_rejects_existing_profile() {
        let error = append_database_profile_text(
            r#"[database]
riko = "postgres://localhost/riko"
"#,
            "riko",
            "postgres://localhost/other",
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "profile 已存在于 [database]: riko");
    }

    #[test]
    fn write_config_text_atomic_replaces_target_content() {
        let root = std::env::temp_dir().join(format!(
            "mem012_atomic_config_write_test_{}",
            std::process::id()
        ));
        let path = root.join("config.toml");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&path, "old").unwrap();

        write_config_text_atomic(&path, "new").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn write_config_text_atomic_preserves_existing_permissions() {
        use std::os::unix::fs::PermissionsExt as _;

        let root = std::env::temp_dir().join(format!(
            "mem012_atomic_config_permission_test_{}",
            std::process::id()
        ));
        let path = root.join("config.toml");
        let temp_path = root.join(format!(".config.toml.{}.tmp", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(&path, "old").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&temp_path, "stale").unwrap();
        std::fs::set_permissions(&temp_path, std::fs::Permissions::from_mode(0o644)).unwrap();

        write_config_text_atomic(&path, "new").unwrap();

        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(mode, 0o600);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn database_profile_helpers_reject_invalid_profile_names() {
        for profile in ["Riko", "riKo", "1riko", "riko-codex"] {
            assert!(
                append_database_profile_text("[database]\n", profile, "postgres://db").is_err()
            );
            assert!(
                derive_profile_database_url("postgres://admin@localhost/postgres", profile, "pw")
                    .is_err()
            );
            assert!(
                derive_admin_profile_database_url("postgres://admin@localhost/postgres", profile)
                    .is_err()
            );
        }
    }

    #[test]
    fn database_profile_helpers_accept_share_profile() {
        let updated =
            append_database_profile_text("[database]\n", "share", "postgres://localhost/mem_share")
                .unwrap();
        let profile_url =
            derive_profile_database_url("postgres://admin@localhost/postgres", "share", "pw")
                .unwrap();
        let admin_url =
            derive_admin_profile_database_url("postgres://admin@localhost/postgres", "share")
                .unwrap();

        assert!(updated.contains("share = \"postgres://localhost/mem_share\""));
        assert_eq!(profile_url, "postgres://share:pw@localhost/mem_share");
        assert_eq!(admin_url, "postgres://admin@localhost/mem_share");
    }

    #[test]
    fn derive_profile_database_url_replaces_user_password_and_database() {
        let url = derive_profile_database_url(
            "postgresql://admin:secret@127.0.0.1:5632/postgres?sslmode=disable",
            "rikocodex",
            "abc_DEF-123",
        )
        .unwrap();

        assert_eq!(
            url,
            "postgresql://rikocodex:abc_DEF-123@127.0.0.1:5632/mem_rikocodex?sslmode=disable"
        );
    }

    #[test]
    fn derive_profile_database_url_rejects_non_postgres_scheme() {
        let error = derive_profile_database_url("http://127.0.0.1/postgres", "rikocodex", "pw")
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "admin database URL 必须使用 postgres/postgresql scheme"
        );
    }

    #[test]
    fn derive_admin_profile_database_url_keeps_admin_credentials() {
        let url = derive_admin_profile_database_url(
            "postgresql://admin:secret@127.0.0.1:5632/postgres?sslmode=disable",
            "rikocodex",
        )
        .unwrap();

        assert_eq!(
            url,
            "postgresql://admin:secret@127.0.0.1:5632/mem_rikocodex?sslmode=disable"
        );
    }

    #[test]
    fn generate_profile_password_is_16_ascii_alphanumeric_with_required_classes() {
        let first = generate_profile_password();
        let second = generate_profile_password();

        assert_eq!(first.len(), 16);
        assert_ne!(first, second);
        assert!(first.bytes().all(|byte| byte.is_ascii_alphanumeric()));
        assert!(first.bytes().any(|byte| byte.is_ascii_uppercase()));
        assert!(first.bytes().any(|byte| byte.is_ascii_lowercase()));
        assert!(first.bytes().any(|byte| byte.is_ascii_digit()));
        assert!(!first.contains(['_', '-']));
    }

    #[test]
    fn admin_database_url_from_env_value_accepts_non_empty_value() {
        assert_eq!(
            admin_database_url_from_env_value(Some(OsString::from(
                " postgresql://admin:secret@127.0.0.1/postgres "
            )))
            .unwrap(),
            "postgresql://admin:secret@127.0.0.1/postgres"
        );
    }

    #[test]
    fn admin_database_url_from_env_value_rejects_missing_or_empty_value() {
        let missing = admin_database_url_from_env_value(None).unwrap_err();
        let empty = admin_database_url_from_env_value(Some(OsString::from("  "))).unwrap_err();

        assert_eq!(
            missing.to_string(),
            "缺少环境变量 MEM012_ADMIN_DATABASE_URL"
        );
        assert_eq!(empty.to_string(), "MEM012_ADMIN_DATABASE_URL 不能为空");
    }
}
