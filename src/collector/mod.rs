//! 파일 수집 모듈
//!
//! 로컬 파일 및 폴더를 수집하여 지식베이스에 추가합니다.
//! .gitignore 패턴을 존중하고, 지원하는 확장자만 수집합니다.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use ignore::WalkBuilder;

// ============================================================================
// File Types
// ============================================================================

/// 지원하는 파일 타입
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    /// 텍스트 파일 (마크다운, 코드 등)
    Text,
    /// 이미지 파일 (Gemini Vision으로 처리)
    Image,
    /// PDF 파일
    Pdf,
}

impl FileType {
    /// 확장자로 파일 타입 결정
    pub fn from_extension(ext: &str) -> Option<Self> {
        let ext = ext.to_lowercase();
        match ext.as_str() {
            // 텍스트 파일
            "md" | "txt" | "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "json" | "toml" | "yaml"
            | "yml" | "html" | "css" | "scss" | "go" | "java" | "c" | "cpp" | "h" | "hpp"
            | "sh" | "bash" | "zsh" | "sql" | "xml" | "csv" => Some(FileType::Text),

            // 이미지 파일
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => Some(FileType::Image),

            // PDF 파일
            "pdf" => Some(FileType::Pdf),

            _ => None,
        }
    }

    /// 파일 경로에서 타입 결정
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
    }
}

// ============================================================================
// Collected File
// ============================================================================

/// 수집된 파일 정보
#[derive(Debug, Clone)]
pub struct CollectedFile {
    /// 파일 절대 경로
    pub path: PathBuf,
    /// 파일 타입
    pub file_type: FileType,
    /// 파일 크기 (바이트)
    pub size: u64,
    /// 수정 시간
    pub modified_at: Option<SystemTime>,
}

impl CollectedFile {
    /// 파일에서 CollectedFile 생성
    pub fn from_path(path: PathBuf) -> Result<Option<Self>> {
        // 파일 타입 확인
        let file_type = match FileType::from_path(&path) {
            Some(ft) => ft,
            None => return Ok(None), // 지원하지 않는 확장자
        };

        // 메타데이터 읽기
        let metadata = std::fs::metadata(&path)
            .with_context(|| format!("Failed to read metadata: {:?}", path))?;

        if !metadata.is_file() {
            return Ok(None);
        }

        Ok(Some(Self {
            path,
            file_type,
            size: metadata.len(),
            modified_at: metadata.modified().ok(),
        }))
    }
}

// ============================================================================
// File Collector
// ============================================================================

/// 파일 수집기 설정
#[derive(Debug, Clone)]
pub struct CollectorConfig {
    /// .gitignore 패턴 존중 여부
    pub respect_gitignore: bool,
    /// 숨김 파일 포함 여부
    pub include_hidden: bool,
    /// 최대 파일 크기 (바이트, 0이면 제한 없음)
    pub max_file_size: u64,
    /// 특정 확장자만 수집 (비어있으면 모든 지원 확장자)
    pub extensions: Vec<String>,
    /// 이미지 파일 건너뛰기
    pub skip_images: bool,
    /// PDF 파일 건너뛰기
    pub skip_pdfs: bool,
}

impl Default for CollectorConfig {
    fn default() -> Self {
        Self {
            respect_gitignore: true,
            include_hidden: false,
            max_file_size: 10 * 1024 * 1024, // 10MB
            extensions: vec![],
            skip_images: false,
            skip_pdfs: false,
        }
    }
}

/// 파일 수집기
pub struct FileCollector {
    config: CollectorConfig,
}

impl FileCollector {
    /// 새 수집기 생성
    pub fn new(config: CollectorConfig) -> Self {
        Self { config }
    }

    /// 기본 설정으로 수집기 생성
    pub fn with_defaults() -> Self {
        Self::new(CollectorConfig::default())
    }

    /// 단일 파일 수집
    pub fn collect_file(&self, path: &Path) -> Result<Option<CollectedFile>> {
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };

        if !abs_path.exists() {
            anyhow::bail!("File not found: {:?}", abs_path);
        }

        if !abs_path.is_file() {
            anyhow::bail!("Not a file: {:?}", abs_path);
        }

        let file = CollectedFile::from_path(abs_path)?;

        // 필터 적용
        if let Some(ref file) = file {
            if !self.should_include(file) {
                return Ok(None);
            }
        }

        Ok(file)
    }

    /// 폴더 재귀 수집
    pub fn collect_directory(&self, path: &Path) -> Result<Vec<CollectedFile>> {
        let abs_path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };

        if !abs_path.exists() {
            anyhow::bail!("Directory not found: {:?}", abs_path);
        }

        if !abs_path.is_dir() {
            anyhow::bail!("Not a directory: {:?}", abs_path);
        }

        let mut files = Vec::new();

        // ignore 크레이트로 .gitignore 지원
        let walker = WalkBuilder::new(&abs_path)
            .hidden(!self.config.include_hidden)
            .git_ignore(self.config.respect_gitignore)
            .git_global(self.config.respect_gitignore)
            .git_exclude(self.config.respect_gitignore)
            .build();

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!("Failed to read entry: {}", e);
                    continue;
                }
            };

            // 파일만 처리
            if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                continue;
            }

            let file_path = entry.path().to_path_buf();

            match CollectedFile::from_path(file_path) {
                Ok(Some(file)) => {
                    if self.should_include(&file) {
                        files.push(file);
                    }
                }
                Ok(None) => {} // 지원하지 않는 확장자
                Err(e) => {
                    tracing::warn!("Failed to collect file: {}", e);
                }
            }
        }

        tracing::info!("Collected {} files from {:?}", files.len(), abs_path);
        Ok(files)
    }

    /// 파일이 필터 조건을 만족하는지 확인
    fn should_include(&self, file: &CollectedFile) -> bool {
        // 파일 크기 제한
        if self.config.max_file_size > 0 && file.size > self.config.max_file_size {
            tracing::debug!("Skipping large file: {:?} ({} bytes)", file.path, file.size);
            return false;
        }

        // 이미지 건너뛰기
        if self.config.skip_images && file.file_type == FileType::Image {
            return false;
        }

        // PDF 건너뛰기
        if self.config.skip_pdfs && file.file_type == FileType::Pdf {
            return false;
        }

        // 특정 확장자만 수집
        if !self.config.extensions.is_empty() {
            if let Some(ext) = file.path.extension().and_then(|e| e.to_str()) {
                if !self
                    .config
                    .extensions
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case(ext))
                {
                    return false;
                }
            } else {
                return false;
            }
        }

        true
    }
}

// ============================================================================
// Statistics
// ============================================================================

/// 수집 통계
#[derive(Debug, Default)]
pub struct CollectionStats {
    pub total_files: usize,
    pub text_files: usize,
    pub image_files: usize,
    pub pdf_files: usize,
    pub total_size: u64,
}

impl CollectionStats {
    /// 수집된 파일 목록에서 통계 계산
    pub fn from_files(files: &[CollectedFile]) -> Self {
        let mut stats = Self::default();

        for file in files {
            stats.total_files += 1;
            stats.total_size += file.size;

            match file.file_type {
                FileType::Text => stats.text_files += 1,
                FileType::Image => stats.image_files += 1,
                FileType::Pdf => stats.pdf_files += 1,
            }
        }

        stats
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_type_from_extension() {
        assert_eq!(FileType::from_extension("md"), Some(FileType::Text));
        assert_eq!(FileType::from_extension("rs"), Some(FileType::Text));
        assert_eq!(FileType::from_extension("png"), Some(FileType::Image));
        assert_eq!(FileType::from_extension("PDF"), Some(FileType::Pdf));
        assert_eq!(FileType::from_extension("exe"), None);
    }

    #[test]
    fn test_collector_config_default() {
        let config = CollectorConfig::default();
        assert!(config.respect_gitignore);
        assert!(!config.include_hidden);
        assert_eq!(config.max_file_size, 10 * 1024 * 1024);
    }
}
