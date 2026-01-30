//! 이미지 텍스트 추출 모듈
//!
//! Gemini Vision API를 사용하여 이미지에서 텍스트를 추출합니다.

use std::path::Path;

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};

/// Gemini Vision API 엔드포인트
const GEMINI_VISION_URL: &str =
    "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.0-flash-exp:generateContent";

/// 이미지에서 텍스트 추출
pub async fn extract_text_from_image(path: &Path, api_key: &str) -> Result<String> {
    // 1. 이미지 파일 읽기
    let image_data = tokio::fs::read(path)
        .await
        .with_context(|| format!("Failed to read image: {:?}", path))?;

    // 2. MIME 타입 결정
    let mime_type = get_mime_type(path)?;

    // 3. Base64 인코딩
    let base64_image = STANDARD.encode(&image_data);

    // 4. API 요청 구성
    let request = VisionRequest {
        contents: vec![VisionContent {
            parts: vec![
                VisionPart::Text {
                    text: EXTRACTION_PROMPT.to_string(),
                },
                VisionPart::InlineData {
                    inline_data: InlineData {
                        mime_type: mime_type.to_string(),
                        data: base64_image,
                    },
                },
            ],
        }],
        generation_config: GenerationConfig {
            temperature: 0.1,
            max_output_tokens: 8192,
        },
    };

    // 5. API 호출
    let client = reqwest::Client::new();
    let response = client
        .post(GEMINI_VISION_URL)
        .header("x-goog-api-key", api_key)
        .json(&request)
        .send()
        .await
        .context("Failed to send Vision API request")?;

    let status = response.status();
    let body = response.text().await?;

    if !status.is_success() {
        anyhow::bail!("Vision API error ({}): {}", status, body);
    }

    // 6. 응답 파싱
    let vision_response: VisionResponse =
        serde_json::from_str(&body).context("Failed to parse Vision API response")?;

    // 7. 텍스트 추출
    let text = vision_response
        .candidates
        .into_iter()
        .next()
        .and_then(|c| c.content.parts.into_iter().next())
        .map(|p| p.text)
        .unwrap_or_default();

    if text.is_empty() {
        tracing::warn!("No text extracted from image: {:?}", path);
    }

    Ok(text)
}

/// 파일 경로에서 MIME 타입 결정
fn get_mime_type(path: &Path) -> Result<&'static str> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();

    match ext.as_str() {
        "png" => Ok("image/png"),
        "jpg" | "jpeg" => Ok("image/jpeg"),
        "webp" => Ok("image/webp"),
        "gif" => Ok("image/gif"),
        "bmp" => Ok("image/bmp"),
        _ => anyhow::bail!("Unsupported image format: {}", ext),
    }
}

/// 이미지 텍스트 추출 프롬프트
const EXTRACTION_PROMPT: &str = r#"이 이미지에서 모든 텍스트 콘텐츠를 추출해주세요.

지시사항:
1. 이미지에 보이는 모든 텍스트를 추출합니다
2. 문서, 다이어그램, 코드, 표 등 모든 텍스트를 포함합니다
3. 원본 구조와 형식을 최대한 유지합니다
4. 마크다운 형식으로 출력합니다
5. 텍스트가 없으면 "[이미지에 텍스트 없음]"이라고 응답합니다

추출된 텍스트:"#;

// ============================================================================
// API Types
// ============================================================================

#[derive(Debug, Serialize)]
struct VisionRequest {
    contents: Vec<VisionContent>,
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,
}

#[derive(Debug, Serialize)]
struct VisionContent {
    parts: Vec<VisionPart>,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum VisionPart {
    Text {
        text: String,
    },
    InlineData {
        #[serde(rename = "inlineData")]
        inline_data: InlineData,
    },
}

#[derive(Debug, Serialize)]
struct InlineData {
    #[serde(rename = "mimeType")]
    mime_type: String,
    data: String,
}

#[derive(Debug, Serialize)]
struct GenerationConfig {
    temperature: f32,
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,
}

#[derive(Debug, Deserialize)]
struct VisionResponse {
    candidates: Vec<Candidate>,
}

#[derive(Debug, Deserialize)]
struct Candidate {
    content: CandidateContent,
}

#[derive(Debug, Deserialize)]
struct CandidateContent {
    parts: Vec<TextPart>,
}

#[derive(Debug, Deserialize)]
struct TextPart {
    text: String,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_mime_type() {
        assert_eq!(get_mime_type(Path::new("test.png")).unwrap(), "image/png");
        assert_eq!(get_mime_type(Path::new("test.jpg")).unwrap(), "image/jpeg");
        assert_eq!(get_mime_type(Path::new("test.JPEG")).unwrap(), "image/jpeg");
        assert!(get_mime_type(Path::new("test.exe")).is_err());
    }
}
