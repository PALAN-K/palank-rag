//! 임베딩 모듈 - Gemini API를 통한 텍스트 벡터화
//!
//! source: D:\010 Web Applicaton\palan-k\core\src\embedding\mod.rs
//!
//! 텍스트를 벡터로 변환하는 Gemini 임베딩 프로바이더입니다.
//! 시맨틱 검색을 위한 핵심 모듈입니다.
//!
//! ## 사용법
//! ```rust,ignore
//! let embedder = GeminiEmbedding::from_env()?;
//! let embedding = embedder.embed("Hello, world!").await?;
//! ```

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

// ============================================================================
// EmbeddingProvider Trait
// ============================================================================

/// 임베딩 프로바이더 트레이트
///
/// 텍스트를 벡터로 변환하는 인터페이스입니다.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// 단일 텍스트 임베딩
    async fn embed(&self, text: &str) -> Result<Vec<f32>>;

    /// 배치 임베딩 (기본 구현: 순차 호출)
    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut results = Vec::with_capacity(texts.len());
        for text in texts {
            results.push(self.embed(text).await?);
        }
        Ok(results)
    }

    /// 임베딩 차원 수
    fn dimension(&self) -> usize;

    /// 프로바이더 이름
    fn name(&self) -> &str;
}

// ============================================================================
// Google Gemini Embedding
// ============================================================================

/// Gemini 임베딩 API 엔드포인트 (gemini-embedding-001 - MRL 지원)
/// source: https://ai.google.dev/gemini-api/docs/embeddings
const GEMINI_EMBED_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-embedding-001:embedContent";

/// 기본 임베딩 차원
pub const DEFAULT_DIMENSION: usize = 768;

/// Rate Limiter 설정 (Gemini 무료 티어: 60 RPM)
const RATE_LIMIT_RPM: u32 = 60;
const RATE_LIMIT_WINDOW: Duration = Duration::from_secs(60);
/// 호출 간 최소 딜레이 (1000ms = 60 RPM 준수)
const MIN_DELAY_MS: u64 = 1000;
/// 429 에러 시 최대 재시도 횟수
const MAX_RETRIES: u32 = 3;
/// 재시도 시 초기 백오프 (ms)
const INITIAL_BACKOFF_MS: u64 = 2000;

/// Google Gemini 임베딩 구현체
///
/// source: https://ai.google.dev/gemini-api/docs/embeddings
#[derive(Debug)]
pub struct GeminiEmbedding {
    api_key: String,
    client: reqwest::Client,
    dimension: usize,
    rate_limiter: Arc<Mutex<RateLimiter>>,
}

/// Rate Limiter with minimum delay between requests
#[derive(Debug)]
struct RateLimiter {
    requests: Vec<Instant>,
    max_requests: u32,
    window: Duration,
    min_delay: Duration,
    last_request: Option<Instant>,
}

impl RateLimiter {
    fn new(max_requests: u32, window: Duration) -> Self {
        Self {
            requests: Vec::new(),
            max_requests,
            window,
            min_delay: Duration::from_millis(MIN_DELAY_MS),
            last_request: None,
        }
    }

    /// 요청 가능 여부 확인 및 대기
    async fn acquire(&mut self) {
        // 1. 최소 딜레이 적용 (버스트 방지)
        if let Some(last) = self.last_request {
            let elapsed = last.elapsed();
            if elapsed < self.min_delay {
                let wait_time = self.min_delay - elapsed;
                tracing::debug!("Min delay: waiting {:?}", wait_time);
                tokio::time::sleep(wait_time).await;
            }
        }

        let now = Instant::now();

        // 2. 윈도우 밖의 오래된 요청 제거
        self.requests.retain(|&t| now.duration_since(t) < self.window);

        // 3. Rate limit 초과 시 대기
        if self.requests.len() >= self.max_requests as usize {
            if let Some(&oldest) = self.requests.first() {
                let wait_time = self.window - now.duration_since(oldest);
                if !wait_time.is_zero() {
                    tracing::debug!("Rate limit reached, waiting {:?}", wait_time);
                    tokio::time::sleep(wait_time).await;
                }
                // 대기 후 다시 정리
                let now = Instant::now();
                self.requests.retain(|&t| now.duration_since(t) < self.window);
            }
        }

        // 4. 현재 요청 기록
        let now = Instant::now();
        self.requests.push(now);
        self.last_request = Some(now);
    }
}

impl GeminiEmbedding {
    /// 새 Gemini 임베딩 인스턴스 생성
    ///
    /// # Arguments
    /// * `api_key` - Google AI API 키
    pub fn new(api_key: String) -> Result<Self> {
        Self::with_dimension(api_key, DEFAULT_DIMENSION)
    }

    /// 차원을 지정하여 생성
    ///
    /// # Arguments
    /// * `api_key` - Google AI API 키
    /// * `dimension` - 임베딩 차원 (768, 1536, 3072 중 선택)
    pub fn with_dimension(api_key: String, dimension: usize) -> Result<Self> {
        // 유효한 차원 확인
        if ![768, 1536, 3072].contains(&dimension) {
            anyhow::bail!(
                "Invalid dimension: {}. Must be 768, 1536, or 3072",
                dimension
            );
        }

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .context("Failed to create HTTP client")?;

        let rate_limiter = Arc::new(Mutex::new(RateLimiter::new(
            RATE_LIMIT_RPM,
            RATE_LIMIT_WINDOW,
        )));

        Ok(Self {
            api_key,
            client,
            dimension,
            rate_limiter,
        })
    }

    /// 환경변수에서 API 키를 읽어 생성
    ///
    /// 우선순위: GEMINI_API_KEY > GOOGLE_AI_API_KEY
    pub fn from_env() -> Result<Self> {
        let api_key = get_api_key()?;
        Self::new(api_key)
    }

    /// 환경변수에서 API 키를 읽어 차원 지정하여 생성
    pub fn from_env_with_dimension(dimension: usize) -> Result<Self> {
        let api_key = get_api_key()?;
        Self::with_dimension(api_key, dimension)
    }

    /// 임베딩 차원 반환
    pub fn dimension(&self) -> usize {
        self.dimension
    }
}

/// Gemini API 요청 본문
/// source: https://ai.google.dev/gemini-api/docs/embeddings
#[derive(Debug, Serialize)]
struct EmbedRequest {
    model: String,
    content: EmbedContent,
    #[serde(rename = "taskType")]
    task_type: String,
    #[serde(rename = "outputDimensionality", skip_serializing_if = "Option::is_none")]
    output_dimensionality: Option<usize>,
}

#[derive(Debug, Serialize)]
struct EmbedContent {
    parts: Vec<EmbedPart>,
}

#[derive(Debug, Serialize)]
struct EmbedPart {
    text: String,
}

/// Gemini API 응답
#[derive(Debug, Deserialize)]
struct EmbedResponse {
    embedding: EmbeddingValues,
}

#[derive(Debug, Deserialize)]
struct EmbeddingValues {
    values: Vec<f32>,
}

/// Gemini API 에러 응답
#[derive(Debug, Deserialize)]
struct GeminiError {
    error: GeminiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct GeminiErrorDetail {
    message: String,
    #[serde(default)]
    status: String,
}

#[async_trait]
impl EmbeddingProvider for GeminiEmbedding {
    async fn embed(&self, text: &str) -> Result<Vec<f32>> {
        // 빈 텍스트 처리
        if text.trim().is_empty() {
            return Ok(vec![0.0; self.dimension]);
        }

        // 요청 본문 구성
        let request = EmbedRequest {
            model: "models/gemini-embedding-001".to_string(),
            content: EmbedContent {
                parts: vec![EmbedPart {
                    text: text.to_string(),
                }],
            },
            task_type: "RETRIEVAL_DOCUMENT".to_string(),
            output_dimensionality: Some(self.dimension),
        };

        let mut last_error: Option<anyhow::Error> = None;

        // 재시도 루프 (429 에러 시 지수 백오프)
        for attempt in 0..=MAX_RETRIES {
            // Rate limiting (매 시도마다)
            {
                let mut limiter = self.rate_limiter.lock().await;
                limiter.acquire().await;
            }

            // API 호출 (API 키는 URL이 아닌 헤더로 전송 - 보안 강화)
            let response = match self
                .client
                .post(GEMINI_EMBED_URL)
                .header("x-goog-api-key", &self.api_key)
                .json(&request)
                .send()
                .await
            {
                Ok(resp) => resp,
                Err(e) => {
                    last_error = Some(anyhow::anyhow!("Failed to send embedding request: {}", e));
                    if attempt < MAX_RETRIES {
                        let backoff = Duration::from_millis(INITIAL_BACKOFF_MS * 2u64.pow(attempt));
                        tracing::warn!(
                            "Request failed, retrying in {:?} (attempt {}/{})",
                            backoff,
                            attempt + 1,
                            MAX_RETRIES
                        );
                        tokio::time::sleep(backoff).await;
                        continue;
                    }
                    break;
                }
            };

            let status = response.status();
            let body = response
                .text()
                .await
                .context("Failed to read response body")?;

            // 성공
            if status.is_success() {
                let embed_response: EmbedResponse =
                    serde_json::from_str(&body).context("Failed to parse embedding response")?;
                return Ok(embed_response.embedding.values);
            }

            // 429 Rate Limit 에러 - 재시도
            if status.as_u16() == 429 {
                let backoff = Duration::from_millis(INITIAL_BACKOFF_MS * 2u64.pow(attempt));
                tracing::warn!(
                    "Rate limit hit (429), backing off {:?} (attempt {}/{})",
                    backoff,
                    attempt + 1,
                    MAX_RETRIES
                );
                last_error = Some(anyhow::anyhow!("Rate limit exceeded (429)"));

                if attempt < MAX_RETRIES {
                    tokio::time::sleep(backoff).await;
                    continue;
                }
            } else {
                // 다른 에러 - 즉시 실패
                if let Ok(error) = serde_json::from_str::<GeminiError>(&body) {
                    anyhow::bail!(
                        "Gemini API error ({}): {}",
                        error.error.status,
                        error.error.message
                    );
                }
                anyhow::bail!("Gemini API error ({}): {}", status, body);
            }
        }

        // 모든 재시도 실패
        Err(last_error
            .unwrap_or_else(|| anyhow::anyhow!("Embedding failed after {} retries", MAX_RETRIES)))
    }

    async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        // Gemini는 배치 API가 없으므로 순차 처리
        // Rate limiter가 자동으로 조절함
        let mut results = Vec::with_capacity(texts.len());

        for (i, text) in texts.iter().enumerate() {
            tracing::debug!("Embedding batch {}/{}", i + 1, texts.len());
            results.push(self.embed(text).await?);
        }

        Ok(results)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn name(&self) -> &str {
        "gemini-embedding-001"
    }
}

// ============================================================================
// API Key Management
// ============================================================================

/// API 키 로드 (환경변수에서)
///
/// 우선순위:
/// 1. `GEMINI_API_KEY` 환경변수
/// 2. `GOOGLE_AI_API_KEY` 환경변수
pub fn get_api_key() -> Result<String> {
    // 1. GEMINI_API_KEY 확인
    if let Ok(key) = std::env::var("GEMINI_API_KEY") {
        if !key.is_empty() {
            tracing::debug!("Using API key from GEMINI_API_KEY");
            return Ok(key);
        }
    }

    // 2. GOOGLE_AI_API_KEY 확인 (대체)
    if let Ok(key) = std::env::var("GOOGLE_AI_API_KEY") {
        if !key.is_empty() {
            tracing::debug!("Using API key from GOOGLE_AI_API_KEY");
            return Ok(key);
        }
    }

    anyhow::bail!(
        "API key not found. Set GEMINI_API_KEY or GOOGLE_AI_API_KEY environment variable.\n\
         Get your API key at: https://aistudio.google.com/app/apikey"
    )
}

/// API 키 존재 여부 확인
pub fn has_api_key() -> bool {
    if let Ok(key) = std::env::var("GEMINI_API_KEY") {
        if !key.is_empty() {
            return true;
        }
    }

    if let Ok(key) = std::env::var("GOOGLE_AI_API_KEY") {
        if !key.is_empty() {
            return true;
        }
    }

    false
}

// ============================================================================
// Factory Function
// ============================================================================

/// 임베딩 프로바이더 생성 (Gemini API)
///
/// 환경변수에서 API 키를 읽어 GeminiEmbedding을 생성합니다.
pub fn create_embedder() -> Result<GeminiEmbedding> {
    create_embedder_with_dimension(DEFAULT_DIMENSION)
}

/// 차원을 지정하여 임베딩 프로바이더 생성
pub fn create_embedder_with_dimension(dimension: usize) -> Result<GeminiEmbedding> {
    if !has_api_key() {
        anyhow::bail!(
            "GEMINI_API_KEY or GOOGLE_AI_API_KEY not set.\n\
             Set: export GEMINI_API_KEY=your-api-key\n\
             Get your API key at: https://aistudio.google.com/app/apikey"
        );
    }

    let embedder = GeminiEmbedding::from_env_with_dimension(dimension)?;
    tracing::info!(
        "Using Gemini API embedding (dimension: {})",
        embedder.dimension()
    );
    Ok(embedder)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_api_key() {
        // 환경변수 설정 여부에 따라 결과가 달라짐
        let _ = has_api_key();
    }

    #[test]
    fn test_invalid_dimension() {
        let result = GeminiEmbedding::with_dimension("fake_key".to_string(), 999);
        assert!(result.is_err());
        let err = result.err();
        assert!(err.is_some());
        assert!(err
            .as_ref()
            .map(|e| e.to_string().contains("Invalid dimension"))
            .unwrap_or(false));
    }

    #[test]
    fn test_valid_dimensions() {
        for dim in [768, 1536, 3072] {
            let result = GeminiEmbedding::with_dimension("fake_key".to_string(), dim);
            assert!(result.is_ok());
        }
    }

    #[tokio::test]
    async fn test_create_embedder_without_key_returns_error() {
        // 환경변수 제거 (테스트용)
        std::env::remove_var("GEMINI_API_KEY");
        std::env::remove_var("GOOGLE_AI_API_KEY");

        let result = create_embedder();
        assert!(result.is_err());
        let err = result.err();
        assert!(err.is_some());
    }
}
