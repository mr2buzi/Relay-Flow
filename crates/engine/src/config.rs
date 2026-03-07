use anyhow::Result;
use dotenvy::dotenv;
use std::env;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub database_url: String,
    pub server_addr: String,
    pub app_base_url: String,
    pub dashboard_origin: String,
    pub mode: String,
    pub openai_api_key: Option<String>,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let _ = dotenv();
        Ok(Self {
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| {
                "postgres://relayflow:relayflow@localhost:5432/relayflow".to_string()
            }),
            server_addr: env::var("SERVER_ADDR").unwrap_or_else(|_| "127.0.0.1:8000".to_string()),
            app_base_url: env::var("APP_BASE_URL")
                .unwrap_or_else(|_| "http://localhost:8000".to_string()),
            dashboard_origin: env::var("DASHBOARD_ORIGIN")
                .unwrap_or_else(|_| "http://localhost:5173".to_string()),
            mode: env::var("RELAYFLOW_MODE").unwrap_or_else(|_| "demo".to_string()),
            openai_api_key: env::var("OPENAI_API_KEY").ok(),
        })
    }
}
