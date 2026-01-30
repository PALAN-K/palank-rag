//! Vector Store - 벡터 검색 트레이트 및 유틸리티
//!
//! source: D:\010 Web Applicaton\palan-k\core\src\knowledge\vector.rs (단순화)
//!
//! LanceDB ANN (Approximate Nearest Neighbor) 검색을 사용합니다.

use anyhow::Result;
use async_trait::async_trait;

/// 벡터 임베딩 차원 (Gemini gemini-embedding-001 기본값)
/// source: https://ai.google.dev/gemini-api/docs/embeddings
pub const EMBEDDING_DIMENSION: i32 = 768;

// ============================================================================
// Types
// ============================================================================

/// 벡터 엔트리 (저장용)
#[derive(Debug, Clone)]
pub struct VectorEntry {
    /// 문서 ID (documents.id)
    pub doc_id: i64,
    /// 청크 인덱스 (0-based)
    pub chunk_index: i32,
    /// 청크 텍스트
    pub chunk_text: String,
    /// 임베딩 벡터
    pub embedding: Vec<f32>,
}

/// 검색 결과
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// 문서 ID
    pub doc_id: i64,
    /// 청크 인덱스
    pub chunk_index: i32,
    /// 청크 텍스트
    pub chunk_text: String,
    /// 유사도 스코어 (0.0 ~ 1.0)
    pub similarity: f32,
}

// ============================================================================
// VectorStore Trait
// ============================================================================

/// VectorStore 트레이트 (async)
///
/// 벡터 저장소의 공통 인터페이스입니다.
#[async_trait]
pub trait VectorStore: Send + Sync {
    /// 벡터 배치 삽입
    async fn insert_batch(&self, entries: &[VectorEntry]) -> Result<usize>;

    /// 벡터 검색
    async fn search(&self, query_embedding: &[f32], limit: usize) -> Result<Vec<SearchResult>>;

    /// doc_id로 벡터 삭제
    async fn delete_by_doc_id(&self, doc_id: i64) -> Result<usize>;

    /// 벡터 개수 조회
    async fn count(&self) -> Result<usize>;

    /// 특정 doc_id의 임베딩 존재 여부
    async fn has_embeddings(&self, doc_id: i64) -> Result<bool>;
}

// ============================================================================
// Utility Functions
// ============================================================================

/// 코사인 유사도 계산
///
/// 두 벡터 간의 코사인 유사도를 계산합니다.
/// 결과는 -1.0 ~ 1.0 범위입니다.
///
/// # Arguments
/// * `a` - 첫 번째 벡터
/// * `b` - 두 번째 벡터
///
/// # Returns
/// 코사인 유사도 (-1.0 ~ 1.0)
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }

    dot_product / (norm_a * norm_b)
}

/// 텍스트를 청크로 분할
///
/// 문서를 지정된 크기의 청크로 나눕니다.
/// overlap으로 청크 간 중첩 단어 수를 지정합니다.
///
/// # Arguments
/// * `text` - 분할할 텍스트
/// * `chunk_size` - 청크 당 단어 수
/// * `overlap` - 청크 간 중첩 단어 수
///
/// # Returns
/// 청크 문자열 목록
pub fn chunk_text(text: &str, chunk_size: usize, overlap: usize) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();

    if words.is_empty() {
        return vec![];
    }

    if words.len() <= chunk_size {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < words.len() {
        let end = (start + chunk_size).min(words.len());
        let chunk = words[start..end].join(" ");
        chunks.push(chunk);

        if end >= words.len() {
            break;
        }

        start += chunk_size - overlap;
    }

    chunks
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_same() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &b) - 1.0).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let a = vec![1.0, 0.0, 0.0];
        let c = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &c) - 0.0).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_opposite() {
        let a = vec![1.0, 0.0, 0.0];
        let d = vec![-1.0, 0.0, 0.0];
        assert!((cosine_similarity(&a, &d) - -1.0).abs() < 0.0001);
    }

    #[test]
    fn test_cosine_similarity_empty() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        assert_eq!(cosine_similarity(&a, &b), 0.0);
    }

    #[test]
    fn test_chunk_text() {
        let text = "a b c d e f g h i j";
        let chunks = chunk_text(text, 4, 1);

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0], "a b c d");
        assert_eq!(chunks[1], "d e f g");
        assert_eq!(chunks[2], "g h i j");
    }

    #[test]
    fn test_chunk_text_empty() {
        let chunks = chunk_text("", 4, 1);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_text_small() {
        let text = "a b c";
        let chunks = chunk_text(text, 4, 1);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "a b c");
    }

    #[test]
    fn test_chunk_text_no_overlap() {
        let text = "a b c d e f g h";
        let chunks = chunk_text(text, 4, 0);

        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "a b c d");
        assert_eq!(chunks[1], "e f g h");
    }
}
