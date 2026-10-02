# 기여 방법

## 개발 환경

`codebase/backend/rust-toolchain.toml`에 지정한 Rust와 Node.js 22 이상을 사용합니다. 프런트엔드·launcher 폴더에서 각각 `npm ci`로 의존성을 설치합니다. `docs/`의 개인 조사 메모는 커밋하지 않습니다.

```sh
cd codebase/backend
cargo test --workspace --locked
cargo build -p gtmux-cli --locked
cd ../frontend
npm ci
npm run check
npm test
npm run build
cd ../launcher
npm ci
npm test
```

## 로컬 데스크톱 빌드 준비

저장소 루트에서 backend 실행 파일과 빌드한 frontend를 launcher resources에 복사합니다.

```sh
node scripts/prepare-launcher.mjs codebase/backend/target/debug/gtmux codebase/frontend/dist
cd codebase/launcher
npm start
```

Electron 없이 브라우저 관리 화면만 사용하려면 `npm run web -- --data-dir /absolute/temporary/manager-data`를 실행합니다. 관리 URL을 출력하고 사용자가 Start를 누르기 전에는 서버를 시작하지 않습니다. 테스트는 별도 데이터 폴더·미사용 포트를 사용하고 실제 사용자 Store나 설정을 재사용하지 않습니다.

## 하나의 기여, 구분된 커밋

하나의 통합 브랜치에서 기능별 커밋을 유지합니다. `push/cloud-mode`를 포함한 upstream 변경을 확인한 뒤 의도적으로 rebase·merge합니다. 미병합 브랜치가 존재한다고 기능이 예약된 것으로 가정하지 않습니다. 겹치는 부분을 설명하고 독립적으로 유용한 upstream 동작을 유지합니다.

## 문서 검사

영어를 원문으로 사용하며 같은 이름의 한국어 페이지를 둡니다. CI는 코드 변경에 문서 갱신이 포함되고 영어 페이지 변경에 한국어 수정이 함께 있는지 확인합니다. 갱신 여부를 검사하는 것이며 번역의 정확성을 판정하지는 않습니다. 설정 예제는 참고 문서의 생성 입력으로 유지하며 설명문 자동 번역을 가장하지 않습니다.

```sh
cd website
npm ci
npm run check
npm run build
```

문서 workflow는 main에서 자체 정적 사이트를 빌드합니다. Maintainer가 Pages 소스를 GitHub Actions로 선택하고 저장소 변수 `GTMUX_PUBLISH_DOCS=true`를 설정하면 산출물 전용 `gh-pages` 브랜치 갱신과 Pages artifact 배포도 수행합니다. PR에서는 검증만 수행합니다. Release workflow는 서버 압축 파일과 앱 패키지를 만들며 서명 비밀 값은 maintainer가 제공하고 저장소에 커밋하지 않습니다.

## 패키징한 앱 회귀 검사

`npm run pack` 후 `GTMUX_TEST_APP`을 압축 해제된 실행 파일의 절대 경로로 지정하고 `codebase/launcher`에서 `npm run test:electron`을 실행합니다. 임시 프로필과 미사용 포트로 네이티브 서버 및 격리된 작업 창을 열고 서버를 정지한 뒤 작업 창이 남아 있는 상태에서 관리자 창을 닫습니다. 앱 프로세스의 정상 종료까지 검사합니다. 임시 데스크톱 창이 열리며 평소 사용하는 앱 프로필을 지정하면 안 됩니다.
