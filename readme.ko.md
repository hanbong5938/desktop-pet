# Desktop Pet

독립적인 네이티브 macOS 데스크톱 캐릭터입니다. 루벨리아는 터치에 반응하고 Herdr·Tern 등 등록된 제공자의 세션을 표시하며 해당 출처가 허용한 경우 답장을 보냅니다. 호스트 없이 캐릭터만 실행할 수도 있습니다.

[English](README.md) · [한국어](readme.ko.md) · [日本語](readme.ja.md) · [简体中文](readme.zh-Hans.md)

**게시 경계:** 독립 [Desktop Pet v0.4.0](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.0)(protocol 4 / capability 2 / integration API 1)과 `hanbong5938/tap/desktop-pet` 포뮬러는 공개되었습니다. 익명 다운로드 6개, 압축 파일 체크섬·서명·68개 배포 파일 closure를 검증했고, 일회용 Mac에서 실제 Brew 설치와 네이티브 UI/control 준비를 확인했습니다. 현재 소스는 v0.4.1이며 해당 공개 자산은 아직 없습니다. bridge v0.3.7 및 Herdr v0.4.0/Tern v0.2.0 연동 태그는 미공개입니다. 두 호스트의 실제 계정/제공자 런타임은 여전히 차단되어 최종 수락 표는 미완료입니다. 기존 [Herdr v0.3.6](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6)은 protocol 3 / capability 1의 과거 공개판입니다. 동결된 태그·본문·자산과 과거 증거는 변경하지 않습니다.

<img src="assets/rubelia-thumbnail.png" alt="기본 데스크톱 캐릭터 루벨리아" width="220">

## 주요 기능

- 메뉴 막대에서 제어하는 네이티브 캐릭터: 터치·쓰다듬기·이동·크기 변경, 시스템 언어 또는 한국어/영어/중국어 간체/일본어 UI.
- 관측한 로컬·원격 세션의 상태 카드와 검색·정렬; 원격 카드는 읽기 전용.
- 로컬 에이전트 카드의 한 줄 답장과 별도 말풍선 표시·크기·대사 설정.
- 공식 캐릭터 브라우저의 선택형 팩 다운로드·적용 및 로컬 팩 가져오기.
- 실행 관리·관찰 대상 설정과 출처별 명시적 앱 업데이트; 자동 업데이트 확인은 읽기 전용.
- **capability 2:** 통합 연결 설정에서 Auto 또는 등록된 제공자(Herdr·Tern 등)를 선택하고 실제 제공자 상태를 확인하며 공용 프로필에 실행 중 변경을 저장합니다. 공개 Desktop Pet v0.4.0은 이 UI를 포함하지만 과거 Herdr v0.3.6에는 없습니다. 실제 두 호스트 계정/제공자 런타임 검증 완료를 뜻하지는 않습니다.

## 요구 사항

Apple Silicon Mac, macOS 13 이상. 소스 빌드는 Rust/Cargo·Bun·Node.js/npm·Xcode Command Line Tools가 필요하며 사전 빌드 설치에는 필요 없습니다. 앱은 ad-hoc 서명·미공증입니다. Herdr 스냅샷에는 호환 로컬 서버(0.9.0 이상), 답장에는 별도 `agent.prompt`가 필요합니다. 선택형 client-attach 자동 시작에는 공식 Herdr 0.9.3에 [연동 저장소의 패치와 호스트 빌드 절차](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)를 적용하거나 실제 훅 지원 호스트를 사용해야 합니다. 순정 0.9.0/0.9.3에는 훅이 없고 플러그인 설치는 호스트를 패치하지 않습니다.

## 설치와 선택형 연동

공개된 중립 v0.4.0은 다음 Brew 명령으로 설치할 수 있습니다:

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
# 공개된 불변 v0.4.0 설치기와 고정 자산 사용:
git checkout --detach v0.4.0
bash scripts/install.sh --prebuilt
# 현재 v0.4.1 소스 대안은 별도 현재 체크아웃에서 명시적으로 실행:
# bash scripts/install.sh --source
# Replace with the installer-reported executable and your actual profile paths:
/absolute/reported/desktop-pet --config-dir /absolute/dev/config --state-dir /absolute/dev/state start
```

불변 v0.4.0 체크아웃의 기본/`--prebuilt` 설치는 공개된 고정 자산을 사용합니다. 현재 v0.4.1 체크아웃의 prebuilt는 해당 자산이 게시될 때까지 사용할 수 없습니다. 실패를 소스 빌드로 숨기지 않고 명시적 `--source`만 현재 검증된 core를 빌드합니다. standalone prebuilt는 **실제 OS 계정 home**의 `Applications/DesktopPet.app`, 승인된 source는 core의 `dist/DesktopPet.app`을 사용합니다. admission은 검증된 정상 기존 소유자를 재사용할 수 있으므로 추정 dist 경로가 아닌 설치기가 보고한 실행 파일을 사용하세요. 공유 locator는 계정 home `.local/share/desktop-pet/installation.json`에 원자 저장하며 탐색 근거일 뿐 권한이 아닙니다. 잘못된 명시 경로/locator는 fallback하지 않고 실행 파일·서명·capability·origin을 다시 검증합니다. Brew는 실제 formula/prefix를 소유하고 live/예약된 설치를 몰래 인수하지 않습니다.

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
- 앱 업데이트의 자동 확인은 읽기 전용이며 설치·적용에는 별도 동의가 필요합니다. 도우미 소유권 ACK도 완료가 아닙니다. 불명확한 결과는 [복구 절차](docs/usage.ko.md#앱-내-업데이트)와 [CLI 계약](docs/cli.md#앱-업데이트-상태와-복구)에 따라 확인하세요.

## 설정 및 문제 해결

프로필은 **명시적 `--config-dir`/`--state-dir` > `DESKTOP_PET_CONFIG_DIR`/`DESKTOP_PET_STATE_DIR` > default-profile record > 중립 XDG 기본값** 순입니다(XDG 미지정 시 `~/.config/desktop-pet`, `~/.local/state/desktop-pet`). core는 `HERDR_PLUGIN_*`를 암묵적으로 사용하지 않으며 Herdr wrapper만 host 주입을 명시 인자로 전달합니다. config에는 lifecycle/preferences·characters·menu-bar-icons, state에는 lock·전용 `control.sock`·daemon log·update 근거가 있습니다. 실제 두 경로를 맞추고 함께 백업하세요. `--herdr-socket ABS`는 등록된 Herdr 어댑터의 backend 옵션이지 제어 socket이 아니며 어댑터 없이는 core가 정규화/등록하지 않습니다. [정확한 프로필·탐색 계약](development.md#profiles-and-installation-discovery).

Core는 공통 lifecycle 저장과 `auto_start`(기본 on)를 소유하고 추가 키는 등록 연동이 소유합니다. Herdr 소유자는 **`lifecycle.json`이 없을 때만** 첫 lifecycle 쓰기의 공유 profile writer 안에서 `exit_with_herdr:true`를 원자적으로 초기화합니다. 기존 파일에서 키가 없으면 false이며 명시적 false도 유지합니다. 기존 boolean `enabled`는 false까지 `auto_start`로 이전합니다. endpoint·시작 메타데이터·다른 소유자의 필드를 보존하며 `preferences.json`이 아닌 `lifecycle.json`에 저장합니다. 잘못된 JSON/값은 초기화하지 않고 오류이며 저장 실패는 기존 실행 정책을 유지합니다. Auto/어댑터 없는 앱은 Herdr 없이 살아 있고 아래 Herdr endpoint·단절 규칙은 Herdr 전용 선택에서만 적용합니다.

## 문서 및 웹사이트

[Desktop Pet Wiki](https://github.com/hanbong5938/desktop-pet/wiki)는 core 소유의 생성된 읽기용 문서이며 원본은 검토된 소스입니다. 실제 Wiki 게시 여부는 별도 확인합니다. 이전 Herdr 사이트는 공통 앱을 복제하지 않는 연동·이전 입구입니다.

[사용 안내](docs/usage.ko.md) · [개발 안내](docs/development.ko.md) · [v0.3.6 한영 노트](docs/releases/v0.3.6.md) · [CLI 계약](docs/cli.md#한국어) · [호스트 연동 (English)](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md) · [릴리스 기록](docs/releases/README.ko.md) · [0.3 업그레이드/대응 베타](docs/releases/0.3.md#한국어--v03-계열) · [과거 베타·프리뷰](docs/migrations/unreleased.ko.md).

[한국어 웹사이트](https://hanbong5938.github.io/desktop-pet/) · [영어 웹사이트](https://hanbong5938.github.io/desktop-pet/en/) · [중국어 간체 웹사이트](https://hanbong5938.github.io/desktop-pet/zh-Hans/) · [일본어 웹사이트](https://hanbong5938.github.io/desktop-pet/ja/): 네이티브 앱 소개 페이지이며 브라우저에서 앱을 실행하지 않습니다. 별도 캐릭터 갤러리는 영어·한국어만 제공합니다. 이 중립 Pages 배포는 아직 확인하지 않았습니다. 웹사이트 개발·배포는 [개발 안내](docs/development.ko.md#웹사이트-github-pages)에 있습니다.

## 그림 및 라이선스

소프트웨어 MIT 라이선스와 그림·모델 사용 조건은 서로 다릅니다. 기본 루벨리아의 [라이선스](assets/rubelia-default/LICENSE.txt)·[출처](assets/rubelia-default/ATTRIBUTION.txt)·[제작 기록](assets/rubelia-default/source-record.json)·[배경 보정 기록](assets/rubelia-default/background-repair.json), 원본 Coding Cat 고지가 남은 [LICENSE.txt](LICENSE.txt)를 각각 확인하세요. 그림 소유자 승인은 모델 권리를 부여하지 않으며 소프트웨어 MIT가 그림을 재라이선스하지 않습니다. Qwen 연구 라이선스는 제작 기록에 보존되고 모델 가중치·엔진은 번들에 없습니다. 가져온 팩의 라이선스·출처·사용 조건도 함께 보관하세요. [그림 제작 상세](docs/development.ko.md#캐릭터-제작).
