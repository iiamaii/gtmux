# Password mode cloud handover

작성일: 2026-05-21

## 배경

`codebase/QUICKSTART.ko.md` 는 외부 접속 cloud mode 의 기본 인증 경로를
password mode 로 안내하고 있었지만, 현재 codebase 에서는 password hash 를 설정한
뒤 서버를 시작하면 부팅 중 패닉이 발생한다. 따라서 Quickstart 는 우선 검증된
token mode 로 전환했고, password mode 는 BE/FE 작업을 분리해 후속 정리한다.

## 재현 절차

```bash
mkdir -p /private/tmp/gtmux-cloud-check/config/gtmux \
  /private/tmp/gtmux-cloud-check/state \
  /private/tmp/gtmux-cloud-check/data

cp codebase/config.sample.toml /private/tmp/gtmux-cloud-check/cloud.config.toml
perl -0pi -e 's/PUBLIC_IP/127.0.0.1/g; s/9001/19091/g; s/mode\s+=\s+"token"/mode                = "password"/' \
  /private/tmp/gtmux-cloud-check/cloud.config.toml

printf 'Password1\nPassword1\n' | \
  XDG_CONFIG_HOME=/private/tmp/gtmux-cloud-check/config \
  XDG_STATE_HOME=/private/tmp/gtmux-cloud-check/state \
  XDG_DATA_HOME=/private/tmp/gtmux-cloud-check/data \
  codebase/backend/target/release/gtmux set-password

XDG_CONFIG_HOME=/private/tmp/gtmux-cloud-check/config \
XDG_STATE_HOME=/private/tmp/gtmux-cloud-check/state \
XDG_DATA_HOME=/private/tmp/gtmux-cloud-check/data \
GTMUX_FRONTEND_DIST="$PWD/codebase/frontend/dist" \
codebase/backend/target/release/gtmux start \
  --session cloud-check \
  --config /private/tmp/gtmux-cloud-check/cloud.config.toml
```

현재 결과:

```text
thread 'main' panicked at crates/http-api/src/lib.rs:251:29:
Cannot block the current thread from within a runtime.
```

token mode 로 바꾸면 같은 cloud 설정에서 `0.0.0.0:<port>` 바인딩은 성공한다.

## BE handover

### 증상

`gtmux start` 의 async runtime 안에서 `build_app_state()` 가 password hash 를
주입할 때 `AppState::with_password_hash()` 를 호출한다. 이 함수는
`tokio::sync::RwLock::blocking_write()` 를 사용하고 있어 runtime thread 안에서
패닉이 난다.

관련 위치:

- `codebase/backend/bin/gtmux-cli/src/main.rs` 의 `build_app_state()`
- `codebase/backend/crates/http-api/src/lib.rs` 의 `AppState::with_password_hash()`

### 수정 방향

1. `AppState::with_password_hash()` 를 async 함수로 바꾸고 `write().await` 를
   사용하거나, builder 단계에서 `Arc<RwLock<Option<String>>>` 초기값을 직접
   구성해 blocking write 를 제거한다.
2. `with_password_hash_path()` 와 함께 start path 에서 password mode, token mode
   모두 같은 방식으로 state 가 초기화되는지 확인한다.
3. password hash 파일이 없을 때 현재처럼 경고 후 기동할지, password mode 에서는
   fail-fast 할지 정책을 명확히 한다. 외부 접속 운영 경로라면 fail-fast 가 더
   예측 가능하다.
4. regression test 를 추가한다.
   - password hash 가 있는 상태에서 `build_app_state()` 또는 router 구성 경로가
     runtime 안에서 패닉 없이 완료되어야 한다.
   - password mode 에서 `/auth/login` 이 정상 credential 에 대해 cookie 를
     발급해야 한다.
   - 잘못된 password 반복 시 rate limit 이 유지되어야 한다.

### 추가 확인 사항

cloud mode 배너는 `bind = 0.0.0.0` 일 때 `Open URL` 의 host 를 `127.0.0.1` 로
표시한다. local mode 에서는 편리하지만 cloud mode Quickstart 에서는 외부
IP/도메인으로 치환해야 하므로, BE 쪽에서 cloud mode 배너 문구를 분기하는 것이
좋다.

token mode 도 완전한 first-run 경로는 아니다. cloud mode 에서 token 파일이 아직
없고 `${XDG_STATE_HOME:-~/.local/state}/gtmux` 디렉터리도 없으면 `load_token()` 이
`AuthError::NotFound` 가 아니라 IO error 로 실패해 새 token 발행 경로에 들어가지
못한다. Quickstart 는 우선 `mkdir -p ~/.local/state/gtmux && chmod 700 ...` 로
우회했지만, BE 에서는 cloud first-run 시 token parent directory 를 생성한 뒤
load/issue 분기로 들어가도록 정리하는 편이 맞다.

## FE handover

### 현재 차단점

BE 부팅 패닉 때문에 실제 password mode `/auth` 화면의 end-to-end 검증을 아직
완료할 수 없다. BE 수정 후 FE 는 아래 항목을 확인해야 한다.

### 검증 항목

1. unauthenticated browser 가 `http://PUBLIC_IP:9001/` 로 접근하면 auth 화면으로
   이동하거나 auth 상태가 필요한 화면을 안정적으로 막아야 한다.
2. password login 성공 시 HttpOnly cookie 발급 후 canvas 로 진입해야 한다.
3. login 실패, lockout, password hash 미설정 같은 BE 응답을 FE 가 사용자가
   이해 가능한 상태로 보여야 한다.
4. token bootstrap 으로 이미 cookie 가 있는 사용자가 password mode 로 전환된
   서버에 접근할 때 stale auth 상태가 깨끗하게 정리되어야 한다.
5. `GTMUX_FRONTEND_DIST` 또는 TOML `frontend_dist` 없이 서버를 띄우면 `/` 는
   `{"error":"not_found"}` 가 된다. FE 검증 시에는 반드시 dist 경로를 지정한다.

## 완료 기준

- password mode cloud 설정에서 `gtmux start` 가 패닉 없이 부팅한다.
- `GET /healthz` 는 200, `GET /` 는 frontend dist 지정 시 200 HTML 이다.
- password login 성공/실패/lockout 흐름이 브라우저에서 확인된다.
- Quickstart 를 password mode 로 되돌릴지, token mode 와 password mode 를 병렬로
  안내할지 결정할 수 있는 검증 결과가 남아 있다.
