---
description: 로컬 RAG 시스템 - 문서 수집, 검색, 관리
---

# /rag - 로컬 하이브리드 RAG

로컬 파일 기반 RAG 시스템입니다. 서버 없이 LanceDB + FTS5로 하이브리드 검색을 제공합니다.

## 기능

- **문서 수집**: URL 또는 텍스트를 벡터 DB에 저장
- **하이브리드 검색**: 벡터 유사도 + 키워드 검색 결합
- **로컬 저장**: 모든 데이터가 로컬에 저장됨

## 사용법

### 문서 추가
```bash
palank-rag ingest "https://docs.example.com/guide"
palank-rag ingest --text "직접 입력한 텍스트"
```

### 검색
```bash
palank-rag query "Next.js App Router 사용법"
palank-rag query "React hooks" --limit 5
```

### 문서 목록
```bash
palank-rag list
palank-rag list --framework nextjs
```

### 문서 삭제
```bash
palank-rag delete --url "https://docs.example.com/guide"
```

## 설치

### 1. CLI 다운로드
[GitHub Releases](https://github.com/PALAN-K/palank-rag/releases)에서 OS에 맞는 바이너리 다운로드

### 2. PATH에 추가
```bash
# Windows
move palank-rag.exe %USERPROFILE%\.local\bin\

# macOS/Linux
mv palank-rag ~/.local/bin/
chmod +x ~/.local/bin/palank-rag
```

### 3. API 키 설정
```bash
export GEMINI_API_KEY="your-api-key"
# 또는
export GOOGLE_AI_API_KEY="your-api-key"
```

## 요구사항

- **임베딩 API**: Gemini API 키 (무료 티어 사용 가능)
- **저장소**: 로컬 파일 시스템 (~/.palank-rag/)

## 특징

| 기능 | 설명 |
|------|------|
| 벡터 검색 | LanceDB (로컬 파일 기반) |
| 키워드 검색 | SQLite FTS5 |
| 하이브리드 | RRF (Reciprocal Rank Fusion) |
| 임베딩 | Gemini text-embedding-004 |
| 청킹 | Markdown 구조 인식 |
