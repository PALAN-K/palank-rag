//! PDF 텍스트 추출 모듈
//!
//! pdf-extract 크레이트를 사용하여 PDF에서 텍스트를 추출합니다.

use std::path::Path;

use anyhow::{Context, Result};

/// PDF에서 텍스트 추출
///
/// 페이지별로 텍스트를 추출하여 (페이지 번호, 텍스트) 튜플 벡터로 반환합니다.
/// 페이지 번호는 1부터 시작합니다.
pub fn extract_text_from_pdf(path: &Path) -> Result<Vec<(usize, String)>> {
    // PDF 파일 열기
    let bytes = std::fs::read(path).with_context(|| format!("Failed to read PDF: {:?}", path))?;

    // 전체 텍스트 추출
    let text = pdf_extract::extract_text_from_mem(&bytes)
        .with_context(|| format!("Failed to extract text from PDF: {:?}", path))?;

    // 텍스트가 비어있으면 경고
    if text.trim().is_empty() {
        tracing::warn!(
            "No text extracted from PDF: {:?}. It might be a scanned document.",
            path
        );
        return Ok(vec![(1, String::new())]);
    }

    // 페이지 분리 시도 (폼피드 문자 또는 페이지 구분자로)
    let pages = split_pdf_pages(&text);

    if pages.is_empty() {
        // 페이지 분리 실패 시 전체 텍스트를 1페이지로
        Ok(vec![(1, text)])
    } else {
        Ok(pages
            .into_iter()
            .enumerate()
            .map(|(i, text)| (i + 1, text))
            .collect())
    }
}

/// PDF 텍스트를 페이지별로 분리
fn split_pdf_pages(text: &str) -> Vec<String> {
    // 폼피드 문자 (\x0c)로 페이지 분리 시도
    let pages: Vec<String> = text
        .split('\x0c')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if pages.len() > 1 {
        return pages;
    }

    // 페이지 구분자 패턴으로 시도 (일부 PDF에서 사용)
    // 예: "--- Page 1 ---" 또는 숫자만 있는 줄
    let page_pattern = regex::Regex::new(r"(?m)^[\s]*[-=]+[\s]*(?:Page[\s]*)?(\d+)[\s]*[-=]+[\s]*$")
        .expect("Invalid regex");

    if page_pattern.is_match(text) {
        let pages: Vec<String> = page_pattern
            .split(text)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        if pages.len() > 1 {
            return pages;
        }
    }

    // 분리 실패 - 전체를 하나의 페이지로
    vec![text.to_string()]
}

/// PDF 페이지 수 추정 (텍스트 길이 기반)
#[allow(dead_code)]
fn estimate_page_count(text: &str) -> usize {
    // 평균적으로 한 페이지당 약 3000자 정도로 추정
    const CHARS_PER_PAGE: usize = 3000;
    let char_count = text.chars().count();
    (char_count / CHARS_PER_PAGE).max(1)
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_pdf_pages_with_formfeed() {
        let text = "Page 1 content\x0cPage 2 content\x0cPage 3 content";
        let pages = split_pdf_pages(text);
        assert_eq!(pages.len(), 3);
        assert_eq!(pages[0], "Page 1 content");
        assert_eq!(pages[1], "Page 2 content");
    }

    #[test]
    fn test_split_pdf_pages_no_separator() {
        let text = "Just some text without page breaks";
        let pages = split_pdf_pages(text);
        assert_eq!(pages.len(), 1);
    }

    #[test]
    fn test_estimate_page_count() {
        let short_text = "Hello world";
        assert_eq!(estimate_page_count(short_text), 1);

        let long_text = "a".repeat(6000);
        assert_eq!(estimate_page_count(&long_text), 2);
    }
}
