# 에이전트 Activity 연동

워크스페이스의 **Settings → Appearance → Terminal activity**를 켭니다. 데스크톱 관리자의 **Agent activity**에서 에이전트를 선택하고 **Generate integration**을 누릅니다. 결과를 복사하고 검토한 뒤 기존 에이전트 설정과 병합합니다. 설정 생성은 기존 파일을 수정하거나 도구를 승인하지 않습니다. hook 설치 후 에이전트를 다시 시작합니다.

관리자는 AppImage에서도 사용할 수 있도록 자체 데이터 폴더의 고정 경로에 hook 실행 파일을 보관합니다. gtmux 업데이트 후 연동을 다시 생성하면 실행 파일도 갱신됩니다. 관리자 데이터 폴더를 삭제했다면 연동을 다시 생성합니다. gtmux 터미널 밖에서는 hook이 상태를 전송하지 않습니다.

## 지원 신호

| 에이전트 | 작업 중 | 완료 | 입력 필요 | 설정 위치 |
|---|---|---|---|---|
| Claude Code | 프롬프트·도구 hook | Stop | PermissionRequest·권한 알림 | `~/.claude/settings.json`에 hook 병합 |
| Codex | 프롬프트·도구 hook | Stop | PermissionRequest | `~/.codex/hooks.json` 병합 후 `/hooks`에서 검토·신뢰 |
| Gemini CLI | BeforeAgent·도구 hook | AfterAgent | ToolPermission 알림 | `~/.gemini/settings.json`에 hook 병합 |
| Copilot CLI | 프롬프트·도구 hook | agentStop | 이 어댑터에서 구분하지 않음 | `.github/hooks/gtmux-activity.json` |
| Cursor | 프롬프트·도구 hook | completed 상태의 stop | 이 어댑터에서 구분하지 않음 | `~/.cursor/hooks.json` 병합 |
| Aider | 일반 터미널 Activity | 완료 알림 | 별도 구분하지 않음 | Aider YAML에 생성한 키 병합 또는 알림 명령 옵션 |
| OpenCode | session.status | session.idle | 권한·질문 이벤트 | 생성한 JS를 `.opencode/plugins/gtmux-activity.js`로 저장 |

Windows의 `~`는 사용자 프로필입니다. 에이전트와 같은 실행 환경의 gtmux 빌드를 사용합니다. WSL에만 설치한 에이전트는 Windows 네이티브 실행 파일이 아닙니다. 지원 이벤트는 에이전트 버전에 따라 다릅니다. Copilot 설정은 실행 파일·인자 형식의 hook을 사용하며 Cursor CLI의 이벤트는 데스크톱보다 적을 수 있습니다.

프로토콜에서 구분되는 하위 에이전트 완료는 제외합니다. Stop hook은 응답 경계에 도달했음을 나타내며 다른 hook이 후속 실행을 요구할 수 있습니다. 취소·오류를 성공적인 완료로 보고하지 않습니다. hook 입력에서 이벤트 메타데이터를 읽으며 대화 기록 파일을 열거나 프롬프트·API 키를 저장·전송하지 않습니다. 지원되지 않는 이벤트는 알 수 없음으로 남기며 일반 출력 추정은 estimated로 구분합니다.

## CLI와 임베드

`gtmux agent hooks claude`로 설정을 출력합니다. `claude` 대신 `codex`, `gemini`, `copilot`, `cursor`, `aider`, `opencode`를 사용할 수 있습니다. 기존 hook 파일을 덮어쓰지 않습니다. 생성한 명령은 stdin의 JSON을 `gtmux agent event`로 처리합니다. 다른 호스트도 gtmux 터미널에서 `gtmux terminal report working`, `completed`, `needs_input`, `unknown`을 전달할 수 있습니다.

연동을 끄려면 gtmux hook 항목이나 OpenCode 플러그인만 제거합니다. 복원하려면 설정을 다시 생성합니다. Activity UI는 별도로 끌 수 있습니다. 탭 제목의 unread 개수는 기본 off이며 Appearance에서 다시 켤 수 있습니다.

## 근거와 검증 범위

공식 [Claude hook](https://code.claude.com/docs/en/hooks), [Codex hook](https://learn.chatgpt.com/docs/hooks), [Gemini hook](https://geminicli.com/docs/hooks/reference/), [Copilot hook](https://docs.github.com/en/copilot/reference/hooks-reference), [Cursor hook](https://cursor.com/docs/hooks), [Aider 알림](https://aider.chat/docs/usage/notifications.html), [OpenCode 플러그인](https://opencode.ai/docs/plugins/) 명세를 따릅니다. 프로토콜 입력과 실제 gtmux CLI·서버 전달 경로를 검사합니다. 모든 에이전트 버전에서 유료 모델 대화까지 실행했다는 뜻은 아닙니다.

## 관리 화면에서 설치와 복원

**Review installation**에서 실제 변경 경로를 확인하고 **Apply integration change**를 누릅니다. 기존 설정을 백업하고 권한·다른 hook을 보존하면서 추가합니다. 에이전트를 다시 시작하고 필요한 hook 신뢰 확인은 에이전트에서 직접 수행하세요. 관리자가 대신 신뢰를 승인하지 않습니다. **Review removal**은 설치 이후 파일이 그대로인 경우 원본을 복원합니다. 이후 변경된 파일은 gtmux 항목만 수동으로 제거하세요. 잘못된 JSON, 지원하지 않는 주석 문법, 기존 Aider 알림 명령, 플러그인 충돌·동시 편집은 덮어쓰지 않고 오류를 표시합니다. `CODEX_HOME` 등 사용자 지정 설정 경로는 생성된 설정을 수동으로 병합하세요.
