use crate::llm::LlmProvider;
use crate::llm::parser::parse_llm_output;
use crate::engine::commands::LlmResponse;
use async_trait::async_trait;
use futures_util::StreamExt;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaModel};
use llama_cpp_2::sampling::LlamaSampler;
use serde::Serialize;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const HF_REPO_OWNER: &str = "google";
const HF_REPO_NAME: &str = "gemma-4-E4B-it-qat-q4_0-gguf";
const HF_FILE: &str = "gemma-4-E4B_q4_0-it.gguf";

const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_STALL_TIMEOUT: Duration = Duration::from_secs(20);
const DOWNLOAD_MAX_RETRIES: u32 = 5;

pub struct GemmaEngine {
    model: Arc<LlamaModel>,
}

// Initialize the C backend exactly once
static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

/// Emitted to the frontend during download so an init screen can show
/// real progress (and real retries) instead of a frozen spinner.
#[derive(Serialize, Clone)]
struct GemmaDownloadStatus<'a> {
    attempt: u32,
    max_attempts: u32,
    status: &'a str, // "starting" | "retrying" | "complete" | "failed"
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    message: Option<String>,
}

fn emit_status(
    app: &AppHandle,
    attempt: u32,
    status: &str,
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    message: Option<String>,
) {
    let _ = app.emit(
        "gemma-download-status",
        GemmaDownloadStatus {
            attempt,
            max_attempts: DOWNLOAD_MAX_RETRIES,
            status,
            downloaded_bytes,
            total_bytes,
            message,
        },
    );
}

enum DownloadAttemptError {
    Fatal(String),
    Retryable(String),
}

/// One attempt at downloading (or resuming) `target_path`. Resumes from
/// an existing `.part` file via `Range: bytes=N-` when possible; falls
/// back to a full restart if the server ignores the Range header (i.e.
/// doesn't reply 206).
async fn try_download_once(
    app: &AppHandle,
    client: &reqwest::Client,
    url: &str,
    part_path: &Path,
    attempt: u32,
) -> Result<u64, DownloadAttemptError> {
    let already_have = tokio::fs::metadata(part_path)
        .await
        .map(|m| m.len())
        .unwrap_or(0);

    let mut request = client.get(url);
    if already_have > 0 {
        request = request.header("Range", format!("bytes={already_have}-"));
    }

    let response = request
        .send()
        .await
        .map_err(|e| DownloadAttemptError::Retryable(format!("download request failed: {e}")))?;

    let status = response.status();
    if !status.is_success() {
        return Err(DownloadAttemptError::Fatal(format!(
            "download failed: server returned {status}"
        )));
    }

    let is_resumed_range = already_have > 0 && status.as_u16() == 206;
    let should_truncate = already_have > 0 && !is_resumed_range;

    let content_length = response.content_length();
    let total_bytes = if is_resumed_range {
        content_length.map(|remaining| already_have + remaining)
    } else {
        content_length
    };

    let mut file = if should_truncate || already_have == 0 {
        tokio::fs::File::create(part_path)
            .await
            .map_err(|e| DownloadAttemptError::Fatal(format!("failed to create temp file: {e}")))?
    } else {
        tokio::fs::OpenOptions::new()
            .append(true)
            .open(part_path)
            .await
            .map_err(|e| DownloadAttemptError::Fatal(format!("failed to reopen temp file: {e}")))?
    };

    let mut downloaded: u64 = if should_truncate { 0 } else { already_have };
    let mut stream = response.bytes_stream();
    let mut last_emit = std::time::Instant::now();

    use tokio::io::AsyncWriteExt;
    loop {
        let next_chunk = tokio::time::timeout(DOWNLOAD_STALL_TIMEOUT, stream.next()).await;

        let chunk = match next_chunk {
            Ok(Some(Ok(chunk))) => chunk,
            Ok(Some(Err(e))) => {
                return Err(DownloadAttemptError::Retryable(format!(
                    "download stream error: {e}"
                )))
            }
            Ok(None) => break,
            Err(_elapsed) => {
                return Err(DownloadAttemptError::Retryable(format!(
                    "no data received for {}s, connection appears stalled",
                    DOWNLOAD_STALL_TIMEOUT.as_secs()
                )))
            }
        };

        file.write_all(&chunk).await.map_err(|e| {
            DownloadAttemptError::Fatal(format!("failed to write temp file: {e}"))
        })?;
        downloaded += chunk.len() as u64;

        if last_emit.elapsed().as_millis() >= 100 {
            emit_status(app, attempt, "downloading", downloaded, total_bytes, None);
            last_emit = std::time::Instant::now();
        }
    }

    file.flush()
        .await
        .map_err(|e| DownloadAttemptError::Fatal(e.to_string()))?;
    drop(file);

    emit_status(app, attempt, "downloading", downloaded, total_bytes, None);
    Ok(downloaded)
}

/// Downloads `HF_FILE` straight to `target_path`, retrying stalled/dropped
/// attempts by resuming from the `.part` file. Renames to `target_path`
/// only once the final size is confirmed to match, so a crash mid-download
/// never leaves a truncated file that looks valid on next launch.
async fn download_with_retry(app: &AppHandle, target_path: &Path) -> anyhow::Result<()> {
    let part_path = target_path.with_extension("part");
    let url = format!("https://huggingface.co/{HF_REPO_OWNER}/{HF_REPO_NAME}/resolve/main/{HF_FILE}");

    let client = reqwest::Client::builder()
        .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
        .build()
        .map_err(|e| anyhow::anyhow!("failed to build HTTP client: {e}"))?;

    let mut last_err = String::new();
    for attempt in 1..=DOWNLOAD_MAX_RETRIES {
        emit_status(
            app,
            attempt,
            if attempt == 1 { "starting" } else { "retrying" },
            0,
            None,
            (attempt > 1).then(|| last_err.clone()),
        );

        match try_download_once(app, &client, &url, &part_path, attempt).await {
            Ok(_) => {
                tokio::fs::rename(&part_path, target_path)
                    .await
                    .map_err(|e| anyhow::anyhow!("failed to finalize downloaded model: {e}"))?;
                emit_status(app, attempt, "complete", 0, None, None);
                return Ok(());
            }
            Err(DownloadAttemptError::Fatal(msg)) => {
                let _ = tokio::fs::remove_file(&part_path).await;
                emit_status(app, attempt, "failed", 0, None, Some(msg.clone()));
                return Err(anyhow::anyhow!(msg));
            }
            Err(DownloadAttemptError::Retryable(msg)) => {
                println!("Download attempt {attempt}/{DOWNLOAD_MAX_RETRIES} failed: {msg}");
                last_err = msg;
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }

    let _ = tokio::fs::remove_file(&part_path).await;
    let final_message = format!(
        "failed to download '{HF_FILE}' from '{HF_REPO_OWNER}/{HF_REPO_NAME}' after \
         {DOWNLOAD_MAX_RETRIES} attempts: {last_err}"
    );
    emit_status(app, DOWNLOAD_MAX_RETRIES, "failed", 0, None, Some(final_message.clone()));
    Err(anyhow::anyhow!(final_message))
}

impl GemmaEngine {
    /// Synchronous, blocking load (downloads + loads the model into memory).
    pub fn load(app: &AppHandle) -> anyhow::Result<Self> {
        let backend = BACKEND.get_or_init(|| LlamaBackend::init().expect("failed to init llama backend"));

        let local_dir = dirs::data_dir()
            .ok_or_else(|| anyhow::anyhow!("Could not find data directory"))?
            .join("com.tauridragons.app/models");
        std::fs::create_dir_all(&local_dir)?;
        let local_path: PathBuf = local_dir.join(HF_FILE);

        if !local_path.exists() {
            println!("Model not found locally. Downloading from HuggingFace...");
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| anyhow::anyhow!("failed to create download runtime: {e}"))?;
            rt.block_on(download_with_retry(app, &local_path))?;
            println!("Model downloaded successfully to {:?}", local_path);
        }

        println!("Loading model into memory...");
        let params = LlamaModelParams::default().with_n_gpu_layers(1000); // Offload all to Metal
        let model = LlamaModel::load_from_file(backend, &local_path, &params)?;
        println!("Gemma Engine ready!");
        
        // Wrap in Arc so it can be moved into spawn_blocking later
        Ok(Self { model: Arc::new(model) })
    }

    /// Async-friendly wrapper
    pub async fn load_async(app: &AppHandle) -> anyhow::Result<Self> {
        let app = app.clone();
        tokio::task::spawn_blocking(move || Self::load(&app)).await?
    }
}

#[async_trait]
impl LlmProvider for GemmaEngine {
    async fn generate_response(&self, app: AppHandle, prompt: String) -> Result<LlmResponse, String> {
        let app_handle = app.clone();
        
        // Clone the Arc so we have an owned pointer to move into the blocking thread
        let model_clone = self.model.clone();
        
        // 1. Run the CPU-heavy C++ generation in a blocking thread to avoid Send issues
        let raw_output = tokio::task::spawn_blocking(move || -> Result<String, String> {
            let backend = BACKEND.get().expect("backend not initialized");
            let formatted_prompt = format!(
                "<start_of_turn>user\n{}<end_of_turn>\n<start_of_turn>model\n",
                prompt
            );

            let ctx_params = LlamaContextParams::default().with_n_ctx(NonZeroU32::new(4096));
            let mut ctx = model_clone.new_context(backend, ctx_params).map_err(|e| e.to_string())?;

            let tokens = model_clone.str_to_token(&formatted_prompt, AddBos::Always).map_err(|e| e.to_string())?;
            let n_prompt = tokens.len();

            let mut batch = LlamaBatch::new(4096, 1);
            for (i, &tok) in tokens.iter().enumerate() {
                let is_last = i == n_prompt - 1;
                batch.add(tok, i as i32, &[0], is_last).map_err(|e| e.to_string())?;
            }
            ctx.decode(&mut batch).map_err(|e| e.to_string())?;

            let mut sampler = LlamaSampler::chain_simple([
                LlamaSampler::temp(0.1),
                LlamaSampler::dist(1234),
            ]);

            let mut n_cur = n_prompt as i32;
            let mut raw_output = String::new();

            for _ in 0..512 {
                let token = sampler.sample(&ctx, batch.n_tokens() - 1);
                sampler.accept(token);

                if model_clone.is_eog_token(token) {
                    break;
                }

                let bytes = model_clone.token_to_piece(token, &mut encoding_rs::UTF_8.new_decoder(), true, None).unwrap_or_default();
                raw_output.push_str(&bytes);

                batch.clear();
                batch.add(token, n_cur, &[0], true).map_err(|e| e.to_string())?;
                ctx.decode(&mut batch).map_err(|e| e.to_string())?;
                n_cur += 1;
            }
            
            Ok(raw_output)
        })
        .await
        .map_err(|e| format!("Task join error: {}", e))??;

        // 2. Parse the JSON
        let parsed = parse_llm_output(&raw_output)?;

        // 3. Stream the narration to the UI word-by-word
        for word in parsed.narration.split_whitespace() {
            let _ = app_handle.emit("llm-token", &format!("{} ", word));
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        let _ = app_handle.emit("llm-done", ());
        Ok(parsed)
    }
}