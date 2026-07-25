pub mod gemma;
pub mod mock;
pub mod parser;

use async_trait::async_trait;
use std::sync::Arc;
use tauri::AppHandle;
use tokio::sync::RwLock;

#[async_trait]
pub trait LlmProvider: Send + Sync {
    // Changed: Now returns the parsed LlmResponse instead of just streaming to UI
    async fn generate_response(&self, app: AppHandle, prompt: String) -> Result<crate::engine::commands::LlmResponse, String>;
}

pub struct LlmManager {
    pub active_engine: RwLock<Arc<dyn LlmProvider>>,
}

impl LlmManager {
    pub fn new(mock: Arc<dyn LlmProvider>) -> Self {
        Self {
            active_engine: RwLock::new(mock),
        }
    }
}