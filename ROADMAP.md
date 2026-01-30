# palank-rag Roadmap

> 로컬 하이브리드 RAG 시스템 발전 계획

---

## 현재 상태 (v0.1.0)

| 영역 | 구현 상태 | 품질 |
|------|----------|------|
| 임베딩 | Gemini API (gemini-embedding-001) | ⭐⭐⭐ |
| 벡터 검색 | LanceDB | ⭐⭐⭐ |
| 키워드 검색 | SQLite FTS5 | ⭐⭐⭐ |
| 하이브리드 | RRF (Reciprocal Rank Fusion) | ⭐⭐⭐ |
| 스크래퍼 | 기본 HTML 추출 | ⭐⭐ |
| 청킹 | 마크다운 기반 규칙 | ⭐⭐ |
| Rate Limiting | 60 RPM, 1초 딜레이, 429 백오프 | ⭐⭐⭐ |
| 중복 감지 | URL 기반 | ⭐⭐ |

---

## Phase 1: 핵심 기능 (v0.2.0)

> 실사용에 필수적인 기능

### 1.1 로컬 파일/폴더 수집
- [ ] 단일 파일 수집 (`--file`)
- [ ] 폴더 일괄 수집 (`--dir`)
- [ ] 지원 포맷: `.md`, `.txt`, `.rs`, `.ts`, `.py`, `.json`
- [ ] `.gitignore` 패턴 존중
- [ ] 증분 업데이트 (변경된 파일만)

### 1.2 설정 파일
- [ ] `~/.palank-rag/config.toml` 지원
- [ ] 설정 항목:
  ```toml
  [embedding]
  provider = "gemini"
  model = "gemini-embedding-001"
  dimension = 768

  [search]
  default_limit = 5
  hybrid_weight = 0.5  # vector vs keyword

  [storage]
  data_dir = "~/.palank-rag"
  ```

### 1.3 출력 포맷
- [ ] `--format json` 옵션
- [ ] `--format markdown` 옵션
- [ ] stdin/stdout 파이프라인 지원

### 1.4 중복 제거 강화
- [ ] 콘텐츠 해시 기반 중복 감지
- [ ] `--force` 옵션으로 덮어쓰기

---

## Phase 2: 확장 (v0.3.0)

> 추가 소스 및 검색 품질 향상

### 2.1 PDF 지원
- [ ] PDF 텍스트 추출
- [ ] 페이지별 청킹
- [ ] 메타데이터 추출 (제목, 저자)

### 2.2 컨텍스트 윈도우
- [ ] 인접 청크 포함 옵션
- [ ] `--context-before 1 --context-after 1`
- [ ] 문서 전체 반환 옵션 (`--full-doc`)

### 2.3 검색 리랭킹 (선택적)
- [ ] LLM 기반 리랭킹
- [ ] `--rerank` 플래그로 활성화
- [ ] Top-K 결과 재정렬

### 2.4 고급 필터링
- [ ] 날짜 범위 필터 (`--after`, `--before`)
- [ ] 소스 타입 필터 (`--type url|file|pdf`)
- [ ] 다중 태그 지원

---

## Phase 3: 사용성 (v0.4.0)

> 사용자 경험 개선

### 3.1 진행률 표시
- [ ] 수집 진행률 바
- [ ] 임베딩 진행률
- [ ] 예상 시간 표시

### 3.2 통계 확장
- [ ] `palank-rag stats --detailed`
- [ ] 프레임워크별 분포
- [ ] 저장 용량 표시
- [ ] 최근 검색 히스토리

### 3.3 내보내기/가져오기
- [ ] `palank-rag export --format json`
- [ ] `palank-rag import backup.json`
- [ ] 선택적 내보내기 (프레임워크별)

### 3.4 GitHub 저장소 수집
- [ ] `palank-rag ingest --github owner/repo`
- [ ] README, 문서 폴더 자동 감지
- [ ] 코드 파일 선택적 수집

---

## Phase 4: 신뢰성 (v0.5.0)

> 안정성 및 유지보수

### 4.1 스키마 마이그레이션
- [ ] 버전별 마이그레이션 스크립트
- [ ] 자동 마이그레이션 감지
- [ ] 롤백 지원

### 4.2 에러 복구
- [ ] 부분 실패 시 재개 (`--resume`)
- [ ] 손상된 인덱스 복구 (`palank-rag repair`)

### 4.3 크로스플랫폼
- [ ] macOS 빌드 및 테스트
- [ ] Linux 빌드 및 테스트
- [ ] GitHub Actions CI 설정

---

## 미래 고려사항 (v1.0+)

> 장기적으로 검토할 기능

### 고급 AI 기능
- [ ] 시맨틱 청킹 (LLM 기반 의미 단위 분할)
- [ ] 쿼리 확장 (동의어, LLM 재작성)
- [ ] 콘텐츠 자동 분류

### 통합
- [ ] 로컬 임베딩 (Ollama)
- [ ] VSCode 확장
- [ ] Obsidian 플러그인
- [ ] Notion 연동

### 확장성
- [ ] 멀티 테넌트 (프로젝트별 분리)
- [ ] 원격 저장소 지원 (S3, GCS)
- [ ] 웹 UI 대시보드

---

## 버전 릴리즈 계획

| 버전 | 목표 | 주요 기능 |
|------|------|----------|
| v0.1.0 | ✅ 초기 릴리즈 | 기본 RAG, Rate Limiting |
| v0.2.0 | 핵심 기능 | 파일/폴더 수집, 설정, JSON 출력 |
| v0.3.0 | 확장 | PDF, 컨텍스트 윈도우, 리랭킹 |
| v0.4.0 | 사용성 | 진행률, 통계, GitHub 수집 |
| v0.5.0 | 신뢰성 | 마이그레이션, 복구, 크로스플랫폼 |

---

## 변경 이력

### 2026-01-30 (v0.1.0 평가 후)
- Rate Limiting: 로드맵에서 제거 (이미 구현됨)
- 시맨틱 청킹, 쿼리 확장, 콘텐츠 분류: v1.0+로 이동
- 대화형 모드: 삭제 (CLI 반복 실행으로 충분)
- 자동 백업, 로깅 개선: 삭제 (과잉 기능)
- Phase 재구성: 핵심 → 확장 → 사용성 → 신뢰성

---

## 기여 가이드

PR 환영합니다! 특히 다음 영역:
- 크로스플랫폼 지원 (macOS, Linux 빌드)
- 추가 파일 포맷 지원
- 성능 최적화
- 문서화

---

*Last Updated: 2026-01-30*
