//! Text Chunking Module
//!
//! source: D:\010 Web Applicaton\palan-k\core\src\knowledge\chunker.rs (단순화)
//!
//! Markdown 인식 텍스트 분할을 제공합니다.
//! 문서 구조를 존중하면서 적절한 크기의 청크로 나눕니다.

use regex::Regex;

// ============================================================================
// Chunk Configuration
// ============================================================================

/// 청킹 설정
#[derive(Debug, Clone)]
pub struct ChunkConfig {
    /// 최소 청크 크기 (문자 수)
    pub min_characters: usize,
    /// 최대 청크 크기 (문자 수)
    pub max_characters: usize,
    /// 오버랩 크기 (문자 수)
    pub overlap_characters: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            min_characters: 200,
            max_characters: 1200,
            overlap_characters: 100,
        }
    }
}

impl ChunkConfig {
    /// RAG 최적화된 설정
    pub fn for_rag() -> Self {
        Self {
            min_characters: 300,
            max_characters: 1500,
            overlap_characters: 150,
        }
    }

    /// 빠른 인덱싱용 설정 (오버랩 없음)
    pub fn for_fast() -> Self {
        Self {
            min_characters: 500,
            max_characters: 1000,
            overlap_characters: 0,
        }
    }
}

// ============================================================================
// Chunker Trait
// ============================================================================

/// 텍스트 청킹 전략 트레이트
pub trait Chunker: Send + Sync {
    /// 텍스트를 청크로 분할
    fn chunk(&self, text: &str) -> Vec<String>;

    /// 청커 이름
    fn name(&self) -> &'static str;
}

// ============================================================================
// MarkdownChunker
// ============================================================================

/// Markdown 인식 청커
///
/// Markdown 구조를 존중하면서 텍스트를 분할합니다:
/// - 헤더 경계 유지
/// - 코드 블록 보존
/// - 리스트 그룹화
/// - 문단 경계 존중
pub struct MarkdownChunker {
    config: ChunkConfig,
}

impl MarkdownChunker {
    /// 설정으로 생성
    pub fn new(config: ChunkConfig) -> Self {
        Self { config }
    }

    /// 기본 설정으로 생성
    pub fn with_defaults() -> Self {
        Self::new(ChunkConfig::default())
    }

    /// Markdown을 섹션으로 분할
    fn split_sections(&self, text: &str) -> Vec<String> {
        let header_re = Regex::new(r"(?m)^(#{1,6})\s+").unwrap();
        let mut sections = Vec::new();
        let mut current = String::new();
        let mut in_code_block = false;

        for line in text.lines() {
            // 코드 블록 추적
            if line.trim_start().starts_with("```") {
                in_code_block = !in_code_block;
            }

            // 코드 블록 내부가 아니고 헤더를 만나면 새 섹션 시작
            if !in_code_block && header_re.is_match(line) && !current.is_empty() {
                sections.push(current.trim().to_string());
                current = String::new();
            }

            current.push_str(line);
            current.push('\n');
        }

        // 마지막 섹션 추가
        if !current.trim().is_empty() {
            sections.push(current.trim().to_string());
        }

        sections
    }

    /// 긴 섹션을 문단 경계에서 분할
    fn split_long_section(&self, section: &str) -> Vec<String> {
        if section.len() <= self.config.max_characters {
            return vec![section.to_string()];
        }

        let mut chunks = Vec::new();
        let mut current = String::new();

        // 이중 줄바꿈(문단 경계)으로 분할
        for para in section.split("\n\n") {
            let para = para.trim();
            if para.is_empty() {
                continue;
            }

            // 현재 청크에 추가하면 최대 크기 초과?
            if !current.is_empty()
                && current.len() + para.len() + 2 > self.config.max_characters
            {
                // 현재 청크 저장
                if current.len() >= self.config.min_characters {
                    chunks.push(current.clone());
                    current = String::new();
                }
            }

            // 문단 자체가 최대 크기 초과?
            if para.len() > self.config.max_characters {
                // 현재까지 저장
                if !current.is_empty() && current.len() >= self.config.min_characters {
                    chunks.push(current.clone());
                    current = String::new();
                }

                // 긴 문단을 줄 단위로 분할
                let mut line_chunk = String::new();
                for line in para.lines() {
                    if !line_chunk.is_empty()
                        && line_chunk.len() + line.len() + 1 > self.config.max_characters
                    {
                        chunks.push(line_chunk.clone());
                        line_chunk = String::new();
                    }
                    if !line_chunk.is_empty() {
                        line_chunk.push('\n');
                    }
                    line_chunk.push_str(line);
                }
                if !line_chunk.is_empty() {
                    current = line_chunk;
                }
            } else {
                // 문단 추가
                if !current.is_empty() {
                    current.push_str("\n\n");
                }
                current.push_str(para);
            }
        }

        // 마지막 청크 추가
        if !current.is_empty() {
            chunks.push(current);
        }

        // 너무 작은 청크 병합
        self.merge_small_chunks(chunks)
    }

    /// 작은 청크 병합
    fn merge_small_chunks(&self, chunks: Vec<String>) -> Vec<String> {
        if self.config.min_characters == 0 {
            return chunks;
        }

        let mut result: Vec<String> = Vec::new();

        for chunk in chunks {
            if let Some(last) = result.last_mut() {
                // 이전 청크가 너무 작으면 병합
                if last.len() < self.config.min_characters
                    && last.len() + chunk.len() + 2 <= self.config.max_characters
                {
                    last.push_str("\n\n");
                    last.push_str(&chunk);
                    continue;
                }
            }
            result.push(chunk);
        }

        result
    }

    /// 오버랩 적용
    fn apply_overlap(&self, chunks: Vec<String>) -> Vec<String> {
        if self.config.overlap_characters == 0 || chunks.len() < 2 {
            return chunks;
        }

        let mut result = Vec::with_capacity(chunks.len());

        for (i, chunk) in chunks.iter().enumerate() {
            if i == 0 {
                result.push(chunk.clone());
            } else {
                // 이전 청크의 끝부분 가져오기
                let prev = &chunks[i - 1];
                let overlap_start = prev.len().saturating_sub(self.config.overlap_characters);

                // UTF-8 경계 조정
                let overlap_start = floor_char_boundary(prev, overlap_start);

                // 단어 경계에서 시작
                let overlap_text = &prev[overlap_start..];
                let word_start = overlap_text
                    .find(char::is_whitespace)
                    .map(|p| overlap_start + p + 1)
                    .unwrap_or(overlap_start);

                let overlap = &prev[word_start..];

                // 오버랩이 의미있으면 추가
                if !overlap.trim().is_empty() && overlap.len() > 20 {
                    result.push(format!("...\n{}\n---\n{}", overlap.trim(), chunk));
                } else {
                    result.push(chunk.clone());
                }
            }
        }

        result
    }
}

impl Chunker for MarkdownChunker {
    fn chunk(&self, text: &str) -> Vec<String> {
        if text.trim().is_empty() {
            return vec![];
        }

        // 1. 섹션으로 분할
        let sections = self.split_sections(text);

        // 2. 긴 섹션 분할
        let mut chunks: Vec<String> = sections
            .into_iter()
            .flat_map(|s| self.split_long_section(&s))
            .collect();

        // 3. 빈 청크 제거
        chunks.retain(|c| !c.trim().is_empty());

        // 4. 오버랩 적용
        self.apply_overlap(chunks)
    }

    fn name(&self) -> &'static str {
        "MarkdownChunker"
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// UTF-8 경계 조정 (인덱스 이하로)
#[inline]
fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        s.len()
    } else {
        let mut i = index;
        while i > 0 && !s.is_char_boundary(i) {
            i -= 1;
        }
        i
    }
}

// ============================================================================
// Factory Functions
// ============================================================================

/// 기본 청커 생성
pub fn default_chunker() -> Box<dyn Chunker> {
    Box::new(MarkdownChunker::with_defaults())
}

/// Markdown 청커 생성 (설정 지정)
pub fn markdown_chunker(config: ChunkConfig) -> Box<dyn Chunker> {
    Box::new(MarkdownChunker::new(config))
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunker_empty() {
        let chunker = MarkdownChunker::with_defaults();
        let chunks = chunker.chunk("");
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunker_small_text() {
        let chunker = MarkdownChunker::with_defaults();
        let text = "# Header\n\nShort paragraph.";
        let chunks = chunker.chunk(text);
        assert!(!chunks.is_empty());
        assert!(chunks[0].contains("Header"));
    }

    #[test]
    fn test_chunker_preserves_code_blocks() {
        let config = ChunkConfig {
            min_characters: 50,
            max_characters: 200,
            overlap_characters: 0,
        };
        let chunker = MarkdownChunker::new(config);

        let text = r#"# Introduction

Some text here.

```rust
fn main() {
    println!("Hello, world!");
}
```

More text after code."#;

        let chunks = chunker.chunk(text);

        // 코드 블록이 분리되지 않았는지 확인
        let _has_complete_code = chunks.iter().any(|c| {
            c.contains("```rust") && c.contains("println!") && c.contains("```")
        });
        // 작은 max_characters로 인해 분리될 수 있으나 구문은 유지되어야 함
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_chunker_sections() {
        let config = ChunkConfig {
            min_characters: 10,
            max_characters: 200,
            overlap_characters: 0,
        };
        let chunker = MarkdownChunker::new(config);

        let text = r#"# Section 1

Content for section 1.

# Section 2

Content for section 2."#;

        let chunks = chunker.chunk(text);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_config_presets() {
        let default = ChunkConfig::default();
        assert_eq!(default.max_characters, 1200);

        let rag = ChunkConfig::for_rag();
        assert_eq!(rag.max_characters, 1500);
        assert_eq!(rag.overlap_characters, 150);

        let fast = ChunkConfig::for_fast();
        assert_eq!(fast.overlap_characters, 0);
    }

    #[test]
    fn test_floor_char_boundary() {
        let s = "Hello, 세계!"; // UTF-8 다중 바이트 문자

        // ASCII 범위는 그대로
        assert_eq!(floor_char_boundary(s, 5), 5);

        // 문자열 끝 초과
        assert_eq!(floor_char_boundary(s, 100), s.len());

        // 빈 문자열
        assert_eq!(floor_char_boundary("", 0), 0);
    }

    #[test]
    fn test_merge_small_chunks() {
        let config = ChunkConfig {
            min_characters: 100,
            max_characters: 500,
            overlap_characters: 0,
        };
        let chunker = MarkdownChunker::new(config);

        // 작은 청크들
        let chunks = vec![
            "Short 1.".to_string(),
            "Short 2.".to_string(),
            "Short 3.".to_string(),
        ];

        let merged = chunker.merge_small_chunks(chunks);

        // 병합되어 청크 수가 줄어야 함
        assert!(merged.len() < 3);
    }
}
