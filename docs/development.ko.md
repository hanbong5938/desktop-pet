# Desktop Pet

독립적인 네이티브 macOS 데스크톱 캐릭터입니다. 루벨리아는 터치에 반응하고 Herdr·Tern 등 등록된 제공자의 세션을 표시하며 해당 출처가 허용한 경우 답장을 보냅니다. 호스트 없이 캐릭터만 실행할 수도 있습니다.

[English](../README.md) · [한국어](../readme.ko.md) · [日本語](../readme.ja.md) · [简体中文](../readme.zh-Hans.md)

**게시 경계:** 독립 [Desktop Pet v0.4.0](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.0)(protocol 4 / capability 2 / integration API 1)과 `hanbong5938/tap/desktop-pet` 포뮬러는 공개되었습니다. 익명 다운로드 6개, 압축 파일 체크섬·서명·68개 배포 파일 closure를 검증했고, 일회용 Mac에서 실제 Brew 설치와 네이티브 UI/control 준비를 확인했습니다. 현재 소스는 v0.4.1이며 해당 공개 자산은 아직 없습니다. bridge v0.3.7 및 Herdr v0.4.0/Tern v0.2.0 연동 태그는 미공개입니다. 두 호스트의 실제 계정/제공자 런타임은 여전히 차단되어 최종 수락 표는 미완료입니다. 기존 [Herdr v0.3.6](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6)은 protocol 3 / capability 1의 과거 공개판입니다. 동결된 태그·본문·자산과 과거 증거는 변경하지 않습니다.

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
# Published immutable v0.4.0 installer and pinned assets:
git checkout --detach v0.4.0
bash scripts/install.sh --prebuilt
# Explicit current v0.4.1 source alternative in a separate current checkout:
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

새 프로필은 Auto입니다. Connection은 등록된 제공자의 실제 연결을 표시하며 설치 여부로 추측하지 않습니다. 제공자 ID는 불투명 값이고 작업 권한은 출처 이름 접두사·경로가 아니라 capability로 결정합니다. Auto는 동시 관찰이지 fallback이 아닙니다. 원격은 읽기 전용이며 Tern은 답장만 지원하고 worktree 제거는 지원하지 않습니다. 접수는 에이전트 완료가 아닙니다.

[사용법](usage.ko.md), [정확한 프로필·탐색 계약](development.md#profiles-and-installation-discovery), [A→B 이전 안내](migrations/desktop-pet.md)를 확인하세요. status 또는 Connection의 실제 실행 파일/config/state 세 값을 Tern Configure에 입력하고 Connect하세요. 기존 별도 Tern·beta·custom 프로필은 그 자리에서 보존하고 병합하지 않습니다.

```sh
desktop-pet profile show --json
desktop-pet --herdr-socket /actual/path/to/herdr-api.sock start
desktop-pet preferences set --observation-host auto
```

## Profiles and installation discovery

중립 제품은 `DesktopPet.app`(`io.github.hanbong5938.desktop-pet`), CLI `desktop-pet`, helper `desktop-pet-update-coordinator`, release authority `hanbong5938/desktop-pet`, formula `hanbong5938/tap/desktop-pet`을 소유합니다. non-Brew prebuilt는 실제 OS 계정 home의 `Applications/DesktopPet.app`, 승인된 source 설치는 정확한 core의 `dist/DesktopPet.app`입니다. 영구 old-name alias는 정상 탐색 경로가 아닙니다.

프로필 우선순위는 **명시적 `--config-dir`/`--state-dir` > `DESKTOP_PET_CONFIG_DIR`/`DESKTOP_PET_STATE_DIR` > default-profile record > 중립 XDG 기본값**입니다. 기본값은 `${XDG_CONFIG_HOME:-$HOME/.config}/desktop-pet`, `${XDG_STATE_HOME:-$HOME/.local/state}/desktop-pet`입니다. 기본 config root의 `default-profile.json`은 `{ "version":1, "config_dir":"/absolute/config", "state_dir":"/absolute/state" }`로 두 실제 경로를 원자 기록하며 재귀 profile pointer는 금지합니다. `desktop-pet profile show --json`은 daemon/디렉터리를 만들지 않는 읽기 전용입니다.

이전은 기존 두 디렉터리를 **그 자리에서 채택**합니다. preferences/lifecycle/characters/menu-bar-icons와 lock/control socket, 구 `updates/` journal/helper를 이동·병합·삭제하지 않습니다. neutral default가 아직 미선택일 때만 legacy base pair를 기록합니다. 다른 pair를 가리키는 기존 default는 유지하고 이전 profile은 명시 경로로 사용하세요. 별도 Tern/beta/custom profile은 분리 상태를 유지합니다. default를 의도적으로 선택하려면:

```sh
desktop-pet profile adopt --config-dir /absolute/config --state-dir /absolute/state
```

adopt는 pair 선택이지 데이터 복사가 아닙니다. active update/예약, 다른 live daemon, invalid/symlink/nonowned 경로는 변경 전에 실패합니다. 실제 config/state **둘 다** 백업하고 start/status/command/stop에 같은 pair를 사용하세요. core는 `HERDR_PLUGIN_*`를 암묵적 app 기본값으로 해석하지 않습니다. Herdr wrapper만 host 주입 우선순위를 명시 native 인자로 바꾸고 Tern은 검증된 pair를 명시 전달하며 일치하는 중립 환경값을 설정합니다. status/Connection은 실제 실행 파일과 선택 profile을 표시합니다.

공유 locator는 **실제 OS 계정 home**의 `.local/share/desktop-pet/installation.json`이며 `{ "version":1, "executable":"/absolute/path/to/desktop-pet", "kind":"homebrew" }`입니다. kind는 `homebrew|standalone|source`만 허용하고 같은 디렉터리의 install lock 아래 원자 저장합니다. 탐색 순서는 **사용자 명시 절대 실행 파일 → 유효 locator → PATH `desktop-pet` → 실제 `brew --prefix desktop-pet` 실행 파일 → `~/Applications/DesktopPet.app`**입니다. invalid 명시 경로/locator는 fallback하지 않습니다. locator는 권한이 아니며 소유·private regular file, 실제 executable/bundle/signature, capability와 origin을 매번 검증합니다. 실행 이미지/profile 불일치는 몰래 재사용하지 않고 표시합니다. 서명 없는 script·사용자 plugin·버전 문자열은 권한을 얻지 못합니다. 업데이트/migration의 locator 변경은 예약 아래 같은 계약을 따릅니다.

새 관찰은 Auto이며 `--host auto|PROVIDER`는 Auto 또는 등록된 불투명 provider를 선택하는 옵션이지 app 변형/고정 열거가 아닙니다. 우선순위는 명시 CLI > `DESKTOP_PET_OBSERVATION_MODE` > 저장 `observation.host` > 새 profile Auto입니다. 구 auto/herdr/tern 선택은 유지하고 `both`→`auto`, host-less→Herdr는 legacy 읽기 이전에만 적용합니다. 새 변경 값으로 `both`를 받지 않습니다. 실제 adapter source 수가 provider 연결 증거이며 control은 private same-UID profile socket입니다. locality/capability만 작업 권한을 결정하고 이름 접두사/경로로 권한을 얻지 못합니다. 공통 설정과 adapter 소유 lifecycle/preferences 키는 같은 atomic profile writer를 사용하고 `exit_with_herdr`/endpoints는 Herdr가 소유합니다.

`herdr plugin install`, `tern plugin install URL --force`는 연동만 갱신합니다. Herdr uninstall/unlink, Tern remove/unlink는 공유 앱/profile/characters를 보존합니다. 일반 native update는 host plugin을 재설치하지 않습니다. Tern Connect/Configure는 검증·저장 후 명시적으로 시작한 pet에 연결할 뿐 다운로드·빌드·설치·자동 시작하지 않습니다. 기존 legacy executable을 검증된 neutral executable과 **기존 config/state pair**로 바꾸는 제안은 가능하지만 성공한 사용자 Configure 저장만 triple을 변경합니다. 취소/실패는 이전 연결을 유지하고 제시/불명확한 prompt를 다시 보내지 않습니다.

Core는 공통 lifecycle 저장과 `auto_start`(기본 on)를 소유하고 추가 키는 등록 연동이 소유합니다. Herdr 소유자는 **`lifecycle.json`이 없을 때만** 첫 lifecycle 쓰기의 공유 profile writer 안에서 `exit_with_herdr:true`를 원자적으로 초기화합니다. 기존 파일에서 키가 없으면 false이며 명시적 false도 유지합니다. 기존 boolean `enabled`는 false까지 `auto_start`로 이전합니다. endpoint·시작 메타데이터·다른 소유자의 필드를 보존하며 `preferences.json`이 아닌 `lifecycle.json`에 저장합니다. 잘못된 JSON/값은 초기화하지 않고 오류이며 저장 실패는 기존 실행 정책을 유지합니다. Auto/어댑터 없는 앱은 Herdr 없이 살아 있고 아래 Herdr endpoint·단절 규칙은 Herdr 전용 선택에서만 적용합니다.

`settings set auto_start on|off`는 정확한 `on|off`만 받으며 추가 lifecycle 키는 등록된 연동 소유자가 제공합니다. `--herdr-socket ABS`는 등록 Herdr 어댑터의 backend 선택이지 core control socket이 아닙니다.

## 네이티브 빌드와 서명

소스 설치 후 네이티브 검사는 다음 명령으로 실행합니다.

```sh
bun run check
bun run test:native
```

네이티브 hit-overlay 혼합 계산은 릴리스 러너의 Xcode 16.4 Swift 컴파일러와 호환되도록 타입을 지정한 부분 식을 사용합니다. 오버레이를 고칠 때 이 타입 경계를 유지하세요. 소스 빌드에는 여전히 Xcode Command Line Tools가 필요합니다.

debug·release 네이티브 빌드는 `codesign`을 필요로 하며 [`native/rig/RigDecodeWorker.entitlements.plist`](../native/rig/RigDecodeWorker.entitlements.plist)의 `com.apple.security.cs.allow-jit=true`를 **`rig-decode-worker`에만** 부여해 ad-hoc 서명합니다. Cargo는 plist 변경을 추적해 worker를 다시 빌드·서명합니다. JavaScriptCore가 신뢰된 디코더 번들을 JIT 컴파일할 수 있게 하지만 앱 본체와 네이티브 rig 라이브러리에는 **JIT 권한이 없으며** worker 격리·검증·자원 제한은 유지합니다. `bun run package:native`는 선택한 서명 identity로 worker 전용 권한을 다시 적용하고 외부 앱 서명 후 실제 entitlement 및 worker·앱의 엄격한 서명을 검증합니다. 서명·검증 실패 시 **패키징을 중단**합니다. 기본 ad-hoc 패키지는 로컬 실행용이며 Developer ID 서명·공증이 아닙니다.

Rust/Swift rig 경계는 타입 있는 asset/token을 넘기기 **전에** ABI 버전과 C 구조체 일곱 개의 크기를 확인합니다. 초기 motion은 네이티브 호스트가 복사하는 봉인된 바이트이며 옛 경로 조회 방식으로 돌아가지 않습니다. Metal은 같은 인코더의 색상 그리기 전에 실제 흰자 구멍을 L/R/무방향 stencil로 준비하고 CPU 의미론적 hit·참조 래스터도 같은 소유권 규칙을 적용합니다. 흰자가 없는 rig의 기존 홍채 표시 동작은 유지합니다. `RIG_PROBE_FAULTS` probe의 `--eye-stencil-checks --output ABS`는 실제 픽셀·의미론적 fixture를 저장하지만 과거 취소/연결 해제 상황의 시각 현상 원인을 입증하지 않습니다.

`scripts/build-rig-native.mjs`는 범위 제한된 소스/컴파일러/출력 해시를 `native/target/rig-native/release/rig-native.json`에 기록합니다. 패키징은 소스·생성 출력의 해시를 확인하고 원본 기록을 `Contents/Resources/rig-native-build.json`에 포함하며 독립 업데이트 코디네이터와 protocol/서명 검사도 유지합니다. 앱 서명 **이후** 코디네이터를 포함한 `Contents`의 모든 배포 파일 해시를 외부 `dist/DesktopPet.app.rig-native.json`에 기록해 서명 번들 안의 자체 해시 순환을 피합니다. 릴리스 워크플로는 기존 앱 전용 압축 파일·체크섬 두 파일 외에 별도 `DesktopPet-v0.4.0-rig-native.json` 자산을 게시하도록 구성되어 있습니다. 실제 공개 게이트는 별도입니다. 설치기/업데이터의 아카이브 입력 형식은 바꾸지 않으며 실제 추출된 서명 후 파일과 사이드카의 해시를 비교해야 배포 바이트 출처를 주장할 수 있습니다.

## 저장소 구조

| 경로 | 역할 |
| --- | --- |
| [`native/contracts/`](../native/contracts/), [`native/core/`](../native/core/), [`native/app/`](../native/app/), [`native/update-coordinator/`](../native/update-coordinator/) | Neutral contracts, runtime, composition root and helper; adapters live in the integration repositories |
| [`native/`](../native/) | Rust 앱·네이티브 UI·세션 처리·캐릭터 관리 |
| [`native/rig/`](../native/rig/), [`web/rig/`](../web/rig/) | Rig 렌더링·디코딩·제작 지원 |
| [`assets/`](../assets/) | 기본 루벨리아 팩·메뉴 썸네일만 번들 |
| [`tools/`](../tools/) | 범용 캐릭터 제작·rig 검사 도구 |
| [`scripts/`](../scripts/) | 설치·네이티브 빌드·앱 패키징 |
| [`.github/workflows/release.yml`](../.github/workflows/release.yml) | 릴리스 패키징 워크플로 |

## 네이티브 현지화

네이티브 UI 문구는 정적 카탈로그 [`native/core/src/i18n.rs`](../native/core/src/i18n.rs)에 있으며 중국어 간체와 일본어 카탈로그는 각각 [`native/core/src/i18n/zh_hans.rs`](../native/core/src/i18n/zh_hans.rs), [`native/core/src/i18n/ja.rs`](../native/core/src/i18n/ja.rs)입니다. UI 메시지를 추가할 때는 모든 카탈로그를 갱신하고 런타임 값·경로·ID·팩 소유 메타데이터는 번역하지 마세요. 환경 설정의 정규 저장 토큰은 `system|ko|en|zh-Hans|ja`, 대화문 override 토큰은 `ko|en|zh-Hans|ja`이며 언어별 초안은 독립적입니다. CLI/패치 검사는 엄격하게 유지하고 기존 저장 설정의 알 수 없는 값에는 관대한 fallback을 유지하세요. `system`은 OS 선호 언어를 순서대로 검사합니다. 일본어 태그는 대소문자와 무관하며 중국어 `zh`·명시적 Hans·CN/SG 지역은 간체로 해석합니다. 명시적 Hant 또는 Hans가 없는 TW/HK/MO는 다음 지원 언어로 넘기고 없으면 영어를 사용합니다. 외부 카탈로그 메타데이터는 `ko`에 한국어, `en`·`zh-Hans`·`ja`에 영어 fallback을 사용하고 작성된 대화문과 fallback은 팩 소유를 유지합니다.

네이티브 문구나 CLI 언어 계약을 바꿀 때 [CLI 계약](cli.md), README [English](../README.md)·[한국어](../readme.ko.md)·[简体中文](../readme.zh-Hans.md)·[日本語](../readme.ja.md), 사용 안내 [English](usage.md)·[한국어](usage.ko.md)·[简体中文](usage.zh-Hans.md)·[日本語](usage.ja.md)를 함께 갱신하세요. 이 문서들은 네이티브 앱에 관한 것이며 홍보 웹사이트의 네 언어 번역은 별도로 관리하세요.

## 캐릭터 제작

팩 제작·검증은 [캐릭터 제작 가이드](../.agents/skills/character-creator/SKILL.md)와 [`tools/character-pack.py`](../tools/character-pack.py)를 참고하세요. 배포 앱에도 `Contents/Resources/creator/SKILL.md`·`Contents/Resources/creator/character-pack.py`가 포함됩니다. 소스 체크아웃의 [로컬 Qwen + See-through 제작 스킬](../.agents/skills/create-pet-character/SKILL.md)은 **승인된 그림에서** 독립된 10포즈를 만듭니다. 외부 엔진·모델 가중치는 번들에 포함하지 않습니다. 루벨리아가 기본 rig 템플릿입니다. PNG 템플릿·Aurora·Coding Cat·선택형 루벨리아 의상과 각각의 그림 생성기는 별도 [herdr-characters](https://github.com/hanbong5938/herdr-characters)에서 관리합니다. 외부 템플릿은 `--templates-root PATH`로 지정해야 하며 번들에 있다고 가정하지 마세요.

가져온 팩의 라이선스·출처·사용 조건 파일을 함께 보관하세요. 기본 루벨리아의 [라이선스](../assets/rubelia-default/LICENSE.txt)·[출처](../assets/rubelia-default/ATTRIBUTION.txt)·[제작 기록](../assets/rubelia-default/source-record.json)·[배경 보정 기록](../assets/rubelia-default/background-repair.json)이 그림 기준입니다. [`LICENSE.txt`](../LICENSE.txt)는 원래 Coding Cat 고지를 유지하지만 Coding Cat과 그 그림 생성 소스는 별도 캐릭터 저장소로 이동했습니다. 이 고지들이 저장소의 모든 구성 요소에 일괄 라이선스를 부여하지는 않습니다. 그림 소유자의 승인은 모델 권리를 부여하지 않으며 소프트웨어 MIT는 그림 라이선스를 바꾸지 않습니다. Qwen 연구 라이선스는 제작 기록으로 보존하고 모델 가중치·엔진은 번들에 넣지 않습니다. 버전별 다시 그리기·얼굴 보정·당시 한계는 [v0.1.6](releases/v0.1.6.md)·[v0.1.9](releases/v0.1.9.md)·[v0.1.10](releases/v0.1.10.md)을 참고하세요.

## 웹사이트 (GitHub Pages)

현재 사이트의 제품 시각 자료는 정적 이미지이며 로컬 Pages 작업에 동영상·MP4 자산이 필요하지 않습니다.

캡처를 게시하기 전에 화면 내용을 직접 확인하세요. 격리된 시연 프로필에서 앱만 담고 실제 세션 내용·계정명·로컬 경로·호스트명·이메일·인증 정보를 포함하지 마세요. 공개 PNG에는 이미지·색상 청크만 허용하며 `check`와 `stage`는 EXIF·텍스트·프로필 등 메타데이터를 거부합니다. 이 검사는 화면에 보이는 글자를 판별하지 않으므로 개인정보 수동 검토도 반드시 필요합니다.

한국어 기본 홍보 웹사이트는 [`site/`](../site/)에 있습니다. 네이티브 macOS 앱을 설명하는 사이트이며 브라우저에서 앱을 실행하지 않습니다. 예정 core Pages 주소(게시 확인 전): [한국어](https://hanbong5938.github.io/desktop-pet/) · [영어](https://hanbong5938.github.io/desktop-pet/en/) · [중국어 간체](https://hanbong5938.github.io/desktop-pet/zh-Hans/) · [일본어](https://hanbong5938.github.io/desktop-pet/ja/). 제품 문구를 바꿀 때 네 페이지의 번역과 언어 전환 링크를 함께 갱신하고 `ko`·`en`·`zh-Hans`·`ja`의 상호 `hreflang` 및 한국어 루트를 가리키는 `x-default`를 유지하세요. 각 페이지의 canonical URL과 Open Graph 언어도 해당 페이지에 맞춰야 합니다. 외부 [캐릭터 갤러리](https://github.com/hanbong5938/herdr-characters)는 별도로 유지되며 한국어·영어만 제공합니다. 과거 v0.3.2 스크린샷을 최신 화면이나 번역된 스크린샷처럼 소개하지 마세요. 영어 소셜 미리보기 이미지를 공유해도 페이지 언어는 바뀌지 않습니다. 제목은 화면 폭에 맞춘 간결한 크기와 좁은 주변 여백을 사용하며 본문·터미널 명령어·버튼 크기는 제목과 독립적으로 유지합니다. 저장소 루트에서 실행하세요.

```sh
bun test tools/pages.test.ts
node tools/pages.mjs check
node tools/pages.mjs serve --port 4173
# http://127.0.0.1:4173/desktop-pet/ 접속 (영어: /desktop-pet/en/; 중국어 간체: /desktop-pet/zh-Hans/; 일본어: /desktop-pet/ja/)
node tools/pages.mjs stage /tmp/herdr-pages
```

사이트 CLI에는 **Node.js 22**를 사용하고 **Bun 1.4.2**는 실제 Node 서버를 실행하는 HTTP 회귀 테스트의 러너로만 사용합니다. CI는 staging 전에 이 회귀 테스트를 실행합니다. `check`는 내부 링크·앵커·언어별 메타데이터·에셋을 확인하며 외부 링크를 네트워크로 검사하지 않습니다. 로컬 미리보기는 공개 파일 중 일반 파일만 제공합니다. 최종 `404.html` 대체 파일이 없거나 심볼릭 링크·일반 파일이 아니라면 파일을 전송하는 대신 `nosniff`가 있는 일반 텍스트 `500`을 반환하며 `HEAD`에는 본문이 없습니다. `stage`는 먼저 검사한 뒤 저장소 **밖의 새 디렉터리 또는 빈 디렉터리**에 공개할 `site/` 파일만 복사하며 배포하지 않습니다. Pages 워크플로는 PR을 검사하고 검토용 아티팩트만 첨부하며 배포하지 않습니다. Pages 배포 소스는 **GitHub Actions**이고, 웹사이트 변경을 `main`에 병합하거나 `main`에서 워크플로를 수동 실행했을 때 조건을 만족하는 `main` 실행만 배포합니다. 이 로컬 명령은 릴리스 바이너리·Wiki·저장소 Pages 설정을 바꾸지 않습니다.
