// gestureShield — store-driven canvas gesture shield (ADR-0059 D7).
//
// 문제: iframe(web_view / Html / Pdf viewer) 내부 이벤트는 부모 document 에
// 도달하지 않으므로, iframe 위에서 시작·통과하는 캔버스 제스처(hand tool 팬,
// space-hold 팬, lasso, 노드 드래그, 노드 리사이즈)가 죽는다. 기존
// `.drag-isolated` 는 노드 *자체* 드래그 한정이라 커버 범위가 좁다.
//
// 결정(ADR-0059 D7): 제스처 진행 중 모든 embedded iframe 에 `pointer-events:
// none` 을 걸어 이벤트가 iframe 을 통과해 캔버스로 도달하게 한다. wheel 은
// 대상 아님 — iframe 위 wheel = 페이지 스크롤(의도된 동작). 제스처 종료 시 해제.
//
// 소비처: WebViewNode 의 body iframe + HtmlViewer / PdfViewer.
// 설정처: Canvas.svelte 가 각 제스처의 start/end 지점에서 flag 를 set/clear.
//
// 각 소스는 독립 boolean — 어느 하나라도 true 면 `active`. 개별 flag 라
// 제스처가 겹쳐도(예: space-hold 중 노드 드래그) 한 쪽 종료가 다른 쪽 shield 를
// 조기 해제하지 않는다(카운터의 짝맞춤 누수 위험 회피).

class GestureShieldStore {
  /** Hand tool is the active canvas tool (pan mode). */
  handTool = $state(false);
  /** Space bar held for the momentary pan modifier. */
  spaceHold = $state(false);
  /** A rubber-band lasso selection is in progress. */
  lasso = $state(false);
  /** An existing node is being dragged (moved). */
  nodeDrag = $state(false);
  /** A drag-to-create gesture (rect/ellipse/line/free-draw) is in progress. */
  createDrag = $state(false);
  /** A node is being resized (NodeResizer handle drag). */
  nodeResize = $state(false);

  /** Any gesture active → iframes must ignore pointer events. */
  get active(): boolean {
    return (
      this.handTool ||
      this.spaceHold ||
      this.lasso ||
      this.nodeDrag ||
      this.createDrag ||
      this.nodeResize
    );
  }
}

export const gestureShield = new GestureShieldStore();
