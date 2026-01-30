//! palank-rag - 로컬 하이브리드 RAG 시스템
//!
//! LanceDB 벡터 검색 + SQLite FTS5 키워드 검색을 결합한
//! 하이브리드 RAG 시스템입니다.
//!
//! source: D:\010 Web Applicaton\PALAN-K-palank-rag

pub mod cli;
pub mod embedding;
pub mod knowledge;
pub mod scraper;

// Re-exports
pub use embedding::{EmbeddingProvider, GeminiEmbedding, get_api_key, has_api_key};
pub use knowledge::{
    ChunkConfig, Chunker, Document, FtsSearchResult, HybridRetriever, HybridSearchResult,
    HybridStats, KnowledgeStore, LanceVectorStore, MarkdownChunker, NewDocument, SearchMethod,
    SearchResult, StoreStats, VectorEntry, VectorStore, default_chunker, get_data_dir,
    markdown_chunker,
};
pub use scraper::{ScrapedContent, WebScraper};
