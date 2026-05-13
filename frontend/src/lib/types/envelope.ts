// WS binary envelope 타입 정의.
//
// 정본:
// - `docs/ssot/wire-protocol.md` §2 (32 슬롯 표)
// - `docs/adr/0002-transport-websocket.md` D2/D3/D4 (envelope 구조)
// - `docs/reports/0008-frontend-stack.md` §F4 (frontend dispatcher 골격)
//
// 본 모듈은 *타입 alias만* 노출한다. 인코딩/디코딩 함수는 `$lib/ws/decode.ts`,
// 런타임 `Envelope` interface 와 `FRAME_TYPE` 상수 객체도 `decode.ts` 가 origin —
// 본 모듈은 같은 심볼을 *type-only* 로 re-export 해서 외부 모듈이 type-only import
// 경로(`import type { Envelope } from '$lib/types/envelope'`)로 합의된 채널을 쓸
// 수 있게 한다. (verbatimModuleSyntax + isolatedModules 둘 다 ON 이므로 type-only
// re-export 는 `export type { ... }` 문법으로 명시.)

export type {
  Envelope,
  FrameTypeCode,
  PaneOutPayload,
  LayoutChangedPayload,
  MChangedPayload,
  IChangedPayload,
  ViewportChangedPayload,
  FocusModeChangedPayload,
  NotifyMirrorPayload,
} from '$lib/ws/decode';

export { FRAME_TYPE } from '$lib/ws/decode';
