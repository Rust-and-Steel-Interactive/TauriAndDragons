use crate::llm::LlmProvider;
use crate::engine::commands::LlmResponse;
use async_trait::async_trait;
use tauri::{AppHandle, Emitter};

pub struct MockLlm;

#[async_trait]
impl LlmProvider for MockLlm {
    async fn generate_response(&self, app: AppHandle, prompt: String) -> Result<LlmResponse, String> {
        let fake_response = format!(
            "[MOCK LLM] Processing: '{}'. A cold wind blows through the crypt...",
            prompt
        );

        for token in fake_response.split_whitespace() {
            app.emit("llm-token", &format!("{} ", token)).unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        app.emit("llm-done", ()).unwrap();

        Ok(LlmResponse {
            narration: fake_response,
            commands: vec![],
        })
    }
}