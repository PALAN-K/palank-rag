//! Knowledge Store - rusqlite 기반 동기 지식 저장소
//!
//! source: D:\010 Web Applicaton\palan-k\core\src\knowledge\store.rs (단순화)
//!
//! 학습된 지식(URL에서 가져온 콘텐츠)을 저장하고 검색합니다.
//! 저장 위치: ~/.palank-rag/knowledge.db

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OpenFlags};
use serde::{Deserialize, Serialize};

// ============================================================================
// Data Directory
// ============================================================================

/// 데이터 디렉토리 경로 (~/.palank-rag/)
pub fn get_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".palank-rag")
}

// ============================================================================
// Types
// ============================================================================

/// 저장된 문서 엔트리
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: i64,
    pub url: String,
    pub title: Option<String>,
    pub content: String,
    pub framework: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// 새 문서 입력용 구조체
#[derive(Debug, Clone)]
pub struct NewDocument {
    pub url: String,
    pub title: Option<String>,
    pub content: String,
    pub framework: Option<String>,
}

/// FTS5 검색 결과
#[derive(Debug, Clone)]
pub struct FtsSearchResult {
    pub doc_id: i64,
    pub title: Option<String>,
    pub content_snippet: String,
    pub bm25_score: f64,
}

/// 저장소 통계
#[derive(Debug, Clone, Serialize)]
pub struct StoreStats {
    pub document_count: usize,
    pub total_content_bytes: usize,
    pub db_path: PathBuf,
}

// ============================================================================
// KnowledgeStore
// ============================================================================

/// Knowledge Store - 동기 지식 저장소
///
/// SQLite 기반 문서 저장 및 FTS5 키워드 검색을 제공합니다.
pub struct KnowledgeStore {
    conn: Arc<Mutex<Connection>>,
    db_path: PathBuf,
}

impl KnowledgeStore {
    /// 저장소 열기 (없으면 생성)
    ///
    /// # Arguments
    /// * `path` - DB 파일 경로 (없으면 생성)
    pub fn open(path: &Path) -> Result<Self> {
        // 부모 디렉토리 생성
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent)
                    .context("Failed to create database directory")?;
            }
        }

        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE
                | OpenFlags::SQLITE_OPEN_CREATE
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .context("Failed to open SQLite database")?;

        let store = Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path: path.to_path_buf(),
        };

        store.initialize()?;
        Ok(store)
    }

    /// 기본 위치에서 열기 (~/.palank-rag/knowledge.db)
    pub fn open_default() -> Result<Self> {
        let data_dir = get_data_dir();
        if !data_dir.exists() {
            std::fs::create_dir_all(&data_dir)
                .context("Failed to create data directory")?;
        }

        let db_path = data_dir.join("knowledge.db");
        Self::open(&db_path)
    }

    /// DB 경로 반환
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// 스키마 초기화
    fn initialize(&self) -> Result<()> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        // 메인 테이블 생성
        conn.execute(
            "CREATE TABLE IF NOT EXISTS documents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                url TEXT NOT NULL UNIQUE,
                title TEXT,
                content TEXT NOT NULL,
                framework TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
            [],
        )
        .context("Failed to create documents table")?;

        // URL 인덱스
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_documents_url ON documents(url)",
            [],
        )
        .context("Failed to create URL index")?;

        // Framework 인덱스
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_documents_framework ON documents(framework)",
            [],
        )
        .context("Failed to create framework index")?;

        // FTS5 가상 테이블 (키워드 검색용)
        // source: https://www.sqlite.org/fts5.html
        let fts_result = conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
                title,
                content,
                content=documents,
                content_rowid=id
            )",
            [],
        );

        if let Err(e) = fts_result {
            tracing::warn!("FTS5 not available (optional): {}", e);
        } else {
            // FTS5 동기화 트리거
            let _ = conn.execute_batch(
                r#"
                CREATE TRIGGER IF NOT EXISTS documents_ai AFTER INSERT ON documents BEGIN
                    INSERT INTO documents_fts(rowid, title, content)
                    VALUES (new.id, new.title, new.content);
                END;

                CREATE TRIGGER IF NOT EXISTS documents_ad AFTER DELETE ON documents BEGIN
                    INSERT INTO documents_fts(documents_fts, rowid, title, content)
                    VALUES('delete', old.id, old.title, old.content);
                END;

                CREATE TRIGGER IF NOT EXISTS documents_au AFTER UPDATE ON documents BEGIN
                    INSERT INTO documents_fts(documents_fts, rowid, title, content)
                    VALUES('delete', old.id, old.title, old.content);
                    INSERT INTO documents_fts(rowid, title, content)
                    VALUES (new.id, new.title, new.content);
                END;
                "#,
            );
        }

        tracing::debug!("Knowledge store initialized at {:?}", self.db_path);
        Ok(())
    }

    /// 문서 저장 (URL이 같으면 업데이트)
    pub fn add_document(&self, doc: NewDocument) -> Result<i64> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let now = Utc::now().to_rfc3339();

        conn.execute(
            "INSERT OR REPLACE INTO documents (url, title, content, framework, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![doc.url, doc.title, doc.content, doc.framework, now],
        )
        .context("Failed to insert document")?;

        let id = conn.last_insert_rowid();
        tracing::info!("Added document: {} (id={})", doc.url, id);

        Ok(id)
    }

    /// ID로 문서 조회
    pub fn get_document(&self, id: i64) -> Result<Option<Document>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT id, url, title, content, framework, created_at FROM documents WHERE id = ?1",
        )?;

        let doc = stmt
            .query_row(params![id], |row| {
                Ok(Document {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    content: row.get(3)?,
                    framework: row.get(4)?,
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                })
            })
            .ok();

        Ok(doc)
    }

    /// URL로 문서 조회
    pub fn get_by_url(&self, url: &str) -> Result<Option<Document>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        let mut stmt = conn.prepare(
            "SELECT id, url, title, content, framework, created_at FROM documents WHERE url = ?1",
        )?;

        let doc = stmt
            .query_row(params![url], |row| {
                Ok(Document {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    content: row.get(3)?,
                    framework: row.get(4)?,
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                })
            })
            .ok();

        Ok(doc)
    }

    /// 문서 목록 조회
    pub fn list_documents(&self, limit: usize, framework: Option<&str>) -> Result<Vec<Document>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        let docs: Vec<Document> = if let Some(fw) = framework {
            let mut stmt = conn.prepare(
                "SELECT id, url, title, content, framework, created_at FROM documents
                 WHERE framework = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2",
            )?;

            let rows = stmt.query_map(params![fw, limit as i64], |row| {
                Ok(Document {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    content: row.get(3)?,
                    framework: row.get(4)?,
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                })
            })?;

            rows.filter_map(|r| r.ok()).collect()
        } else {
            let mut stmt = conn.prepare(
                "SELECT id, url, title, content, framework, created_at FROM documents
                 ORDER BY created_at DESC
                 LIMIT ?1",
            )?;

            let rows = stmt.query_map(params![limit as i64], |row| {
                Ok(Document {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    content: row.get(3)?,
                    framework: row.get(4)?,
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                })
            })?;

            rows.filter_map(|r| r.ok()).collect()
        };

        Ok(docs)
    }

    /// 문서 삭제
    pub fn delete_document(&self, id: i64) -> Result<bool> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        let rows = conn.execute("DELETE FROM documents WHERE id = ?1", params![id])?;

        Ok(rows > 0)
    }

    /// FTS5 키워드 검색
    ///
    /// BM25 알고리즘으로 스코어링된 검색 결과를 반환합니다.
    /// source: https://www.sqlite.org/fts5.html#the_bm25_function
    pub fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<FtsSearchResult>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        // FTS5 쿼리 이스케이프
        let escaped_query = escape_fts5_query(query);
        if escaped_query.is_empty() {
            return Ok(vec![]);
        }

        let mut stmt = conn.prepare(
            r#"
            SELECT
                d.id as doc_id,
                d.title,
                snippet(documents_fts, 1, '<b>', '</b>', '...', 64) as content_snippet,
                bm25(documents_fts) as bm25_score
            FROM documents_fts
            JOIN documents d ON d.id = documents_fts.rowid
            WHERE documents_fts MATCH ?1
            ORDER BY bm25(documents_fts)
            LIMIT ?2
            "#,
        )?;

        let results = stmt
            .query_map(params![escaped_query, limit as i64], |row| {
                Ok(FtsSearchResult {
                    doc_id: row.get(0)?,
                    title: row.get(1)?,
                    content_snippet: row.get(2)?,
                    bm25_score: row.get(3)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(results)
    }

    /// 간단한 LIKE 검색 (FTS5 사용 불가 시 폴백)
    pub fn search_like(&self, keyword: &str, limit: usize) -> Result<Vec<Document>> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        let pattern = format!("%{}%", keyword.to_lowercase());

        let mut stmt = conn.prepare(
            "SELECT id, url, title, content, framework, created_at FROM documents
             WHERE LOWER(content) LIKE ?1 OR LOWER(title) LIKE ?1
             ORDER BY created_at DESC
             LIMIT ?2",
        )?;

        let docs = stmt
            .query_map(params![pattern, limit as i64], |row| {
                Ok(Document {
                    id: row.get(0)?,
                    url: row.get(1)?,
                    title: row.get(2)?,
                    content: row.get(3)?,
                    framework: row.get(4)?,
                    created_at: parse_datetime(row.get::<_, String>(5)?),
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(docs)
    }

    /// 저장소 통계
    pub fn stats(&self) -> Result<StoreStats> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM documents",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        let total_size: i64 = conn.query_row(
            "SELECT COALESCE(SUM(LENGTH(content)), 0) FROM documents",
            [],
            |row| row.get(0),
        ).unwrap_or(0);

        Ok(StoreStats {
            document_count: count as usize,
            total_content_bytes: total_size as usize,
            db_path: self.db_path.clone(),
        })
    }

    /// FTS5 인덱스 리빌드
    ///
    /// 트리거가 동작하지 않는 경우 수동으로 인덱스를 재생성합니다.
    pub fn rebuild_fts_index(&self) -> Result<usize> {
        let conn = self.conn.lock().map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        // 기존 인덱스 삭제
        conn.execute("DELETE FROM documents_fts", [])?;

        // 재삽입
        let count = conn.execute(
            r#"
            INSERT INTO documents_fts(rowid, title, content)
            SELECT id, COALESCE(title, ''), content
            FROM documents
            "#,
            [],
        )?;

        tracing::info!("Rebuilt FTS5 index with {} documents", count);
        Ok(count)
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// RFC3339 문자열을 DateTime<Utc>로 파싱
fn parse_datetime(s: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

/// FTS5 쿼리 이스케이프
///
/// 특수 문자를 제거하고 단어만 추출합니다.
/// source: https://www.sqlite.org/fts5.html#full_text_query_syntax
fn escape_fts5_query(query: &str) -> String {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    // 특수 문자 제거 후 단어 조합
    trimmed
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .map(|w| {
            w.chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_store() -> (TempDir, KnowledgeStore) {
        let dir = TempDir::new().unwrap();
        let db_path = dir.path().join("test.db");
        let store = KnowledgeStore::open(&db_path).unwrap();
        (dir, store)
    }

    #[test]
    fn test_add_and_get_document() {
        let (_dir, store) = create_test_store();

        let doc = NewDocument {
            url: "https://example.com/doc".to_string(),
            title: Some("Example Doc".to_string()),
            content: "This is test content".to_string(),
            framework: Some("rust".to_string()),
        };

        let id = store.add_document(doc).unwrap();
        assert!(id > 0);

        let retrieved = store.get_document(id).unwrap();
        assert!(retrieved.is_some());

        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.title, Some("Example Doc".to_string()));
        assert_eq!(retrieved.framework, Some("rust".to_string()));
    }

    #[test]
    fn test_get_by_url() {
        let (_dir, store) = create_test_store();

        store.add_document(NewDocument {
            url: "https://example.com/test".to_string(),
            title: Some("Test".to_string()),
            content: "Content".to_string(),
            framework: None,
        }).unwrap();

        let doc = store.get_by_url("https://example.com/test").unwrap();
        assert!(doc.is_some());

        let doc = store.get_by_url("https://nonexistent.com").unwrap();
        assert!(doc.is_none());
    }

    #[test]
    fn test_list_documents() {
        let (_dir, store) = create_test_store();

        for i in 0..5 {
            store.add_document(NewDocument {
                url: format!("https://example.com/doc{}", i),
                title: Some(format!("Doc {}", i)),
                content: format!("Content {}", i),
                framework: if i % 2 == 0 { Some("rust".to_string()) } else { None },
            }).unwrap();
        }

        // 전체 목록
        let list = store.list_documents(10, None).unwrap();
        assert_eq!(list.len(), 5);

        // Framework 필터
        let rust_list = store.list_documents(10, Some("rust")).unwrap();
        assert_eq!(rust_list.len(), 3); // 0, 2, 4
    }

    #[test]
    fn test_delete_document() {
        let (_dir, store) = create_test_store();

        let id = store.add_document(NewDocument {
            url: "https://example.com/to-delete".to_string(),
            title: None,
            content: "To be deleted".to_string(),
            framework: None,
        }).unwrap();

        assert!(store.get_document(id).unwrap().is_some());

        let deleted = store.delete_document(id).unwrap();
        assert!(deleted);

        assert!(store.get_document(id).unwrap().is_none());
    }

    #[test]
    fn test_stats() {
        let (_dir, store) = create_test_store();

        store.add_document(NewDocument {
            url: "https://example.com/test".to_string(),
            title: Some("Test".to_string()),
            content: "1234567890".to_string(), // 10 bytes
            framework: None,
        }).unwrap();

        let stats = store.stats().unwrap();
        assert_eq!(stats.document_count, 1);
        assert_eq!(stats.total_content_bytes, 10);
    }

    #[test]
    fn test_search_like() {
        let (_dir, store) = create_test_store();

        store.add_document(NewDocument {
            url: "https://example.com/react".to_string(),
            title: Some("React Guide".to_string()),
            content: "React is a JavaScript library".to_string(),
            framework: Some("react".to_string()),
        }).unwrap();

        store.add_document(NewDocument {
            url: "https://example.com/vue".to_string(),
            title: Some("Vue Guide".to_string()),
            content: "Vue is a JavaScript framework".to_string(),
            framework: Some("vue".to_string()),
        }).unwrap();

        let results = store.search_like("JavaScript", 10).unwrap();
        assert_eq!(results.len(), 2);

        let results = store.search_like("React", 10).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_escape_fts5_query() {
        assert_eq!(escape_fts5_query("hello world"), "hello world");
        assert_eq!(escape_fts5_query("  "), "");
        assert_eq!(escape_fts5_query("hello:world"), "helloworld");
        assert_eq!(escape_fts5_query("test-query_123"), "test-query_123");
    }
}
