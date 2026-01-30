# palank-rag

로컬 하이브리드 RAG 시스템 - Claude Code 플러그인

## 특징

- **서버리스**: Pinecone/Weaviate 없이 로컬 파일만으로 동작
- **하이브리드 검색**: 벡터 유사도 + 키워드 검색 결합
- **무료 임베딩**: Gemini API 무료 티어 사용
- **제로 설정**: API 키 하나만 설정하면 바로 사용

## 설치

### Claude Code 플러그인으로 설치 (권장)

```bash
/plugin marketplace add PALAN-K/marketplace
/plugin install palank-rag@palank
```

### CLI만 설치

[Releases](https://github.com/PALAN-K/palank-rag/releases)에서 다운로드

## 사용법

```bash
# 문서 수집
palank-rag ingest "https://nextjs.org/docs"

# 검색
palank-rag query "서버 컴포넌트 사용법"

# 목록
palank-rag list
```

## 아키텍처

```
+---------------------------------------------+
|              palank-rag                     |
+---------------------------------------------+
|  Hybrid Retriever (RRF)                     |
|    +-- Vector Search (LanceDB)              |
|    +-- Keyword Search (FTS5)                |
+---------------------------------------------+
|  Embedding (Gemini text-embedding-004)      |
+---------------------------------------------+
|  Storage (~/.palank-rag/)                   |
|    +-- vectors.lance                        |
|    +-- knowledge.db                         |
+---------------------------------------------+
```

## 라이선스

MIT License
