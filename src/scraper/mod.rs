//! 웹 스크래퍼 모듈 - URL 콘텐츠 추출
//!
//! source: D:\010 Web Applicaton\palan-k\core\src\scraper.rs (단순화)
//!
//! palan-k의 복잡한 ContentClassifier, DomainSelectors, RateLimiter 등을 제거하고
//! 순수 HTML 콘텐츠 추출에만 집중합니다.

use anyhow::{Context, Result};
use scraper::{Html, Selector};

/// 스크랩된 콘텐츠
#[derive(Debug, Clone)]
pub struct ScrapedContent {
    /// 페이지 제목
    pub title: Option<String>,
    /// 본문 텍스트 (HTML 태그 제거됨)
    pub content: String,
    /// 원본 URL
    pub url: String,
}

/// 웹 스크래퍼
pub struct WebScraper {
    client: reqwest::Client,
}

impl WebScraper {
    /// 새 스크래퍼 생성
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent("palank-rag/0.1")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .context("HTTP 클라이언트 생성 실패")?;

        Ok(Self { client })
    }

    /// URL에서 콘텐츠 추출
    pub async fn scrape(&self, url: &str) -> Result<ScrapedContent> {
        tracing::info!("Scraping: {}", url);

        let response = self
            .client
            .get(url)
            .send()
            .await
            .context("HTTP 요청 실패")?;

        let html = response.text().await.context("응답 본문 읽기 실패")?;

        let document = Html::parse_document(&html);

        // 제목 추출
        let title = self.extract_title(&document);

        // 본문 추출
        let content = self.extract_content(&document);

        Ok(ScrapedContent {
            title,
            content,
            url: url.to_string(),
        })
    }

    /// 제목 추출
    fn extract_title(&self, document: &Html) -> Option<String> {
        // <title> 태그
        if let Ok(title_selector) = Selector::parse("title") {
            if let Some(element) = document.select(&title_selector).next() {
                let title = element.text().collect::<String>().trim().to_string();
                if !title.is_empty() {
                    return Some(title);
                }
            }
        }

        // <h1> 태그
        if let Ok(h1_selector) = Selector::parse("h1") {
            if let Some(element) = document.select(&h1_selector).next() {
                let title = element.text().collect::<String>().trim().to_string();
                if !title.is_empty() {
                    return Some(title);
                }
            }
        }

        None
    }

    /// 본문 추출 (HTML 태그 제거)
    fn extract_content(&self, document: &Html) -> String {
        // 우선순위: article > main > body
        let selectors = [
            "article",
            "main",
            "[role=main]",
            ".content",
            "#content",
            "body",
        ];

        for selector_str in selectors {
            if let Ok(selector) = Selector::parse(selector_str) {
                if let Some(element) = document.select(&selector).next() {
                    let text = self.extract_text_from_element(&element);
                    if text.len() > 100 {
                        return text;
                    }
                }
            }
        }

        // 폴백: 전체 body 텍스트
        if let Ok(selector) = Selector::parse("body") {
            if let Some(element) = document.select(&selector).next() {
                return self.extract_text_from_element(&element);
            }
        }

        String::new()
    }

    /// 요소에서 텍스트 추출 (스크립트/스타일 제외)
    fn extract_text_from_element(&self, element: &scraper::ElementRef) -> String {
        let mut text = String::new();

        for node in element.text() {
            let trimmed = node.trim();
            if !trimmed.is_empty() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(trimmed);
            }
        }

        // 연속 공백 정리
        if let Ok(re) = regex::Regex::new(r"\s+") {
            re.replace_all(&text, " ").trim().to_string()
        } else {
            text.split_whitespace().collect::<Vec<_>>().join(" ")
        }
    }
}

impl Default for WebScraper {
    fn default() -> Self {
        Self::new().unwrap_or_else(|e| {
            tracing::error!("WebScraper 생성 실패: {}", e);
            // 최소한의 클라이언트로 폴백
            Self {
                client: reqwest::Client::new(),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scraper_creation() {
        let scraper = WebScraper::new();
        assert!(scraper.is_ok());
    }

    #[test]
    fn test_extract_title() {
        let scraper = WebScraper::new().expect("scraper creation failed");
        let html = r#"
            <html>
                <head><title>Test Page Title</title></head>
                <body><h1>Main Heading</h1></body>
            </html>
        "#;
        let document = Html::parse_document(html);
        let title = scraper.extract_title(&document);
        assert_eq!(title, Some("Test Page Title".to_string()));
    }

    #[test]
    fn test_extract_title_h1_fallback() {
        let scraper = WebScraper::new().expect("scraper creation failed");
        let html = r#"
            <html>
                <head><title></title></head>
                <body><h1>H1 Heading</h1></body>
            </html>
        "#;
        let document = Html::parse_document(html);
        let title = scraper.extract_title(&document);
        assert_eq!(title, Some("H1 Heading".to_string()));
    }

    #[test]
    fn test_extract_content_from_article() {
        let scraper = WebScraper::new().expect("scraper creation failed");
        let html = r#"
            <html>
                <body>
                    <nav>Navigation menu</nav>
                    <article>
                        This is the main article content.
                        It should be extracted as the primary content.
                        More text to ensure it's over 100 characters.
                    </article>
                    <footer>Footer content</footer>
                </body>
            </html>
        "#;
        let document = Html::parse_document(html);
        let content = scraper.extract_content(&document);
        assert!(content.contains("main article content"));
    }

    #[test]
    fn test_extract_content_from_main() {
        let scraper = WebScraper::new().expect("scraper creation failed");
        let html = r#"
            <html>
                <body>
                    <nav>Navigation</nav>
                    <main>
                        Main content area with important information.
                        This should be the extracted content.
                        Adding more text to exceed the 100 character threshold.
                    </main>
                </body>
            </html>
        "#;
        let document = Html::parse_document(html);
        let content = scraper.extract_content(&document);
        assert!(content.contains("Main content area"));
    }

    #[test]
    fn test_default_implementation() {
        let scraper = WebScraper::default();
        // Default should create a valid scraper
        let html = "<html><body><title>Test</title></body></html>";
        let document = Html::parse_document(html);
        let _ = scraper.extract_title(&document);
    }
}
