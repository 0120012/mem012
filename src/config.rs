use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    database: BTreeMap<String, String>,
    #[serde(default)]
    categories: CategoriesConfig,
    search: SearchConfig,
    #[serde(default)]
    rerank: RerankConfig,
    embeddings: EmbeddingsConfig,
    #[serde(default)]
    network: NetworkConfig,
    #[serde(default)]
    #[allow(dead_code)]
    auth: AuthConfig,
    #[serde(default)]
    debug: DebugConfig,
    #[allow(dead_code)]
    cleanup: CleanupConfig,
    server: ServerConfig,
}

#[derive(Deserialize)]
struct SearchConfig {
    default_limit: i32,
    graph_expand_depth: i32,
    keyword: i32,
    fulltext: i32,
    semantic: i32,
    graph: i32,
    stale_penalty: i32,
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
    pub proxy: Option<String>,
}

#[derive(Deserialize)]
struct EmbeddingsConfig {
    embeddings_api: String,
    embeddings_api_type: Option<String>,
    embeddings_key: String,
    embeddings_model: Option<String>,
    embeddings_dimension: Option<usize>,
}

#[derive(Default, Deserialize)]
struct NetworkConfig {
    enable_proxy: bool,
    proxy: Option<String>,
}

#[allow(dead_code)]
#[derive(Default, Deserialize)]
struct AuthConfig {
    #[serde(default)]
    turnstile: TurnstileConfig,
}

#[allow(dead_code)]
#[derive(Default, Deserialize)]
struct TurnstileConfig {
    enabled: bool,
    site_key: Option<String>,
    secret_key: Option<String>,
    verify_url: Option<String>,
}

#[allow(dead_code)]
pub struct TurnstileSettings {
    pub site_key: String,
    pub secret_key: String,
    pub verify_url: String,
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
        // Why：搜索入口必须统一遵守配置上限，避免各工具各自解释 limit。
        self.search.default_limit.max(1)
    }

    pub fn category_index_list(&self) -> &[String] {
        // Why：category 白名单只需要一组可写索引名，避免把展示描述和权限状态混进写入校验。
        &self.categories.index_list
    }

    #[allow(dead_code)]
    pub fn turnstile_settings(&self) -> Option<TurnstileSettings> {
        // Why：Turnstile 只保护 init 授权页；配置不完整时必须保持关闭，不能降级成无验证码。
        if !self.auth.turnstile.enabled {
            return None;
        }
        let site_key = self.auth.turnstile.site_key.as_deref()?.trim();
        let secret_key = self.auth.turnstile.secret_key.as_deref()?.trim();
        if site_key.is_empty() || secret_key.is_empty() {
            return None;
        }
        Some(TurnstileSettings {
            site_key: site_key.to_string(),
            secret_key: secret_key.to_string(),
            verify_url: self
                .auth
                .turnstile
                .verify_url
                .as_deref()
                .unwrap_or("https://challenges.cloudflare.com/turnstile/v0/siteverify")
                .to_string(),
        })
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
    let config_path = std::env::var_os("MEM012_CONFIG")
        .filter(|value| !value.as_os_str().is_empty())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(path));
    let text = std::fs::read_to_string(config_path)?;

    parse_config_text(&text)
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

#[cfg(test)]
mod tests {
    use super::parse_config_text;

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
}
