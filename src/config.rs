use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    database: BTreeMap<String, String>,
    search: SearchConfig,
    embeddings: EmbeddingsConfig,
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

pub struct EmbeddingSettings {
    pub api: String,
    pub key: String,
    pub model: String,
}

#[derive(Deserialize)]
struct EmbeddingsConfig {
    embeddings_api: String,
    embeddings_key: String,
    embeddings_model: Option<String>,
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

    pub fn embedding_settings(&self) -> Option<EmbeddingSettings> {
        // Why：embedding 是派生索引能力，配置为空时应跳过生成而不是阻塞主流程。
        let api = self.embeddings.embeddings_api.trim();
        let key = self.embeddings.embeddings_key.trim();
        if api.is_empty() || key.is_empty() {
            return None;
        }
        Some(EmbeddingSettings {
            api: api.to_string(),
            key: key.to_string(),
            model: self
                .embeddings
                .embeddings_model
                .as_deref()
                .unwrap_or("BAAI/bge-m3")
                .to_string(),
        })
    }
}

// Why：配置读取独立于数据库初始化，避免 IO 错误和数据库错误混在同一层。
pub fn load_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    // 1\ 读取文件 path
    let text = std::fs::read_to_string(path)?;

    // 2、配置读取到struct config
    let config: Config = toml::from_str(&text)?;

    Ok(config)
}
