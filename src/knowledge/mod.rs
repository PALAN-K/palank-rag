//! Knowledge 모듈 - 하이브리드 RAG 지식 저장소
//!
//! source: D:\010 Web Applicaton\palan-k\core\src\knowledge\ (단순화 버전)
//!
//! - SQLite: 텍스트 데이터 저장 + FTS5 키워드 검색
//! - LanceDB: 벡터 검색 (ANN)
//! - Hybrid: RRF 알고리즘으로 두 검색 결과 통합
//! - Chunker: Markdown 인식 텍스트 분할

mod store;
mod vector;
mod lance;
mod hybrid;
mod chunker;

// Re-exports
pub use store::{
    KnowledgeStore, Document, NewDocument, StoreStats, FtsSearchResult,
    get_data_dir,
};
pub use vector::{
    VectorStore, VectorEntry, SearchResult,
    cosine_similarity, chunk_text,
    EMBEDDING_DIMENSION,
};
pub use lance::LanceVectorStore;
pub use hybrid::{HybridRetriever, HybridSearchResult, HybridStats, SearchMethod};
pub use chunker::{
    Chunker, MarkdownChunker, ChunkConfig,
    default_chunker, markdown_chunker,
};
