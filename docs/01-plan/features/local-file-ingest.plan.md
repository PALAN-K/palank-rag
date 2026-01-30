# Plan: 로컬 파일/폴더 수집

> Feature: local-file-ingest
> Version: v0.2.0
> Created: 2026-01-30

---

## 1. 개요

### 1.1 목적
로컬 파일 및 폴더를 지식베이스에 수집하는 기능 추가. 현재 URL만 지원하는 한계를 극복하고 로컬 문서, 코드, 노트를 RAG 시스템에 통합.

### 1.2 배경
- 현재: `palank-rag ingest --url <URL>` 만 지원
- 문제: 로컬 마크다운, 코드 파일을 수집하려면 별도 서버 필요
- 해결: `--file`, `--dir` 옵션으로 로컬 파일 직접 수집

### 1.3 범위

**In Scope:**
- 단일 파일 수집 (`--file path/to/file.md`)
- 폴더 재귀 수집 (`--dir path/to/folder`)
- 텍스트 포맷: `.md`, `.txt`, `.rs`, `.ts`, `.py`, `.json`, `.toml`, `.yaml`
- 이미지 포맷: `.png`, `.jpg`, `.jpeg`, `.webp`, `.gif` (Gemini Vision으로 텍스트 추출)
- PDF 포맷: `.pdf` (텍스트 추출 + 페이지별 청킹)
- `.gitignore` 패턴 존중
- 증분 업데이트 (변경된 파일만)

**Out of Scope:**
- 원격 파일 시스템 (S3, GCS)
- 실시간 파일 감시 (watch mode)
- 비디오/오디오 파일

---

## 2. 요구사항

### 2.1 기능 요구사항

| ID | 요구사항 | 우선순위 |
|----|----------|----------|
| FR-01 | `--file <path>` 옵션으로 단일 파일 수집 | P0 |
| FR-02 | `--dir <path>` 옵션으로 폴더 재귀 수집 | P0 |
| FR-03 | 텍스트 파일 확장자 필터링 | P0 |
| FR-04 | 이미지 파일 Gemini Vision 텍스트 추출 | P0 |
| FR-05 | PDF 파일 텍스트 추출 및 페이지별 청킹 | P0 |
| FR-06 | `.gitignore` 패턴 존중 | P1 |
| FR-07 | 파일 경로 기반 중복 감지 | P0 |
| FR-08 | 파일 수정 시간 기반 증분 업데이트 | P1 |
| FR-09 | `--force` 옵션으로 강제 재수집 | P1 |

### 2.2 비기능 요구사항

| ID | 요구사항 | 기준 |
|----|----------|------|
| NFR-01 | 1000개 파일 수집 시 5분 이내 | Rate limit 고려 |
| NFR-02 | 메모리 사용량 500MB 이하 | 대용량 폴더 |
| NFR-03 | 에러 발생 시 부분 성공 허용 | 일부 파일 실패해도 계속 |

---

## 3. 기술 설계 (High-Level)

### 3.1 CLI 인터페이스

```bash
# 단일 파일
palank-rag ingest --file ./README.md --framework docs

# 폴더 (재귀)
palank-rag ingest --dir ./src --framework rust

# 특정 확장자만
palank-rag ingest --dir ./docs --ext md,txt

# 강제 재수집
palank-rag ingest --dir ./src --force
```

### 3.2 아키텍처

```
CLI (--file/--dir)
    │
    ▼
FileCollector (새 모듈)
    ├── 경로 검증
    ├── 확장자 필터링
    ├── .gitignore 처리
    └── 파일 목록 생성
    │
    ▼
ContentExtractor (새 모듈)
    ├── 텍스트 파일 → 직접 읽기
    ├── 이미지 파일 → Gemini Vision API (텍스트 추출)
    └── PDF 파일 → pdf-extract 크레이트 (텍스트 추출)
    │
    ▼
HybridRetriever (기존)
    ├── 중복 확인 (경로 기반)
    ├── 청킹
    ├── 임베딩
    └── 저장
```

### 3.3 이미지 텍스트 추출 (Gemini Vision)

```rust
// Gemini Vision API를 사용한 이미지 → 텍스트 변환
async fn extract_text_from_image(image_path: &Path) -> Result<String> {
    // 1. 이미지를 base64로 인코딩
    // 2. Gemini Vision API 호출 (gemini-2.0-flash-exp)
    // 3. 추출된 텍스트 반환
}
```

**프롬프트 예시:**
```
이 이미지에서 모든 텍스트를 추출해주세요.
문서, 다이어그램, 코드 등 모든 텍스트 콘텐츠를 포함해주세요.
마크다운 형식으로 구조화해주세요.
```

### 3.4 PDF 텍스트 추출

```rust
// pdf-extract 크레이트 사용
fn extract_text_from_pdf(pdf_path: &Path) -> Result<Vec<(usize, String)>> {
    // 1. PDF 파일 열기
    // 2. 페이지별로 텍스트 추출
    // 3. (페이지 번호, 텍스트) 튜플 반환
}
```

### 3.5 데이터 모델 변경

```sql
-- documents 테이블 확장
ALTER TABLE documents ADD COLUMN source_type TEXT DEFAULT 'url';
-- source_type: 'url' | 'file' | 'image' | 'pdf'

ALTER TABLE documents ADD COLUMN file_path TEXT;
-- 로컬 파일의 절대 경로

ALTER TABLE documents ADD COLUMN file_modified_at TEXT;
-- 파일 수정 시간 (ISO 8601)

ALTER TABLE documents ADD COLUMN page_number INTEGER;
-- PDF 페이지 번호 (PDF인 경우)
```

---

## 4. 구현 계획

### 4.1 단계별 구현

| 단계 | 작업 | 예상 규모 |
|------|------|----------|
| 1 | CLI 옵션 추가 (`--file`, `--dir`) | 소 |
| 2 | FileCollector 모듈 생성 | 중 |
| 3 | ContentExtractor 모듈 생성 (텍스트/이미지/PDF) | 대 |
| 4 | Gemini Vision API 연동 (이미지 → 텍스트) | 중 |
| 5 | PDF 텍스트 추출 연동 | 중 |
| 6 | DB 스키마 확장 (source_type, file_path 등) | 소 |
| 7 | HybridRetriever 연동 | 중 |
| 8 | .gitignore 지원 | 소 |
| 9 | 증분 업데이트 로직 | 중 |
| 10 | 테스트 및 문서화 | 중 |

### 4.2 파일 변경 예상

| 파일 | 변경 유형 | 설명 |
|------|----------|------|
| `src/cli/mod.rs` | 수정 | --file, --dir 옵션 추가 |
| `src/collector/mod.rs` | 신규 | FileCollector 구현 |
| `src/extractor/mod.rs` | 신규 | ContentExtractor 구현 |
| `src/extractor/image.rs` | 신규 | Gemini Vision 이미지 추출 |
| `src/extractor/pdf.rs` | 신규 | PDF 텍스트 추출 |
| `src/knowledge/store.rs` | 수정 | 스키마 확장 |
| `src/knowledge/hybrid.rs` | 수정 | 파일 수집 메서드 추가 |
| `src/lib.rs` | 수정 | 새 모듈 export |
| `Cargo.toml` | 수정 | ignore, pdf-extract, base64 크레이트 추가 |

---

## 5. 리스크 및 대응

| 리스크 | 영향 | 대응 |
|--------|------|------|
| 대용량 폴더 메모리 부족 | 높음 | 스트리밍 처리, 배치 크기 제한 |
| Rate limit 초과 (이미지 다수) | 높음 | 기존 Rate limiter 활용, 이미지당 1초 딜레이 |
| Gemini Vision API 비용 | 중간 | 이미지 파일 개수 경고, --skip-images 옵션 |
| PDF 텍스트 추출 품질 | 중간 | 스캔된 PDF는 이미지로 처리 |
| 파일 인코딩 문제 | 낮음 | UTF-8 가정, 에러 시 스킵 |

---

## 6. 성공 기준

- [ ] `--file` 옵션으로 단일 파일 수집 가능
- [ ] `--dir` 옵션으로 폴더 재귀 수집 가능
- [ ] 텍스트 파일 (.md, .txt, .rs 등) 수집 가능
- [ ] 이미지 파일 Gemini Vision 텍스트 추출 가능
- [ ] PDF 파일 텍스트 추출 및 페이지별 청킹 가능
- [ ] 지원 확장자 자동 필터링
- [ ] 중복 파일 감지 및 스킵
- [ ] 기존 테스트 통과
- [ ] 새 기능 테스트 추가

---

*Plan Author: Claude*
*Review Status: Pending*
