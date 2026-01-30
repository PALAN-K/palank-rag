//! LanceDB Vector Store - 고성능 벡터 검색
//!
//! source: D:\010 Web Applicaton\palan-k\core\src\knowledge\lance.rs (단순화)
//!
//! ANN (Approximate Nearest Neighbor) 검색으로 대용량 벡터에서도 빠른 검색을 지원합니다.
//! ref: https://lancedb.github.io/lancedb/

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use arrow_array::{
    Array, Float32Array, Int32Array, Int64Array, RecordBatch, RecordBatchIterator, StringArray,
    FixedSizeListArray,
};
use arrow_schema::{DataType, Field, Schema};
use async_trait::async_trait;
use lancedb::connection::Connection;
use lancedb::query::{ExecutableQuery, QueryBase};

use super::vector::{SearchResult, VectorEntry, VectorStore, EMBEDDING_DIMENSION};

/// 벡터 테이블 이름
const TABLE_NAME: &str = "vectors";

// ============================================================================
// LanceVectorStore
// ============================================================================

/// LanceDB 벡터 저장소 구현
///
/// LanceDB는 고성능 벡터 검색을 위한 columnar 데이터베이스입니다.
/// Apache Arrow 기반으로 빠른 읽기/쓰기를 제공합니다.
pub struct LanceVectorStore {
    db: Connection,
}

impl LanceVectorStore {
    /// LanceDB 저장소 열기
    ///
    /// # Arguments
    /// * `path` - .lance 디렉토리 경로
    pub async fn open(path: &Path) -> Result<Self> {
        // 부모 디렉토리 생성
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .context("Failed to create LanceDB directory")?;
            }
        }

        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Invalid path encoding"))?;

        let db = lancedb::connect(path_str)
            .execute()
            .await
            .context("Failed to connect to LanceDB")?;

        Ok(Self { db })
    }

    /// 벡터 테이블 스키마 생성
    fn create_schema() -> Schema {
        Schema::new(vec![
            Field::new("doc_id", DataType::Int64, false),
            Field::new("chunk_index", DataType::Int32, false),
            Field::new("chunk_text", DataType::Utf8, false),
            Field::new(
                "embedding",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    EMBEDDING_DIMENSION,
                ),
                false,
            ),
        ])
    }

    /// 엔트리들을 Arrow RecordBatch로 변환
    fn entries_to_batch(entries: &[VectorEntry]) -> Result<RecordBatch> {
        if entries.is_empty() {
            anyhow::bail!("Cannot create batch from empty entries");
        }

        let doc_ids: Vec<i64> = entries.iter().map(|e| e.doc_id).collect();
        let chunk_indices: Vec<i32> = entries.iter().map(|e| e.chunk_index).collect();
        let chunk_texts: Vec<&str> = entries.iter().map(|e| e.chunk_text.as_str()).collect();

        // 임베딩을 FixedSizeList로 변환
        let embeddings_flat: Vec<f32> = entries
            .iter()
            .flat_map(|e| e.embedding.iter().copied())
            .collect();

        let values = Float32Array::from(embeddings_flat);
        let field = Arc::new(Field::new("item", DataType::Float32, true));
        let embeddings_list = FixedSizeListArray::try_new(
            field,
            EMBEDDING_DIMENSION,
            Arc::new(values) as Arc<dyn Array>,
            None,
        )
        .context("Failed to create embedding array")?;

        let batch = RecordBatch::try_new(
            Arc::new(Self::create_schema()),
            vec![
                Arc::new(Int64Array::from(doc_ids)),
                Arc::new(Int32Array::from(chunk_indices)),
                Arc::new(StringArray::from(chunk_texts)),
                Arc::new(embeddings_list),
            ],
        )
        .context("Failed to create RecordBatch")?;

        Ok(batch)
    }

    /// 테이블 존재 여부 확인
    async fn table_exists(&self) -> bool {
        self.db
            .table_names()
            .execute()
            .await
            .map(|names| names.contains(&TABLE_NAME.to_string()))
            .unwrap_or(false)
    }

    /// 테이블 생성 또는 열기
    async fn get_or_create_table(&self, batch: RecordBatch) -> Result<lancedb::table::Table> {
        let schema = batch.schema();
        if self.table_exists().await {
            self.db
                .open_table(TABLE_NAME)
                .execute()
                .await
                .context("Failed to open existing table")
        } else {
            // RecordBatchIterator로 감싸서 전달
            let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);
            self.db
                .create_table(TABLE_NAME, batches)
                .execute()
                .await
                .context("Failed to create table")
        }
    }
}

#[async_trait]
impl VectorStore for LanceVectorStore {
    async fn insert_batch(&self, entries: &[VectorEntry]) -> Result<usize> {
        if entries.is_empty() {
            return Ok(0);
        }

        let batch = Self::entries_to_batch(entries)?;
        let schema = batch.schema();

        if self.table_exists().await {
            // 기존 테이블에 추가
            let table = self
                .db
                .open_table(TABLE_NAME)
                .execute()
                .await
                .context("Failed to open table")?;

            let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);
            table
                .add(batches)
                .execute()
                .await
                .context("Failed to add vectors to table")?;
        } else {
            // 새 테이블 생성
            self.get_or_create_table(batch).await?;
        }

        Ok(entries.len())
    }

    async fn search(&self, query_embedding: &[f32], limit: usize) -> Result<Vec<SearchResult>> {
        if !self.table_exists().await {
            return Ok(vec![]);
        }

        let table = self
            .db
            .open_table(TABLE_NAME)
            .execute()
            .await
            .context("Failed to open table for search")?;

        // 벡터 검색
        let results = table
            .vector_search(query_embedding.to_vec())
            .context("Failed to create vector search")?
            .limit(limit)
            .execute()
            .await
            .context("Failed to execute vector search")?;

        let mut search_results = Vec::new();

        // RecordBatch 스트림에서 결과 추출
        use futures::TryStreamExt;
        let batches: Vec<RecordBatch> = results.try_collect().await?;

        for batch in batches {
            let doc_ids = batch
                .column_by_name("doc_id")
                .and_then(|c| c.as_any().downcast_ref::<Int64Array>())
                .ok_or_else(|| anyhow::anyhow!("Missing doc_id column"))?;

            let chunk_indices = batch
                .column_by_name("chunk_index")
                .and_then(|c| c.as_any().downcast_ref::<Int32Array>())
                .ok_or_else(|| anyhow::anyhow!("Missing chunk_index column"))?;

            let chunk_texts = batch
                .column_by_name("chunk_text")
                .and_then(|c| c.as_any().downcast_ref::<StringArray>())
                .ok_or_else(|| anyhow::anyhow!("Missing chunk_text column"))?;

            // _distance 컬럼 (LanceDB가 자동 추가)
            let distances = batch
                .column_by_name("_distance")
                .and_then(|c| c.as_any().downcast_ref::<Float32Array>())
                .ok_or_else(|| anyhow::anyhow!("Missing _distance column"))?;

            for i in 0..batch.num_rows() {
                let distance = distances.value(i);
                // 거리를 유사도로 변환 (L2 거리 -> 코사인 유사도 근사)
                let similarity = 1.0 / (1.0 + distance);

                search_results.push(SearchResult {
                    doc_id: doc_ids.value(i),
                    chunk_index: chunk_indices.value(i),
                    chunk_text: chunk_texts.value(i).to_string(),
                    similarity,
                });
            }
        }

        Ok(search_results)
    }

    async fn delete_by_doc_id(&self, doc_id: i64) -> Result<usize> {
        if !self.table_exists().await {
            return Ok(0);
        }

        let table = self
            .db
            .open_table(TABLE_NAME)
            .execute()
            .await
            .context("Failed to open table for delete")?;

        // 삭제 전 개수 확인
        let before_count = self.count().await?;

        // doc_id는 i64 타입으로 검증됨 - SQL 인젝션 방지
        let filter = format!("doc_id = {}", doc_id as i64);
        table
            .delete(&filter)
            .await
            .context("Failed to delete vectors")?;

        let after_count = self.count().await?;
        Ok(before_count.saturating_sub(after_count))
    }

    async fn count(&self) -> Result<usize> {
        if !self.table_exists().await {
            return Ok(0);
        }

        let table = self
            .db
            .open_table(TABLE_NAME)
            .execute()
            .await
            .context("Failed to open table for count")?;

        let count = table.count_rows(None).await.context("Failed to count rows")?;
        Ok(count)
    }

    async fn has_embeddings(&self, doc_id: i64) -> Result<bool> {
        if !self.table_exists().await {
            return Ok(false);
        }

        let table = self
            .db
            .open_table(TABLE_NAME)
            .execute()
            .await
            .context("Failed to open table")?;

        // doc_id는 i64 타입으로 검증됨 - SQL 인젝션 방지
        let filter = format!("doc_id = {}", doc_id as i64);
        let count = table
            .count_rows(Some(filter))
            .await
            .context("Failed to count rows for doc_id")?;

        Ok(count > 0)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_entry(doc_id: i64, chunk_index: i32) -> VectorEntry {
        VectorEntry {
            doc_id,
            chunk_index,
            chunk_text: format!("Test chunk {} for doc {}", chunk_index, doc_id),
            embedding: vec![0.1; EMBEDDING_DIMENSION as usize],
        }
    }

    #[tokio::test]
    async fn test_lance_store_basic() {
        let temp_dir = TempDir::new().unwrap();
        let lance_path = temp_dir.path().join("test.lance");

        let store = LanceVectorStore::open(&lance_path).await.unwrap();

        // 초기 상태
        assert_eq!(store.count().await.unwrap(), 0);

        // 삽입
        let entries = vec![create_test_entry(1, 0), create_test_entry(1, 1)];
        let inserted = store.insert_batch(&entries).await.unwrap();
        assert_eq!(inserted, 2);

        // 개수 확인
        assert_eq!(store.count().await.unwrap(), 2);

        // 임베딩 존재 확인
        assert!(store.has_embeddings(1).await.unwrap());
        assert!(!store.has_embeddings(999).await.unwrap());
    }

    #[tokio::test]
    async fn test_lance_search() {
        let temp_dir = TempDir::new().unwrap();
        let lance_path = temp_dir.path().join("search_test.lance");

        let store = LanceVectorStore::open(&lance_path).await.unwrap();

        // 테스트 데이터 삽입
        let entries = vec![
            create_test_entry(1, 0),
            create_test_entry(2, 0),
            create_test_entry(3, 0),
        ];
        store.insert_batch(&entries).await.unwrap();

        // 검색
        let query = vec![0.1; EMBEDDING_DIMENSION as usize];
        let results = store.search(&query, 2).await.unwrap();

        assert!(!results.is_empty());
        assert!(results.len() <= 2);
    }

    #[tokio::test]
    async fn test_lance_delete() {
        let temp_dir = TempDir::new().unwrap();
        let lance_path = temp_dir.path().join("delete_test.lance");

        let store = LanceVectorStore::open(&lance_path).await.unwrap();

        // 삽입
        let entries = vec![
            create_test_entry(1, 0),
            create_test_entry(1, 1),
            create_test_entry(2, 0),
        ];
        store.insert_batch(&entries).await.unwrap();
        assert_eq!(store.count().await.unwrap(), 3);

        // 삭제
        let deleted = store.delete_by_doc_id(1).await.unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(store.count().await.unwrap(), 1);
    }
}
