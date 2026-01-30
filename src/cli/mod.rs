//! CLI 모듈
//!
//! source: D:\010 Web Applicaton\PALAN-K-palank-rag\src\cli\mod.rs
//!
//! palank-rag CLI 명령어 정의 및 구현

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};

use crate::embedding::has_api_key;
use crate::knowledge::{get_data_dir, HybridRetriever, KnowledgeStore, NewDocument};
use crate::scraper::WebScraper;

// ============================================================================
// CLI Definition
// ============================================================================

#[derive(Parser)]
#[command(name = "palank-rag")]
#[command(version, about = "로컬 하이브리드 RAG 시스템", long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// URL 또는 텍스트를 지식베이스에 추가
    Ingest {
        /// 수집할 URL
        #[arg(short, long)]
        url: Option<String>,

        /// 직접 입력할 텍스트
        #[arg(short, long)]
        text: Option<String>,

        /// 프레임워크 태그
        #[arg(short, long)]
        framework: Option<String>,
    },

    /// 지식베이스 검색
    Query {
        /// 검색 쿼리
        query: String,

        /// 결과 개수 제한
        #[arg(short, long, default_value = "5")]
        limit: usize,

        /// 프레임워크 필터 (현재 미구현)
        #[arg(short, long)]
        framework: Option<String>,
    },

    /// 저장된 문서 목록
    List {
        /// 프레임워크 필터
        #[arg(short, long)]
        framework: Option<String>,

        /// 결과 개수 제한
        #[arg(short, long, default_value = "20")]
        limit: usize,
    },

    /// 문서 삭제
    Delete {
        /// 삭제할 문서 URL
        #[arg(short, long)]
        url: Option<String>,

        /// 삭제할 문서 ID
        #[arg(short, long)]
        id: Option<i64>,
    },

    /// 상태 확인
    Status,
}

// ============================================================================
// CLI Runner
// ============================================================================

/// CLI 명령어 실행
pub async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Ingest {
            url,
            text,
            framework,
        } => cmd_ingest(url, text, framework).await,
        Commands::Query {
            query,
            limit,
            framework,
        } => cmd_query(&query, limit, framework).await,
        Commands::List { framework, limit } => cmd_list(framework, limit).await,
        Commands::Delete { url, id } => cmd_delete(url, id).await,
        Commands::Status => cmd_status().await,
    }
}

// ============================================================================
// Command Implementations
// ============================================================================

/// 문서 수집 명령어 (ingest)
///
/// URL에서 콘텐츠를 스크랩하거나 직접 텍스트를 입력받아 지식베이스에 저장합니다.
async fn cmd_ingest(
    url: Option<String>,
    text: Option<String>,
    framework: Option<String>,
) -> Result<()> {
    // API 키 확인
    if !has_api_key() {
        bail!(
            "API 키가 설정되지 않았습니다.\n\n\
             설정 방법:\n  \
             export GEMINI_API_KEY=your-api-key\n  \
             또는\n  \
             export GOOGLE_AI_API_KEY=your-api-key\n\n\
             API 키 발급: https://aistudio.google.com/app/apikey"
        );
    }

    let retriever = HybridRetriever::new()
        .await
        .context("HybridRetriever 초기화 실패")?;

    let (content, source_url, title) = if let Some(ref url_str) = url {
        // URL에서 콘텐츠 스크랩
        println!("[*] URL 스크래핑 중: {}", url_str);

        let scraper = WebScraper::new().context("WebScraper 생성 실패")?;
        let scraped = scraper
            .scrape(url_str)
            .await
            .context("URL 스크래핑 실패")?;

        let content = if let Some(ref title) = scraped.title {
            format!("# {}\n\n{}", title, scraped.content)
        } else {
            scraped.content
        };

        (content, url_str.clone(), scraped.title)
    } else if let Some(ref text_content) = text {
        // 직접 입력된 텍스트
        (text_content.clone(), "direct-input".to_string(), None)
    } else {
        bail!("--url 또는 --text 중 하나를 지정해야 합니다");
    };

    println!("[*] 문서 저장 및 임베딩 생성 중...");

    let doc = NewDocument {
        url: source_url.clone(),
        title,
        content,
        framework,
    };

    let doc_id = retriever
        .add_document(doc)
        .await
        .context("문서 추가 실패")?;

    println!("[OK] 문서가 추가되었습니다 (ID: {})", doc_id);
    println!("     URL: {}", source_url);

    Ok(())
}

/// 검색 명령어 (query)
///
/// 하이브리드 검색 (FTS5 + 벡터)을 사용하여 지식베이스를 검색합니다.
async fn cmd_query(query: &str, limit: usize, _framework: Option<String>) -> Result<()> {
    if !has_api_key() {
        bail!(
            "API 키가 설정되지 않았습니다.\n\
             설정: export GEMINI_API_KEY=your-key"
        );
    }

    println!("[*] 검색 중: \"{}\"", query);

    let retriever = HybridRetriever::new()
        .await
        .context("HybridRetriever 초기화 실패")?;

    let results = retriever.search(query, limit).await.context("검색 실패")?;

    if results.is_empty() {
        println!("\n[!] 검색 결과가 없습니다.");
        return Ok(());
    }

    println!("\n[OK] 검색 결과 ({} 건):\n", results.len());

    for (i, result) in results.iter().enumerate() {
        let method_str = match result.method {
            crate::knowledge::SearchMethod::Vector => "VEC",
            crate::knowledge::SearchMethod::Fts => "FTS",
            crate::knowledge::SearchMethod::Hybrid => "HYB",
        };

        println!(
            "{}. [{}] [점수: {:.4}] Doc #{}",
            i + 1,
            method_str,
            result.rrf_score,
            result.doc_id
        );

        if let Some(ref title) = result.title {
            println!("   제목: {}", title);
        }

        println!("   URL: {}", result.url);

        // 청크 텍스트 또는 스니펫 출력
        if let Some(ref chunk) = result.chunk_text {
            println!("   내용: {}", truncate_text(chunk, 200));
        } else if let Some(ref snippet) = result.snippet {
            println!("   스니펫: {}", truncate_text(snippet, 200));
        }

        println!();
    }

    Ok(())
}

/// 목록 명령어 (list)
///
/// 저장된 문서 목록을 조회합니다.
async fn cmd_list(framework: Option<String>, limit: usize) -> Result<()> {
    let store = KnowledgeStore::open_default().context("KnowledgeStore 열기 실패")?;

    let docs = store
        .list_documents(limit, framework.as_deref())
        .context("문서 목록 조회 실패")?;

    if docs.is_empty() {
        println!("[!] 저장된 문서가 없습니다.");
        return Ok(());
    }

    println!("[OK] 저장된 문서 ({} 건):\n", docs.len());

    for doc in docs {
        let fw = doc.framework.as_deref().unwrap_or("-");
        let title_display = doc
            .title
            .as_ref()
            .map(|t| truncate_text(t, 40))
            .unwrap_or_else(|| "-".to_string());

        println!("  #{:<4} [{}] {}", doc.id, fw, title_display);
        println!("        URL: {}", doc.url);
        println!(
            "        {} | {} chars",
            doc.created_at.format("%Y-%m-%d %H:%M"),
            doc.content.len()
        );
        println!();
    }

    Ok(())
}

/// 삭제 명령어 (delete)
///
/// ID 또는 URL로 문서를 삭제합니다.
async fn cmd_delete(url: Option<String>, id: Option<i64>) -> Result<()> {
    let store = KnowledgeStore::open_default().context("KnowledgeStore 열기 실패")?;

    let doc_id = if let Some(id) = id {
        // ID로 삭제
        id
    } else if let Some(ref url_str) = url {
        // URL로 문서 조회 후 삭제
        let doc = store
            .get_by_url(url_str)
            .context("문서 조회 실패")?
            .ok_or_else(|| anyhow::anyhow!("URL '{}'인 문서를 찾을 수 없습니다", url_str))?;
        doc.id
    } else {
        bail!("--id 또는 --url 중 하나를 지정해야 합니다");
    };

    // 문서 존재 확인
    let doc = store.get_document(doc_id).context("문서 조회 실패")?;

    if doc.is_none() {
        bail!("ID {}인 문서를 찾을 수 없습니다", doc_id);
    }

    // 삭제 수행 (벡터 삭제도 필요하지만 HybridRetriever가 필요)
    // 현재는 SQLite만 삭제 (벡터는 남아있음)
    let deleted = store.delete_document(doc_id).context("문서 삭제 실패")?;

    if deleted {
        println!("[OK] 문서 #{} 삭제됨", doc_id);
        println!("     (주의: 벡터 인덱스는 별도로 정리가 필요할 수 있습니다)");
    } else {
        println!("[!] 삭제할 문서를 찾을 수 없습니다");
    }

    Ok(())
}

/// 상태 명령어 (status)
///
/// 시스템 상태를 확인합니다.
async fn cmd_status() -> Result<()> {
    println!("palank-rag v{}", env!("CARGO_PKG_VERSION"));
    println!();

    // 데이터 디렉토리
    let data_dir = get_data_dir();
    println!("[*] 데이터 디렉토리: {}", data_dir.display());

    // API 키 상태
    if has_api_key() {
        println!("[OK] API 키: 설정됨");
    } else {
        println!("[!] API 키: 미설정");
        println!("    설정: export GEMINI_API_KEY=your-key");
    }

    // 문서 수 및 통계
    match KnowledgeStore::open_default() {
        Ok(store) => match store.stats() {
            Ok(stats) => {
                println!("[OK] 저장된 문서: {} 건", stats.document_count);
                println!(
                    "     총 콘텐츠: {} bytes",
                    format_bytes(stats.total_content_bytes)
                );
            }
            Err(e) => {
                println!("[!] 통계 조회 실패: {}", e);
            }
        },
        Err(e) => {
            println!("[!] KnowledgeStore 열기 실패: {}", e);
        }
    }

    // 벡터 스토어 상태 (API 키가 있을 때만)
    if has_api_key() {
        match HybridRetriever::new().await {
            Ok(retriever) => match retriever.stats().await {
                Ok(stats) => {
                    println!("[OK] 벡터 인덱스: {} 청크", stats.vector_count);
                }
                Err(e) => {
                    tracing::debug!("벡터 통계 조회 실패: {}", e);
                }
            },
            Err(e) => {
                tracing::debug!("HybridRetriever 초기화 실패: {}", e);
            }
        }
    }

    Ok(())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// 텍스트 자르기 (UTF-8 안전)
fn truncate_text(text: &str, max_chars: usize) -> String {
    let cleaned = text.replace('\n', " ").replace('\r', "");
    let cleaned = cleaned.trim();

    if cleaned.chars().count() <= max_chars {
        cleaned.to_string()
    } else {
        let truncated: String = cleaned.chars().take(max_chars).collect();
        format!("{}...", truncated)
    }
}

/// 바이트 크기 포맷팅
fn format_bytes(bytes: usize) -> String {
    const KB: usize = 1024;
    const MB: usize = KB * 1024;

    if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_text() {
        assert_eq!(truncate_text("hello", 10), "hello");
        assert_eq!(truncate_text("hello world", 5), "hello...");
        assert_eq!(truncate_text("hello\nworld", 20), "hello world");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048576), "1.00 MB");
    }

    #[test]
    fn test_truncate_unicode() {
        let korean = "안녕하세요 세계";
        let truncated = truncate_text(korean, 5);
        assert_eq!(truncated, "안녕하세요...");
    }
}
