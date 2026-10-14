# Desktop Pet

독립적인 네이티브 macOS 데스크톱 캐릭터입니다. 루벨리아는 터치에 반응하고 Herdr·Tern 등 등록된 제공자의 세션을 표시하며 해당 출처가 허용한 경우 답장을 보냅니다. 호스트 없이 캐릭터만 실행할 수도 있습니다.

[English](readme.md) · [한국어](readme.ko.md) · [日本語](readme.ja.md) · [简体中文](readme.zh-Hans.md)

**현재 공개 Core:** [Desktop Pet v0.4.3](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.3), release `412334201`, 게시 `2026-10-14T04:20:48Z`, 불변 소스 `9bba2e71a1d5a1bcef5f71509ac6701b61b3d674`. 익명 공개 자산 6개·strict/deep 서명·일치 protocol 4 / capability 2 / API 1·배포 파일 68개 해시·정확한 일반 파일 closure를 검증했습니다. archive SHA-256 `b1bdf3b118876f5b705617b30dc321dd9cb1ff71cff147bcd19314e072ea746a`, installer SHA-256 `7d5094f89a7433759ac102f5028217f057d3bbbfa70a0a6f686bbc5a7559f576`. [CI 38510441950](https://github.com/hanbong5938/desktop-pet/actions/runs/38510441950)의 release job `115677463431`은 성공했고 이후 Wiki job만 실패했습니다. 실제 37페이지 렌더링은 통과했지만 전용 `WIKI_PUBLISH_TOKEN`이 없어 게시되지 않았습니다. 광범위 소유자 토큰을 재사용하지 않습니다. 중립 v0.4.3 포뮬러는 tap `6a9ae670cb3c69eb6e66bfd99ef351c513168f72`에 공개됐습니다. 일회용 VM의 적격 Brew v0.4.3 새 설치가 통과했고 실제 네이티브 GUI는 Auto / Herdr Waiting 0 / Tern Waiting 0, PID `2967`, instance `d62441eca0603028793fa846aa0555cd`, 파생 image `7b3613635d8cb5e6c26e2b4f4259aa4eeb87bf8eb17c21c7219236e92631c4ae`를 보였습니다. 명시적 hide는 동일한 별도 기본 프로필을 유지했습니다. [Bridge v0.3.8](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.8)은 공개됐습니다: release `412343376`, 게시 `2026-10-14T04:40:30Z`, 정확한 일반 UTF-8 Git 파일 10개의 metadata-only 소스 `e615ed310202d99d4389fb80d7cfc29db87275c6`, retirement ref `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`. 익명 공개 자산 5개는 검증한 CI 바이트와 일치합니다. archive SHA-256 `fc1d69dab9e9287dcb9903bc262b1b2e0ba06690c1b944230b81439b4ff09003`, rig SHA-256 `54ea2e40171407e243726697e6a56eff226244a32f33318ab5d45fd2c8fdda9b`; 일치 protocol 3 / observation 1 / API 1·서명·68개 closure 해시를 검증했습니다. CI `38511259163` / job `115680064263`에서 실제 배포 ingress의 native Rubelia 검증이 `TMPDIR=/tmp`로 통과했으며 두 ingress scratch 디렉터리는 `/private/tmp`입니다. old stable 포뮬러 `49fa8a2`는 v0.3.8이고 beta 포뮬러·동결 v0.3.6 자산/태그는 그대로입니다. 새 실제 managed v0.3.6 → 공개 v0.3.8 A는 완료됐습니다(`16b6307413ec409efe3eb5a04c9cc21f`, `completed` / `manager_exited`, installed/applied true). 원본 서명 v0.3.6 helper로 확인했고 bridge PID `3563` / instance `9aa35111b6b8e54dfca787b01e3425d7`는 정확한 image `be4858803ceec46a9d3ac80cefb2c7d2da0013f1e2e9df49ca99fca01ae675a8`로 동일한 기존 private config/state에서 Herdr mode / Herdr 1 / Tern 0, UI/control ready에 도달했습니다. 공개 managed A의 `/tmp` 수정 입증이지 보존된 실패 VM의 recovery가 아닙니다. Core v0.4.3 managed B·명시적 failed-retirement resume·실제 v0.3.8의 고정 v0.3.6 beta 경로·중립 v0.4.2 → v0.4.3 업데이트는 대기 중입니다. 공개 Herdr v0.4.1의 호환 Core v0.4.2 pin은 유지하며 current-main Core v0.4.3 소비자 작업은 새 Herdr 릴리스가 아닙니다. **과거 v0.4.2 / v0.3.7 근거(v0.4.3 실행 증거 아님):** **게시 경계:** [Desktop Pet v0.4.2](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.2)은 공개되었습니다(release `412241744`, `2026-10-14T01:09:50Z`, 불변 소스 `cb10ce2c3cc65402cbb914d60301f7627d5546d5`). 익명 자산 6개, strict/deep 서명, protocol 4 / capability 2 / integration API 1, 68개 배포 리소스 해시·정확한 closure를 검증했습니다. tap `32919c745864b54a2e716fcc5a3714d0d38d3e84`의 중립 포뮬러와 실제 공개 v0.4.1 → v0.4.2 Brew 업그레이드는 receipt/derivation·서명 5개·`brew test`·Auto 네이티브 UI/control ready(Herdr 0 / Tern 0)를 입증했습니다. 새 공개 standalone v0.4.1 설치 및 동시 설치기 2개의 실행 중 소유자 재사용을 확인했으며, 네이티브 Check가 실제 공개 v0.4.2를 제시하고 동의 후 `standalone_replace`·`manager_exited`·`installed:true`·`applied:true`, 같은 config/state·정확한 공개 v0.4.2 이미지의 준비 완료를 확인했습니다. Check는 locator/profile/lifecycle/locks/PID를 유지했지만 `preferences.json` 확인 캐시는 바꿨으므로 모든 설정 바이트 불변은 아닙니다. [수정 integration-only Herdr v0.4.1](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.4.1)도 공개되었습니다(release `412252863`, 불변 ref `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`). 공개 wrapper 2개가 숨김을 유지하며 같은 Brew v0.4.2 PID/instance/image를 재사용했고, 명시적 config/state와 실제 private patched Herdr 0.9.3 socket으로 Auto Herdr 1 / Tern 0에 도달했습니다. 새 환경의 공개 bootstrap 2개는 OS-home 앱 하나를 설치/재사용했으며 연동-local native 파일이나 데몬 시작은 없었습니다. 이는 공개 Herdr manager 설치 receipt 증거가 아닙니다. 원래 Herdr v0.4.0의 생성 앱·실패 테스트 scaffold symlink 소스 결함은 보존하며 integration-only였다고 주장하지 않습니다. [Bridge v0.3.7](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.7)도 공개되었습니다(release `412267469`, 불변 tag `987a4b859f95e922c1c5443ead372d10d0dddb24`). 정확한 Core `cb10ce2`·수정 Herdr `6d2ac04`에서 빌드했고 익명 자산 5개·서명·68개 파일 closure를 검증했습니다. tap `657f32cc4cc563ae40ae0432ac5123dcb3f58426`에서 old stable 포뮬러만 v0.3.7로 변경했으며 beta는 그대로입니다. manual-origin private 실제 호스트 실행은 Herdr 1 / Tern 0 ready를 입증했지만 managed A→B 증거는 아닙니다. 실제 동결 공개 Brew v0.3.6 → v0.3.7 A `6a10663c2d2f0177990623a969238af4`와 별도 동의 Core v0.4.2 B `c66961f9e5a91c0023545065ae61385b`는 실제 Brew retirement(exit 0, quiescent)까지 완료했습니다. B 전후 모든 설정 파일 해시와 이전 A 증거 87개 파일은 불변이며 실제 Core v0.4.2 UI는 Desktop Pet / Herdr Waiting 0 / Tern Disabled 0입니다. 반면 실제 managed v0.3.7 A `a1aec5902020de7bcaaf583b1a5f50a5`는 native `/tmp` 프로필 검증에 실패했습니다. checkout 교체·registry 등록 전 manager exit 1이 발생했고 동결 helper는 Unknown / `manager_exited`, installed/applied false를 보존합니다. VM·journal·reservation은 recovery/replay 없이 유지합니다. 전체 matrix 완료는 아닙니다. 명시적 definite-failed-retirement resume·실제 SDK 양쪽 시작 순서의 같은 인스턴스/재연결·계정 수락은 미입증입니다. Core v0.4.3·bridge v0.3.8은 공개됐고 Brew v0.4.3 새 설치·새 managed A는 완료됐지만 managed B/resume 수락은 대기 중입니다. [Core v0.4.3 / bridge v0.3.8](https://github.com/hanbong5938/desktop-pet/blob/main/docs/releases/v0.4.3.md). Tern v0.2.0·호환 Core v0.4.1 pin은 그대로이며 실제 Tern keychain/provider 로그인과 OMP providerSignIn/NoModel 해결이 필요합니다. 과거 태그·본문·자산·소스는 보존합니다. Core Pages `cb10ce2` 배포와 EN/KO 브라우저 확인은 성공했지만 JA/ZH 시각 확인·최종 현재 문서 배포는 대기 중입니다. Linux Wiki catalog 렌더는 통과했지만 전용 `WIKI_PUBLISH_TOKEN`·소유자의 Wiki Home 초기화가 필요합니다. Wiki 링크는 공개 증거가 아니며 바이너리 release job 성공과 뒤이은 Wiki 실패는 별개입니다.

**공개 managed B의 알려진 제한·수정 대기:** 실제 managed 0.3.6 → 공개 bridge 0.3.8 A는 완료됐고(`16b6307413ec409efe3eb5a04c9cc21f`), 원래 0.3.6 helper의 서명과 completed status 검증도 통과했습니다. 명시적 Check로 A의 이전 terminal 관측을 정리해 Move가 활성화됐지만 B preflight는 `managed source changed; migration never resets user changes`로 거부됐습니다. 실제 Git status는 `dist/HerdrDesktopPet.app` 아래 생성 파일 정확히 68개만 untracked였으며 다른 변경은 없었습니다. B 동의·Stop 이전이므로 B manager·fault flag는 없었고 원본 PID 3563은 변경 없이 실행 중입니다. 공개 Core 0.4.3 / bridge 0.3.8은 여전히 현재 버전입니다. 승인된 새 Core 0.4.4 + 실제 bridge 0.3.9는 대기 중입니다. producer 수정은 `.gitignore`에 `/dist/HerdrDesktopPet.app/`만 선언하고 실제 materialized Git admission gate로 생성 앱은 clean, tracked script 수정·무관한 root 파일·`dist/user-owned-note`는 모두 dirty임을 확인하도록 합니다. 네이티브 Git dirty 보호는 유지합니다. 공개 0.3.8을 통과시키려고 `git clean`·reset·수동 ignore를 사용하지 마세요. 원래 0.3.8 태그는 `.gitignore` 없는 일반 UTF-8 Git 파일 정확히 10개이며 새 수정 metadata는 11개가 됩니다. 새 게시·managed B/resume·최종 4개 언어 Pages 완료를 주장하지 않습니다. 과거 0.3.7 Unknown/no-replay와 SDK/Wiki 차단은 그대로입니다.


<img src="assets/rubelia-thumbnail.png" alt="기본 데스크톱 캐릭터 루벨리아" width="220">

## 주요 기능

- 메뉴 막대에서 제어하는 네이티브 캐릭터: 터치·쓰다듬기·이동·크기 변경, 시스템 언어 또는 한국어/영어/중국어 간체/일본어 UI.
- 관측한 로컬·원격 세션의 상태 카드와 검색·정렬; 원격 카드는 읽기 전용.
- 로컬 에이전트 카드의 한 줄 답장과 별도 말풍선 표시·크기·대사 설정.
- 공식 캐릭터 브라우저의 선택형 팩 다운로드·적용 및 로컬 팩 가져오기.
- 실행 관리·관찰 대상 설정과 출처별 명시적 앱 업데이트; 자동 업데이트 확인은 설치·실행을 바꾸지 않지만 성공 확인 timestamp 캐시는 쓸 수 있습니다.
- **capability 2:** 통합 연결 설정에서 Auto 또는 등록된 제공자(Herdr·Tern 등)를 선택하고 실제 제공자 상태를 확인하며 공용 프로필에 실행 중 변경을 저장합니다. 현재 공개 Desktop Pet v0.4.3은 이 UI를 포함하지만 과거 Herdr v0.3.6에는 없습니다. 실제 두 호스트 계정/제공자 런타임 검증 완료를 뜻하지는 않습니다.

## 요구 사항

Apple Silicon Mac, macOS 13 이상. 소스 빌드는 Rust/Cargo·Bun·Node.js/npm·Xcode Command Line Tools가 필요하며 사전 빌드 설치에는 필요 없습니다. 앱은 ad-hoc 서명·미공증입니다. Herdr 스냅샷에는 호환 로컬 서버(0.9.0 이상), 답장에는 별도 `agent.prompt`가 필요합니다. 선택형 client-attach 자동 시작에는 공식 Herdr 0.9.3에 [연동 저장소의 패치와 호스트 빌드 절차](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)를 적용하거나 실제 훅 지원 호스트를 사용해야 합니다. 순정 0.9.0/0.9.3에는 훅이 없고 플러그인 설치는 호스트를 패치하지 않습니다.

## 설치와 선택형 연동

중립 v0.4.3 포뮬러는 tap `6a9ae670cb3c69eb6e66bfd99ef351c513168f72`에 게시되었으며 실제 v0.4.3 Brew 새 설치/실행은 대기 중입니다. 설치 명령:

```sh
brew install hanbong5938/tap/desktop-pet
desktop-pet --version
desktop-pet start
desktop-pet status
```

사전 빌드·소스 설치는 Herdr 연동이 아니라 **core** 체크아웃에서 실행합니다. 기본 설치는 prebuilt-only이며 다운로드·검증 실패를 소스 빌드로 숨기지 않습니다. `--source`는 현재 core 체크아웃만 빌드하고 그 `dist/DesktopPet.app`을 사용합니다.

```sh
git clone https://github.com/hanbong5938/desktop-pet.git
cd desktop-pet
# 공개된 불변 v0.4.3 설치기와 고정 자산 사용:
git checkout --detach v0.4.3
bash scripts/install.sh --prebuilt
# Explicit source installation uses a separate checkout pinned to public v0.4.3.
# Replace with the installer-reported executable and your actual profile paths:
/absolute/reported/desktop-pet --config-dir /absolute/dev/config --state-dir /absolute/dev/state start
```

**v0.4.3 소스 설치**는 실제 공개 태그에 고정한 별도 체크아웃에서 명시적으로 실행합니다:

```sh
git clone https://github.com/hanbong5938/desktop-pet.git desktop-pet-source
cd desktop-pet-source
git checkout --detach v0.4.3
bash scripts/install.sh --source
```

불변 v0.4.3의 기본/`--prebuilt`는 정확한 공개 고정 자산을 사용합니다(과거 v0.4.0/v0.4.1 체크아웃은 각 pin 유지). 실패 시 source fallback은 없고 명시적 `--source`는 실제 공개 v0.4.3 태그를 포함해 선택한 core 체크아웃을 빌드합니다. standalone prebuilt는 **실제 OS 계정 home**의 `Applications/DesktopPet.app`, 승인된 source는 core의 `dist/DesktopPet.app`을 사용합니다. admission은 검증된 정상 기존 소유자를 재사용할 수 있으므로 설치기가 보고한 실행 파일을 사용하세요. 공유 locator는 계정 home `.local/share/desktop-pet/installation.json`에 원자 저장하며 탐색 근거일 뿐 권한이 아닙니다. 잘못된 명시 경로/locator는 fallback하지 않고 실행 파일·서명·capability·origin을 다시 검증합니다. Brew는 실제 formula/prefix를 소유하고 live/예약된 설치를 몰래 인수하지 않습니다.

필요한 연동만 설치하세요. Herdr는 얇은 consumer이며 Tern은 네이티브 앱을 다운로드·빌드·설치·자동 시작하지 않습니다. 플러그인 갱신·제거는 연동만 바꾸고 공용 앱·프로필·캐릭터는 남깁니다. 일반 앱 업데이트는 Herdr 플러그인을 재설치하지 않습니다.

```sh
herdr plugin install hanbong5938/herdr-desktop-pet
tern plugin install https://github.com/hanbong5938/tern-desktop-pet
```

## 프로필·연결·이전

`--host auto|PROVIDER`는 Auto 또는 등록된 불투명 제공자 선택이며 고정 Herdr/Tern 열거가 아닙니다. 관찰 선택 우선순위는 CLI > `DESKTOP_PET_OBSERVATION_MODE` > 저장 `observation.host` > 새 Auto입니다. 이전 `both`→`auto`, host-less→Herdr는 읽기 이전에만 적용합니다. 실행 중 선택은 Connection/`preferences set --observation-host auto|PROVIDER`로 저장 후 적용합니다. `--herdr-socket ABS`는 등록 Herdr 어댑터의 backend 옵션이며 core control socket이 아니고 어댑터 없이는 등록하지 않습니다.

새 프로필은 Auto입니다. Connection은 등록된 제공자의 실제 연결을 표시하며 설치 여부로 추측하지 않습니다. 제공자 ID는 불투명 값이고 작업 권한은 출처 이름 접두사·경로가 아니라 capability로 결정합니다. Auto는 동시 관찰이지 fallback이 아닙니다. 원격은 읽기 전용이며 Tern은 답장만 지원하고 worktree 제거는 지원하지 않습니다. 접수는 에이전트 완료가 아닙니다.

[사용법](docs/usage.ko.md), [정확한 프로필·탐색 계약](docs/development.md#profiles-and-installation-discovery), [A→B 이전 안내](docs/migrations/desktop-pet.md)를 확인하세요. status 또는 Connection의 실제 실행 파일/config/state 세 값을 Tern Configure에 입력하고 Connect하세요. 기존 별도 Tern·beta·custom 프로필은 그 자리에서 보존하고 병합하지 않습니다.

```sh
desktop-pet profile show --json
desktop-pet start
# Only with the registered Herdr adapter; use its verified backend:
# desktop-pet --herdr-socket /actual/path/to/herdr-api.sock start
desktop-pet preferences set --observation-host auto
```

## 처음 사용하기

| 조작 | 동작 |
| --- | --- |
| 메뉴 막대 아이콘 좌클릭 / 우클릭·Control-click | 설정 열기 / 설정·종료·필요 시 **캐릭터 표시·조작 복구** 메뉴. |
| 머리·몸통 탭, 머리 위 쓰다듬기; 캐릭터 드래그·오른쪽 아래 조절점 | 반응; 이동·크기 변경. |
| 말풍선 변·모서리 드래그; 카드 선택 | 말풍선 크기 변경; 로컬 에이전트 카드 아래 답장칸 열기. |
| 답장칸 Enter·Command+Enter·보내기 / Escape | IME 조합 중에는 제출하지 않고 Escape는 먼저 조합을 취소합니다. 조합 중이 아닐 때 Enter·Command+Enter·보내기는 메시지를 제출하고 Escape는 답장칸을 접습니다. 검색창에 포커스가 있으면 Enter·Command+Enter는 답장을 제출하지 않습니다. |

캐릭터와 말풍선은 각각 숨길 수 있습니다. 둘 다 숨기거나 **전체 창 클릭 통과**를 켰다면 메뉴 막대 아이콘을 우클릭·Control-click하고 **캐릭터 표시·조작 복구**를 선택하세요. **투명 영역 클릭 통과**는 캐릭터의 투명 부분에만 적용됩니다. [조작·복구 상세](docs/usage.ko.md#조작) · [독립 표시](docs/usage.ko.md#독립-표시와-메뉴-막대-복구).

## 선택 캐릭터와 의상

기본 루벨리아 외 팩은 [캐릭터 갤러리](https://hanbong5938.github.io/herdr-characters/)에서 살펴보세요(처음 영어로 열리면 **KO** 버튼). **캐릭터 → 캐릭터 브라우저 열기…**에서 지원되는 공식 팩의 **다운로드 및 적용**을 명시적으로 선택합니다. 로컬 폴더나 `.herdrchar`를 **캐릭터 추가…**로 가져오기만 해서는 캐릭터가 바뀌지 않습니다. 설치된 카드를 **선택**한 뒤 **캐릭터 적용**을 누르세요. 작업 결과가 불확실하면 작업 ID로 상태를 확인하고 무작정 반복하지 마세요. [팩 사용법](docs/usage.ko.md#캐릭터-팩) · [팩 CLI](docs/cli.md#팩-명령-및-기존-경계) · [제작](docs/development.ko.md#캐릭터-제작).

## 안전 및 제한

- 답장은 선택 출처의 **로컬 prompt capability**가 있어야 합니다. Herdr는 `agent.prompt`, Tern은 일회 `agents:ask`를 쓰고 원격·SSH·셸·PTY 입력으로 대체하지 않습니다. 접수 ACK는 **모델 완료가 아니며** Tern은 queued일 수 있습니다. 승인/질문은 실제 터미널에서 처리하고 불명확한 전달은 확인 없이 다시 보내지 마세요. Herdr 제출에는 현재 occupant 경합이 있어 세션의 원자 고정을 보장하지 않습니다. [전달 상세](docs/usage.ko.md#인라인-답장과-메시지-전달).
- **워크트리 삭제…**는 확인한 카드 하나만이 아닌 **작업 공간 전체**의 탭·패널과 실행 중 프로세스를 닫고, 연결 체크아웃의 **무시된 파일까지** 되돌릴 수 없이 삭제합니다. 확인 창의 **정확한 체크아웃 경로**와 전체 영향을 먼저 검토하세요. 불확실한 결과를 반복 요청하지 마세요. [삭제 조건·확인](docs/usage.ko.md#워크트리-삭제).
- 앱 업데이트의 자동 확인은 lifecycle·설치·이미지·선택 프로필을 유지하지만 `preferences.json` 성공 확인 timestamp 캐시를 쓸 수 있으며 설치·적용에는 별도 동의가 필요합니다. 도우미 소유권 ACK도 완료가 아닙니다. 불명확한 결과는 [복구 절차](docs/usage.ko.md#앱-내-업데이트)와 [CLI 계약](docs/cli.md#앱-업데이트-상태와-복구)에 따라 확인하세요.

## 설정 및 문제 해결

프로필은 **명시적 `--config-dir`/`--state-dir` > `DESKTOP_PET_CONFIG_DIR`/`DESKTOP_PET_STATE_DIR` > default-profile record > 중립 XDG 기본값** 순입니다(XDG 미지정 시 `~/.config/desktop-pet`, `~/.local/state/desktop-pet`). core는 `HERDR_PLUGIN_*`를 암묵적으로 사용하지 않으며 Herdr wrapper만 host 주입을 명시 인자로 전달합니다. config에는 lifecycle/preferences·characters·menu-bar-icons, state에는 lock·전용 `control.sock`·daemon log·update 근거가 있습니다. 실제 두 경로를 맞추고 함께 백업하세요. `--herdr-socket ABS`는 등록된 Herdr 어댑터의 backend 옵션이지 제어 socket이 아니며 어댑터 없이는 core가 정규화/등록하지 않습니다. [정확한 프로필·탐색 계약](development.md#profiles-and-installation-discovery).

Core는 공통 lifecycle 저장과 `auto_start`(기본 on)를 소유하고 추가 키는 등록 연동이 소유합니다. Herdr 소유자는 **`lifecycle.json`이 없을 때만** 첫 lifecycle 쓰기의 공유 profile writer 안에서 `exit_with_herdr:true`를 원자적으로 초기화합니다. 기존 파일에서 키가 없으면 false이며 명시적 false도 유지합니다. 기존 boolean `enabled`는 false까지 `auto_start`로 이전합니다. endpoint·시작 메타데이터·다른 소유자의 필드를 보존하며 `preferences.json`이 아닌 `lifecycle.json`에 저장합니다. 잘못된 JSON/값은 초기화하지 않고 오류이며 저장 실패는 기존 실행 정책을 유지합니다. Auto/어댑터 없는 앱은 Herdr 없이 살아 있고 아래 Herdr endpoint·단절 규칙은 Herdr 전용 선택에서만 적용합니다.

## 문서 및 웹사이트

[Desktop Pet Wiki](https://github.com/hanbong5938/desktop-pet/wiki)는 core 소유의 생성된 읽기용 문서이며 원본은 검토된 소스입니다. 실제 Wiki 게시 여부는 별도 확인합니다. 이전 Herdr 사이트는 공통 앱을 복제하지 않는 연동·이전 입구입니다.

[사용 안내](docs/usage.ko.md) · [개발 안내](docs/development.ko.md) · [v0.3.6 한영 노트](docs/releases/v0.3.6.md) · [CLI 계약](docs/cli.md#한국어) · [호스트 연동 (English)](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md) · [릴리스 기록](docs/releases/README.ko.md) · [0.3 업그레이드/대응 베타](docs/releases/0.3.md#한국어--v03-계열) · [과거 베타·프리뷰](docs/migrations/unreleased.ko.md).

[한국어 웹사이트](https://hanbong5938.github.io/desktop-pet/) · [영어 웹사이트](https://hanbong5938.github.io/desktop-pet/en/) · [중국어 간체 웹사이트](https://hanbong5938.github.io/desktop-pet/zh-Hans/) · [일본어 웹사이트](https://hanbong5938.github.io/desktop-pet/ja/): 네이티브 앱 소개 페이지이며 브라우저에서 앱을 실행하지 않습니다. 별도 캐릭터 갤러리는 영어·한국어만 제공합니다. 이 중립 Pages 배포는 아직 확인하지 않았습니다. 웹사이트 개발·배포는 [개발 안내](docs/development.ko.md#웹사이트-github-pages)에 있습니다.

## 그림 및 라이선스

소프트웨어 MIT 라이선스와 그림·모델 사용 조건은 서로 다릅니다. 기본 루벨리아의 [라이선스](assets/rubelia-default/LICENSE.txt)·[출처](assets/rubelia-default/ATTRIBUTION.txt)·[제작 기록](assets/rubelia-default/source-record.json)·[배경 보정 기록](assets/rubelia-default/background-repair.json), 원본 Coding Cat 고지가 남은 [LICENSE.txt](LICENSE.txt)를 각각 확인하세요. 그림 소유자 승인은 모델 권리를 부여하지 않으며 소프트웨어 MIT가 그림을 재라이선스하지 않습니다. Qwen 연구 라이선스는 제작 기록에 보존되고 모델 가중치·엔진은 번들에 없습니다. 가져온 팩의 라이선스·출처·사용 조건도 함께 보관하세요. [그림 제작 상세](docs/development.ko.md#캐릭터-제작).
