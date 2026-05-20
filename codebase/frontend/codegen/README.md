# codegen — utoipa → openapi-typescript 파이프라인

ADR-0011 D5 (`utoipa` 5.x) + ADR-0012 D7 (`openapi-typescript`) 통일 도구체인.

## 흐름

1. Rust 백엔드의 `bin/gen-openapi` 가 `utoipa` 의 OpenAPI 3.1 산출을 `codebase/shared/openapi.yaml` 로 출력.
2. 본 디렉터리의 `run.sh` (C3 task 산출) 이 `openapi-typescript` 을 실행해 `src/lib/types/api.d.ts` 로 변환.
3. 프런트엔드 코드는 `openapi-fetch` 로 type-safe 호출.

## 실행

```bash
npm run codegen
```

본 C2 스켈레톤 단계에서 `run.sh` 는 미작성 — C3 (devops-architect 담당) 에서 추가.
파이프라인 작동 전까지 `src/lib/types/canvas-layout.d.ts` 는 빈 placeholder 유지.
