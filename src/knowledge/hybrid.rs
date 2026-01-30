//! 하이브리드 검색 - FTS5 + LanceDB RRF 통합
//!
//! source: D:\010 Web Applicaton\palan-k\core\src\knowledge\hybrid.rs (단순화)
//!
//! RRF (Reciprocal Rank Fusion) 알고리즘으로
//! 키워드 검색(FTS5)과 벡터 검색(LanceDB)을 통합합니다.
//!
//! ref: https://www.elastic.co/blog/hybrid-search-rrf

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::embedding::{EmbeddingProvider, GeminiEmbedding};

use super::chunker::{default_chunker, Chunker};
use super::lance::LanceVectorStore;
use super::store::{get_data_dir, FtsSearchResult, KnowledgeStore, NewDocument};
use super::vector::{SearchResult, VectorEntry, VectorStore};

// ============================================================================
// Types
// ============================================================================

/// 하이브리드 검색 결과
#[derive(Debug, Clone)]
pub struct HybridSearchResult {
    /// 문서 ID
    pub doc_id: i64,
    /// 문서 URL
    pub url: String,
    /// 문서 제목
    pub title: Option<String>,
    /// 관련 청크 텍스트 (벡터 검색 결과)
    pub chunk_text: Option<String>,
    /// 콘텐츠 스니펫 (FTS5 결과)
    pub snippet: Option<String>,
    /// RRF 통합 스코어 (높을수록 좋음)
    pub rrf_score: f32,
    /// 검색 방법 (vector, fts, hybrid)
    pub method: SearchMethod,
}

/// 검색 방법
#[derive(Debug, Clone, PartialEq)]
pub enum SearchMethod {
    /// 벡터 검색만 사용
    Vector,
    /// FTS5 키워드 검색만 사용
    Fts,
    /// 하이브리드 (RRF 통합)
    Hybrid,
}

// ============================================================================
// HybridRetriever
// ============================================================================

/// 하이브리드 검색기
///
/// SQLite FTS5 (키워드) + LanceDB (벡터)를 RRF로 통합합니다.
pub struct HybridRetriever {
    store: KnowledgeStore,
    vector: LanceVectorStore,
    embedder: GeminiEmbedding,
    chunker: Box<dyn Chunker>,
}

impl HybridRetriever {
    /// 새 하이브리드 검색기 생성
    ///
    /// 기본 데이터 디렉토리(~/.palank-rag/)를 사용합니다.
    pub async fn new() -> Result<Self> {
        let data_dir = get_data_dir();
        Self::with_data_dir(&data_dir).await
    }

    /// 지정된 데이터 디렉토리로 생성
    ///
    /// # Arguments
    /// * `data_dir` - 데이터 저장 디렉토리
    pub async fn with_data_dir(data_dir: &Path) -> Result<Self> {
        // 디렉토리 생성
        if !data_dir.exists() {
            std::fs::create_dir_all(data_dir)
                .context("Failed to create data directory")?;
        }

        // SQLite 저장소
        let db_path = data_dir.join("knowledge.db");
        let store = KnowledgeStore::open(&db_path)
            .context("Failed to open knowledge store")?;

        // LanceDB 벡터 저장소
        let lance_path = data_dir.join("vectors.lance");
        let vector = LanceVectorStore::open(&lance_path).await
            .context("Failed to open vector store")?;

        // Gemini 임베딩
        let embedder = GeminiEmbedding::from_env()
            .context("Failed to create embedder")?;

        // 청커
        let chunker = default_chunker();

        Ok(Self {
            store,
            vector,
            embedder,
            chunker,
        })
    }

    /// 문서 추가 (자동 임베딩)
    ///
    /// 문서를 SQLite에 저장하고, 청킹 후 LanceDB에 임베딩을 저장합니다.
    ///
    /// # Arguments
    /// * `doc` - 새 문서
    ///
    /// # Returns
    /// 문서 ID
    pub async fn add_document(&self, doc: NewDocument) -> Result<i64> {
        // 1. SQLite에 문서 저장
        let doc_id = self.store.add_document(doc.clone())
            .context("Failed to add document to store")?;

        // 2. 텍스트 청킹
        let chunks = self.chunker.chunk(&doc.content);
        if chunks.is_empty() {
            tracing::warn!("No chunks generated for document: {}", doc.url);
            return Ok(doc_id);
        }

        // 3. 임베딩 생성 및 저장
        let mut entries = Vec::with_capacity(chunks.len());

        for (i, chunk) in chunks.iter().enumerate() {
            let embedding = self.embedder.embed(chunk).await
                .context("Failed to embed chunk")?;

            entries.push(VectorEntry {
                doc_id,
                chunk_index: i as i32,
                chunk_text: chunk.clone(),
                embedding,
            });
        }

        self.vector.insert_batch(&entries).await
            .context("Failed to insert vectors")?;

        tracing::info!(
            "Added document: {} (id={}, chunks={})",
            doc.url, doc_id, entries.len()
        );

        Ok(doc_id)
    }

    /// 문서 삭제
    ///
    /// SQLite와 LanceDB에서 모두 삭제합니다.
    pub async fn delete_document(&self, doc_id: i64) -> Result<bool> {
        // 벡터 먼저 삭제
        self.vector.delete_by_doc_id(doc_id).await?;

        // SQLite에서 삭제
        self.store.delete_document(doc_id)
    }

    /// 하이브리드 검색 (RRF 통합)
    ///
    /// FTS5와 벡터 검색을 RRF 알고리즘으로 통합합니다.
    ///
    /// # Arguments
    /// * `query` - 검색 쿼리
    /// * `limit` - 최대 결과 수
    ///
    /// # Returns
    /// RRF 스코어 기준 정렬된 검색 결과
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<HybridSearchResult>> {
        // 1. FTS5 키워드 검색
        let fts_results = self.store.search_fts(query, limit * 2)?;

        // 2. 벡터 검색
        let query_embedding = self.embedder.embed(query).await?;
        let vector_results = self.vector.search(&query_embedding, limit * 2).await?;

        // 3. RRF 통합
        let merged = self.rrf_merge(&fts_results, &vector_results, limit);

        Ok(merged)
    }

    /// 벡터 검색만 수행
    pub async fn search_vector(&self, query: &str, limit: usize) -> Result<Vec<HybridSearchResult>> {
        let query_embedding = self.embedder.embed(query).await?;
        let results = self.vector.search(&query_embedding, limit).await?;

        let mut hybrid_results = Vec::with_capacity(results.len());

        for result in results {
            let doc = self.store.get_document(result.doc_id)?;
            let (url, title) = doc.map(|d| (d.url, d.title)).unwrap_or_default();

            hybrid_results.push(HybridSearchResult {
                doc_id: result.doc_id,
                url,
                title,
                chunk_text: Some(result.chunk_text),
                snippet: None,
                rrf_score: result.similarity,
                method: SearchMethod::Vector,
            });
        }

        Ok(hybrid_results)
    }

    /// FTS5 키워드 검색만 수행
    pub fn search_fts(&self, query: &str, limit: usize) -> Result<Vec<HybridSearchResult>> {
        let results = self.store.search_fts(query, limit)?;

        let mut hybrid_results = Vec::with_capacity(results.len());

        for result in results {
            let doc = self.store.get_document(result.doc_id)?;
            let (url, title) = doc.map(|d| (d.url, d.title)).unwrap_or_default();

            // BM25 스코어 정규화 (음수 -> 양수)
            let normalized_score = 1.0 / (1.0 + result.bm25_score.abs()) as f32;

            hybrid_results.push(HybridSearchResult {
                doc_id: result.doc_id,
                url,
                title,
                chunk_text: None,
                snippet: Some(result.content_snippet),
                rrf_score: normalized_score,
                method: SearchMethod::Fts,
            });
        }

        Ok(hybrid_results)
    }

    /// RRF (Reciprocal Rank Fusion) 알고리즘
    ///
    /// 두 검색 결과를 순위 기반으로 통합합니다.
    /// ref: https://www.elastic.co/blog/hybrid-search-rrf
    ///
    /// RRF Score = sum(1 / (k + rank))
    /// k = 60 (기본값, 높은 순위에 더 많은 가중치)
    fn rrf_merge(
        &self,
        fts_results: &[FtsSearchResult],
        vector_results: &[SearchResult],
        limit: usize,
    ) -> Vec<HybridSearchResult> {
        const K: f32 = 60.0;

        // doc_id -> (rrf_score, fts_result, vector_result)
        let mut scores: HashMap<i64, (f32, Option<&FtsSearchResult>, Option<&SearchResult>)> =
            HashMap::new();

        // FTS5 결과 추가
        for (rank, result) in fts_results.iter().enumerate() {
            let rrf_score = 1.0 / (K + rank as f32 + 1.0);
            let entry = scores.entry(result.doc_id).or_insert((0.0, None, None));
            entry.0 += rrf_score;
            entry.1 = Some(result);
        }

        // 벡터 결과 추가
        for (rank, result) in vector_results.iter().enumerate() {
            let rrf_score = 1.0 / (K + rank as f32 + 1.0);
            let entry = scores.entry(result.doc_id).or_insert((0.0, None, None));
            entry.0 += rrf_score;
            entry.2 = Some(result);
        }

        // 결과 생성 및 정렬
        let mut results: Vec<(i64, f32, Option<&FtsSearchResult>, Option<&SearchResult>)> = scores
            .into_iter()
            .map(|(doc_id, (score, fts, vec))| (doc_id, score, fts, vec))
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);

        // HybridSearchResult로 변환
        results
            .into_iter()
            .map(|(doc_id, rrf_score, fts_opt, vec_opt)| {
                let doc = self.store.get_document(doc_id).ok().flatten();
                let (url, title) = doc.map(|d| (d.url, d.title)).unwrap_or_default();

                let method = match (fts_opt.is_some(), vec_opt.is_some()) {
                    (true, true) => SearchMethod::Hybrid,
                    (true, false) => SearchMethod::Fts,
                    (false, true) => SearchMethod::Vector,
                    (false, false) => SearchMethod::Hybrid, // shouldn't happen
                };

                HybridSearchResult {
                    doc_id,
                    url,
                    title,
                    chunk_text: vec_opt.map(|v| v.chunk_text.clone()),
                    snippet: fts_opt.map(|f| f.content_snippet.clone()),
                    rrf_score,
                    method,
                }
            })
            .collect()
    }

    /// 저장소 통계
    pub async fn stats(&self) -> Result<HybridStats> {
        let store_stats = self.store.stats()?;
        let vector_count = self.vector.count().await?;

        Ok(HybridStats {
            document_count: store_stats.document_count,
            vector_count,
            total_content_bytes: store_stats.total_content_bytes,
        })
    }

    /// 내부 스토어 접근
    pub fn store(&self) -> &KnowledgeStore {
        &self.store
    }

    /// 내부 벡터 스토어 접근
    pub fn vector_store(&self) -> &LanceVectorStore {
        &self.vector
    }
}

/// 하이브리드 저장소 통계
#[derive(Debug, Clone)]
pub struct HybridStats {
    pub document_count: usize,
    pub vector_count: usize,
    pub total_content_bytes: usize,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_method_equality() {
        assert_eq!(SearchMethod::Vector, SearchMethod::Vector);
        assert_ne!(SearchMethod::Vector, SearchMethod::Fts);
        assert_ne!(SearchMethod::Fts, SearchMethod::Hybrid);
    }

    #[test]
    fn test_rrf_score_calculation() {
        // RRF 스코어 공식 테스트: 1 / (k + rank + 1)
        const K: f32 = 60.0;

        // 1위: 1 / (60 + 0 + 1) = 1/61 ≈ 0.0164
        let score_rank_1 = 1.0 / (K + 0.0 + 1.0);
        assert!((score_rank_1 - 0.0164).abs() < 0.001);

        // 5위: 1 / (60 + 4 + 1) = 1/65 ≈ 0.0154
        let score_rank_5 = 1.0 / (K + 4.0 + 1.0);
        assert!((score_rank_5 - 0.0154).abs() < 0.001);

        // 순위가 높을수록 스코어가 높음
        assert!(score_rank_1 > score_rank_5);
    }
}
