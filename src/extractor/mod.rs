//! 콘텐츠 추출 모듈
//!
//! 다양한 파일 형식에서 텍스트 콘텐츠를 추출합니다.
//! - 텍스트 파일: 직접 읽기
//! - 이미지 파일: Gemini Vision API로 텍스트 추출
//! - PDF 파일: pdf-extract로 텍스트 추출

pub mod image;
pub mod pdf;

use std::path::Path;

use anyhow::{Context, Result};

use crate::collector::FileType;

// ============================================================================
// Extracted Content
// ============================================================================

/// 추출된 콘텐츠
#[derive(Debug, Clone)]
pub struct ExtractedContent {
    /// 추출된 텍스트
    pub text: String,
    /// 원본 파일 타입
    pub source_type: FileType,
    /// 메타데이터 (PDF 페이지 번호 등)
    pub metadata: ContentMetadata,
}

/// 콘텐츠 메타데이터
#[derive(Debug, Clone, Default)]
pub struct ContentMetadata {
    /// PDF 페이지 번호 (1부터 시작)
    pub page_number: Option<usize>,
    /// 총 페이지 수 (PDF)
    pub total_pages: Option<usize>,
    /// 이미지 설명 (Vision API에서 추출)
    pub image_description: Option<String>,
}

// ============================================================================
// Content Extractor
// ============================================================================

/// 콘텐츠 추출기
pub struct ContentExtractor {
    /// Gemini API 키
    api_key: Option<String>,
}

impl ContentExtractor {
    /// API 키로 추출기 생성
    pub fn new(api_key: Option<String>) -> Self {
        Self { api_key }
    }

    /// 환경변수에서 API 키 로드
    pub fn from_env() -> Self {
        let api_key = crate::embedding::get_api_key().ok();
        Self::new(api_key)
    }

    /// 파일에서 콘텐츠 추출
    pub async fn extract(&self, path: &Path, file_type: FileType) -> Result<Vec<ExtractedContent>> {
        match file_type {
            FileType::Text => self.extract_text(path).await,
            FileType::Image => self.extract_image(path).await,
            FileType::Pdf => self.extract_pdf(path).await,
        }
    }

    /// 텍스트 파일에서 추출
    async fn extract_text(&self, path: &Path) -> Result<Vec<ExtractedContent>> {
        let text = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read text file: {:?}", path))?;

        Ok(vec![ExtractedContent {
            text,
            source_type: FileType::Text,
            metadata: ContentMetadata::default(),
        }])
    }

    /// 이미지 파일에서 추출 (Gemini Vision)
    async fn extract_image(&self, path: &Path) -> Result<Vec<ExtractedContent>> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("API key required for image extraction"))?;

        let text = image::extract_text_from_image(path, api_key).await?;

        Ok(vec![ExtractedContent {
            text,
            source_type: FileType::Image,
            metadata: ContentMetadata {
                image_description: Some("Extracted via Gemini Vision".to_string()),
                ..Default::default()
            },
        }])
    }

    /// PDF 파일에서 추출
    async fn extract_pdf(&self, path: &Path) -> Result<Vec<ExtractedContent>> {
        // PDF 추출은 CPU 바운드이므로 spawn_blocking 사용
        let path = path.to_path_buf();
        let pages = tokio::task::spawn_blocking(move || pdf::extract_text_from_pdf(&path))
            .await
            .context("PDF extraction task failed")??;

        let total_pages = pages.len();

        Ok(pages
            .into_iter()
            .map(|(page_num, text)| ExtractedContent {
                text,
                source_type: FileType::Pdf,
                metadata: ContentMetadata {
                    page_number: Some(page_num),
                    total_pages: Some(total_pages),
                    ..Default::default()
                },
            })
            .collect())
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_metadata_default() {
        let meta = ContentMetadata::default();
        assert!(meta.page_number.is_none());
        assert!(meta.total_pages.is_none());
    }
}
