# 개발

[English](development.md) · **한국어** · [설치와 개요](../readme.ko.md)

아래 명령은 저장소 루트에서 실행합니다. [v0.3.6](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6)은 검증된 arm64 자산·체크섬과 안정판 Homebrew 포뮬러를 갖춘 최신 공개 안정판입니다. 소비자 설치에는 소스 체크아웃이 필요 없으며 아래 소스 개발은 선택 사항입니다. 지원 플랫폼은 Apple Silicon macOS 13 이상이며 Intel Mac·Windows·Linux용 네이티브 배포는 없습니다.

**공개 소비자 경로 검증:** 실제 안정판 Brew 0.3.5 → 0.3.6 업그레이드가 성공했으며 공개 고정 `--prebuilt` 설치기는 시스템 도구만 있는 `PATH`에서 완료됐습니다(Cargo·Bun·Node.js·npm 없음). 두 설치 앱 모두 별도 비공개 Tern config/state 프로필에서 `app_version: 0.3.6`, `observation_mode: tern`, `ui_ready: true`로 시작했고 설정된 Herdr 소켓의 부모 디렉터리도 필요하지 않았습니다. 유효한 빈 exchange-v1 스냅샷은 `command: null`로 수락됐으며 일반 Stop으로 깨끗하게 종료됐습니다. 실제 공개 플러그인 URL로 격리 Tern 설정에 0.1.0을 설치했고 카탈로그는 문제 없이 준비됐습니다. 이는 해당 소비자 경로의 근거이며 GUI 플러그인 창 연결·에이전트 목록/모델 전달·전체 업데이트 거래·일반 인증의 근거는 아닙니다. 기존 사용자 프로필은 변경하지 않았습니다.

## 소스 개발

**현재 체크아웃은 공개 바이너리보다 앞선 소스입니다:** Auto·실시간 호스트 설정은 미출시 소스 전용입니다. 소스 앱/도우미는 `{"protocol":3,"observation_mode":2}`를 보고하고 공개 v0.3.6은 capability 1·기본 Herdr 그대로입니다. 문서 수정으로 `--prebuilt`/Homebrew가 새 기능을 얻지 않습니다. 아래 새 계약에는 `--source` 빌드가 필요하며 위 소비자 검증은 공개 릴리스의 역사적 근거 그대로입니다.

이 체크아웃의 네이티브 앱을 소스에서 빌드하고 수동 시작하는 데 Herdr 플러그인·패치 호스트·client-attach 훅은 **필요하지 않습니다**. 지원되는 Apple Silicon Mac에서 Rust/Cargo·Bun·Node.js/npm·Xcode Command Line Tools·`codesign`을 준비하고 앱을 패키징하세요. 아래 수동 시작 예시는 정상 로컬 Herdr 서버가 이미 실행 중인 경우입니다:

```sh
bash scripts/install.sh --source
./dist/HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet start
./dist/HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet status
```

기존 로컬 Herdr 서버가 있다면 수동 `start`로 플러그인·패치 없이 세션 스냅샷(Herdr 0.9.0 이상)을 볼 수 있고 답장에는 해당 서버의 `agent.prompt`가 필요합니다. 소켓 지정·데몬 확인은 Homebrew 명령 대신 위 CLI로 [첫 연결 안내](../readme.ko.md#homebrew로-설치-첫-연결)를 따르세요. 현재 소스의 새 프로필은 Auto이며 정상 Herdr 없이도 캐릭터를 유지하고 Tern도 유지합니다. 호스트 없는 기존 프로필은 Herdr·기존 종료 정책을 유지하므로 Herdr 전용 독립 실행에는 [Herdr 없이 실행](../readme.ko.md#herdr-없이-캐릭터만-실행)처럼 시작 전 `exit_with_herdr off`를 설정하세요. 아래 호스트 절차는 **선택형** 관리형 실행·Herdr client-attach 자동 시작 전용입니다. 앱 빌드로 순정 호스트에 `client.attached`가 생기지 않으며 Auto가 Tern 자동 시작을 보장하지 않습니다.

### 로컬 Tern 연동 (사전 빌드 소비자 경로 또는 선택형 소스 개발)

소비자는 `brew install hanbong5938/tap/herdr-desktop-pet`으로 공개 공통 네이티브 앱을 설치하고 별도 공개 [tern-desktop-pet 플러그인](https://github.com/hanbong5938/tern-desktop-pet)을 `tern plugin install https://github.com/hanbong5938/tern-desktop-pet`으로 명시적으로 설치합니다. 사전 빌드 경로에는 Rust/Cargo·Bun·Node.js·체크아웃이 필요 없고 플러그인 설치가 앱을 몰래 내려받거나 빌드·실행하지 않습니다. 현재 소스 개발은 위 절차로 이 체크아웃을 빌드하며 연동 브랜치 전환은 필요 없습니다. 개발자는 `tern plugin link "$PWD/../tern-desktop-pet"`으로 형제 플러그인을 연결할 수 있습니다. [Tern 연결·선택형 격리 프로필 안내](usage.ko.md)에서 소스 전용 Auto/실시간 설정과 공개 v0.3.6을 구분하세요. 기존 Brew 사용자의 업그레이드는 `brew update && brew upgrade hanbong5938/tap/herdr-desktop-pet`이며 프로필·환경설정은 보존됩니다.
**현재 소스 호환성 주의:** 위 공개 소비자 경로는 오늘의 dirty 형제 플러그인과의 호환성을 입증하지 않습니다. 새 서명 앱/helper·integration API 정책을 이 native 체크아웃이 만족하지 않으므로 아래 차단 사항을 먼저 확인하세요. link/configure 절차가 실제 연결 성공을 뜻하지 않습니다.

현재 소스는 `--host auto|herdr|tern`을 허용하고 시작 우선순위는 명시 CLI > 명시 앱 소유 `HERDR_PET_OBSERVATION_MODE` > 저장 `observation.host` > 새 프로필 Auto입니다. host 없는 기존 설정은 Herdr, legacy `both`는 읽기 이전 전용·`auto` 정규화이며 기본 config의 OMpet 이전은 별도 프로필을 오염시키지 않습니다. 시작 전 host 저장, read/status는 실제 실행 모드 조회, 명시 비호환 start/ensure는 재사용 거절입니다. 실행 중 변경은 `preferences set --observation-host auto|herdr|tern`입니다. Auto는 우선순위·세션 중복 제거 없이 Herdr와 모든 연결된 SDK 노출 Tern host를 함께 관찰하지만 실제 원격 SDK 노출·Tern 자동 시작을 보장하지 않습니다. Auto/Tern은 `exit_with_herdr`와 무관하게 host loss 때 펫을 유지하고 Tern은 Herdr backend I/O를 하지 않습니다. `--socket`은 Herdr 전용입니다.

`--observation-host`와 wire patch는 정확한 `auto`, `herdr`, `tern`만 허용하며 `both`·대소문자 변경·공백·빈 값을 저장/IPC 전에 거절합니다. 타입이 있는 `desired`, `persisted`, `effective` 스냅샷에는 `observation_host`, 디스크에는 `observation.host`가 있습니다. host/local/remote/machines는 한 **저장 먼저** 거래로 처리하고 재시작 없이 적용합니다. `persist_failed`·CAS `revision_conflict`는 저장/실행 값을 유지하고 후속 다른 대상은 이전 pending 작업을 superseded로 만들 수 있으며 이전 네이티브 적용을 거짓 인증하지 않습니다. Tern 전용에서도 범위 선택은 보존합니다. 제공자를 끄면 소스 generation·경로/관찰 epoch를 폐기해 늦은 목록의 부활·오래된 키·대기/제시 프롬프트 재전송·제공자 간 재라우팅을 막습니다. `status.host_connections:{herdr:NUMBER,tern:NUMBER}`는 실제 연결되고 일관되며 지원되는 소스 수이지 설치·범위 활성화·행 수가 아닙니다. 어느 값이든 0보다 크면 `data_connected`입니다([CLI 계약](cli.md)).

네이티브와 Tern 플러그인은 기본 `${XDG_CONFIG_HOME:-$HOME/.config}/herdr/plugins/config/desktop-pet`, `${XDG_STATE_HOME:-$HOME/.local/state}/herdr/plugins/desktop-pet`을 공유합니다. 명시적인 기존 격리 프로필은 자동 병합·삭제·덮어쓰기하지 않습니다. 기존 기준 프로필을 선택하려면 **연결 → 연결 설정**의 실제 실행 파일/config/state를 Tern **Configure desktop pet connection**에 입력하고 Connect하세요. 한 번 검증·저장 후 신원/경로를 바꾸며 취소·저장 실패는 기존 경로를 유지하고 늦은 콜백은 차단합니다. 네이티브 주입 `HERDR_PLUGIN_CONFIG_DIR`/`HERDR_PLUGIN_STATE_DIR`가 CLI보다 우선하므로 수동 프로필 선택에서는 지우거나 일치시키세요. 연결 세부 그룹은 처음 접혀 있고 Tern 전용은 Herdr 원격 머신 제어를 숨기지만 선택을 버리지 않습니다. **앱 실행**은 Herdr 자동 시작과 호스트 선택을 구분하고 Herdr 모드에서만 Herdr 종료 연동을 제공합니다.

일반 `settings`는 별도 생명주기 창이 아니라 같은 데몬의 통합 **설정** 탭(탭 2)을 엽니다. `./dist/HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet settings`는 실행 프로필의 실제 호스트·자산을 재사용하며 프로필 쓰기·자산 조회·표시 변경을 하지 않습니다. 오프라인이면 명시적인 보기 요청으로 저장한 호스트·표시 상태(새 프로필 Auto)로 데몬을 시작하고 `--assets PATH`도 허용합니다. `settings`의 `--host`·관찰 모드 환경 재정의는 호스트를 바꾸지 않습니다. 제어 요청은 `ui::request_settings`를 큐에 넣고 준비 전·종료 중·업데이트 동결 인스턴스는 거절하며 별도 창으로 우회하지 않습니다. `settings get`/`settings set`은 데몬을 시작하지 않는 생명주기 하위 명령 그대로입니다. 현재 소스의 전환이며 공개판 생명주기 창 동작은 역사적 기록입니다.

현재 소스 Tern 수집기는 SDK 호스트 소속으로 `agents:list`를 묶고 Tern 제공자가 켜지면 연결된 모든 호스트를 자동 관찰합니다. Herdr `local`/`remote`/`machines`와 무관하며 새 호스트 선택 UI·SSH watcher·HTTP 서버는 없습니다. 창마다 1200 ms당 `process.run` 도우미 하나를 사용하고 오버헤드는 미측정입니다. 관찰 map과 로컬 전용 답장 map을 분리하여 신선하게 재검증한 로컬 대상만 `agents:get`/`agents:ask`를 호출합니다. 원격 CLI prompt는 `session_read_only`이며 명령을 제시하지 않습니다. 수락 영수증은 대기열 수락일 뿐 모델 전달·완료가 아니며 불명 전달은 재전송하지 않습니다.

현재 수정 계약: `status.herdr_socket`은 Tern 전용 모드에서도 고정된 UpdateContext의 설정 경로이며 실제 Herdr 연결의 증거가 아닙니다. 가짜 등록/소켓은 필요 없습니다. endpoint 신원은 빈/상대 경로를 거절하고 endpoint/부모가 없어도 같은 절대 경로를 허용하며, 그 외에는 전체 canonical 별칭 또는 canonical 부모+같은 파일명으로 비교합니다. basename만으로 대체하지 않습니다. 플러그인은 단일 정규 KV `connection={version=1,executable,config_dir,state_dir}`를 저장합니다. 전체 객체 검증 후 한 번 커밋한 다음 프로필/incarnation을 바꾸며 취소·실패는 기존 연결을 보존합니다. 정규 값이 없을 때만 이전 세 키를 한 번 채택하고 성공적인 커밋 뒤 정리합니다. 잘못된 정규 값은 이전 키로 대체하지 않으며 외부 공유 프로필 커밋은 지연 콜백을 차단합니다.

**현재 소스 exchange v2 — v1 fallback 없음:** 요청은 `version:2`, `kind:"tern"`, `window`, 양의 안전 정수 `epoch`/`sequence`, 바인딩한 `instance_id`, 최대 한 로컬 `results`를 유지하고 flat agents 대신 `hosts:[{host,label,is_local,observation_generation,status,agents}]`를 사용합니다. host는 SDK ID의 정규 십진 안전 정수 문자열, label은 호스트 이름(빈 이름은 ID)이며 로컬 호스트는 정확히 하나입니다. `available`은 완전한 최신 목록(빈 agents도 정상), `unavailable`은 신뢰 불가 목록, `disconnected`는 단절입니다. 뒤 두 상태는 빈 agents를 보내되 아래 유예 기간 동안 마지막 카드를 일시적으로 Offline으로 유지합니다. 응답은 최상위 generation 대신 `hosts:[{host,observation_generation,generation,error,error_code?}]`와 기존 command/error envelope를 제공합니다. native generation 0은 미입장입니다. 선택 오류 코드는 `binding_required|instance_changed|source_retired|receipt_rejected|rejected`입니다.

전체 shape·부모·receipt를 mutation 전에 검증합니다. 잘못된 envelope는 `ok:false,hosts:[],command:null`, 유효한 부분 입장은 `ok:true`와 해당 호스트만의 거절입니다. 기존 호스트를 먼저 갱신하고 신규는 로컬 우선·숫자 ID 순으로 입장시키며 응답 배열은 요청 순서입니다. 신규 원격 거절은 기존 갱신·로컬 receipt를 막지 않고 다음 정상 poll에서 입장을 재시도합니다. 한도는 요청당 64호스트·호스트당 128에이전트·Herdr+Tern 공유 64소스·부모 창 64개입니다. 호스트별 오류는 UTF-8 경계의 512바이트, 기존 일반 필드/최상위 오류는 4096바이트입니다. 요청 JSON은 framing LF/CRLF 제외 256 KiB, 전체 응답은 LF 포함 512 KiB입니다. 실제 host reply 직렬화 크기를 admission/offer 전에 계산합니다.

소스는 창×호스트 `tern:<hex(window)>:<hex(host)>`이며 기존 exact session key의 native generation과 플러그인 observation generation은 별개입니다. 관찰 세대는 관측된 단절·복귀·kind/address 변경을 나타내며 poll 사이에 끝난 실제 transport 재접속을 검출한다고 보장하지 않습니다. 이름 변경·정상 poll은 키를 유지하고 부모 epoch 증가는 모든 자식을, 호스트 관찰 세대 증가는 해당 자식만 교체합니다. 소실·unavailable·단절·5초 신선도 만료는 해당 호스트를 Offline으로 만들 뿐 완료를 생성하지 않습니다. 복귀는 native만 만료되어 플러그인 세대가 같아도 더 큰 native generation을 받습니다. 명시적 단절/소실 뒤 같은/낮은 관찰 세대로 재활성화할 수 없습니다. 부모 만료는 모든 자식을 Offline으로 만들며 다른 호스트 heartbeat는 unavailable 자식을 갱신하지 않습니다. 소스 ID는 창×호스트를 식별하므로 복귀 때 같은 ID를 사용하되 generation으로 오래된 키를 차단합니다. 첫 경로 소실 때 로컬 offered work는 `unknown_delivery`, 미제시 work는 `offline`으로 확정 실패하며 둘 다 재전송하지 않습니다. 원격 소실은 로컬 pending에 영향이 없습니다.

**현재 소스 Offline 회수:** 5초 신선도 기준은 그대로이며 Offline Tern 소스에는 별도의 단조 시계 기준 30초 유예 기간이 있습니다. 자연 만료의 시작은 늦은 tick의 발견 시점이 아니라 `last_seen + 5s`이고, 명시적 단절·unavailable·완전한 호스트 목록의 누락·epoch 교체는 처음 유효하게 관측한 소실 시점입니다. 반복 소실·다른 호스트 heartbeat는 이 시계를 연장하지 않고 수락한 일관된 복귀는 초기화합니다. 기존 100 ms 제어 tick의 `maintain_tern`은 신선도 처리와 회수를 모두 실행하며, 유예 조건 충족 뒤 다음 tick에서 신규 입장 없이 실제 소스·보존 행·집계·registry 자식을 제거합니다. 정확한 wall-clock 시각을 보장하지는 않습니다. 정상 `available`의 빈 호스트는 live로 유지하고 빈 Offline 소스도 제거합니다. live/Herdr 소스는 TTL로 제거하지 않으며 기존 공유 64소스 입장 압력은 Offline Tern을 더 일찍 회수할 수 있습니다. 원격 자식 제거는 live 부모의 binding·관찰 watermark·로컬 exact key·로컬 pending을 보존하고, 죽은 빈 부모는 결국 회수합니다. 세대 검증 제거는 session revision을 증가시켜 제거된 키·오래된 cursor를 거절하며 opaque ID를 재사용하지 않습니다. 세대/epoch high-water map은 보존하므로 과거 메타데이터 전체의 고정 메모리 상한을 뜻하지 않습니다. exchange/helper의 `expire_tern`은 신선도만 처리하고 보존 회수는 하지 않습니다. 공유 AppState lock이 복귀와 유지 관리를 직렬화합니다. 회수 전에 수락한 일관된 복귀는 살아남고, 회수가 먼저면 오래된 bound 부모 요청은 `source_retired`입니다.

연결 수는 건강한 빈 호스트를 포함한 live coherent 창×호스트 소스 수이며 같은 머신이 두 창에 있으면 두 소스입니다. 카드·검색·Source 정렬은 호스트 이름과 네 언어 Local/Remote 표기를 사용합니다. Auto에서도 local/remote/machines는 Herdr만 제한하고 Tern 전용에서는 저장한 Herdr 선택을 유지하며 scope 영역을 숨깁니다.

**소스 전용 검증:** native HEAD `ed1fe248199697b3a61d208f9a42bc741e52a858`, plugin HEAD `9a3f0bca4970c2adf69879b4a7a24525fa70f136`의 dirty 작업 소스에서 수행했습니다. native targeted Tern 57·session-view 37·sources 5개, full native 743개 통과·1개 ignored와 실제 AppKit composer 실행 파일, coordinator 73개(51 library/22 binary), Bun check·59테스트, arm64 release/package·로컬 ad-hoc 서명이 통과했습니다. 실제 Tern 0.8.3(`e2b8656`) VM에서 생산 소스/제어 SDK 회귀 132/132·`done=true`를 확인했으며 실제 ask는 없었습니다. eager 84 KiB 컴파일이 실제 50 ms 예산을 넘어서 harness는 일반 모듈을 나눠 로드합니다(plugin `tests/transition-regressions.luau`).

패키지 실행 파일 `NATIVE_ROOT/dist/HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet`은 `/tmp/tern-remote-smoke.kys5_x4p` 아래 격리 config/state·증거 경로를 사용하여 `ui_ready:true`였습니다. 앱을 그 임시 경로로 옮긴 것은 아닙니다. 실제 `tern-exchange` 도우미에 주입한 L/A/B fixture는 같은 pane의 독립 키 3개·label/locality·Herdr local/remote off에서도 관찰, A만 Offline/복귀·L/B 키 유지, 5.4초 native-only 만료 뒤 새 native generation, rename 키 유지를 입증했습니다. 원격 CLI는 `session_read_only`·command 없음, 로컬 주입 명령은 한 번 offer·주입 accepted receipt로 `prompt_accepted`였습니다. **실제 SDK ask·모델 답변 증거가 아닙니다.**

**이후 현재 소스 회수 검증(공개 v0.3.6 변경 아님):** native 760개 통과·1개 ignored, 별도 중첩 helper subprocess 테스트 통과, 렌더링 snapshot을 포함한 실제 main-thread AppKit composer·native insertion·marked-text harness 통과를 확인했습니다. coordinator 73개·TypeScript check·Bun 59개·실제 Tern VM 생산 소스/제어 경계 Luau 132/132도 통과했습니다. 비공개 프로필의 소스 debug 앱에 주입한 채워진 v2 IPC fixture는 처음 live 64/Offline 0/행 64에서 신선도 만료 뒤 live 1/Offline 63/행 64, 유예 뒤 live 1/Offline 0/행 1로 바뀌었으며 신규 입장이나 daemon instance 변경이 없었습니다. 원격 자식 소실·회수·복귀는 로컬 exact key를 보존하고 새 generation/opaque ID로 복귀했으며 오래된 관찰 세대와 회수된 부모의 기존 bound 요청을 거절했습니다. 별도의 실제 SDK 빈 호스트 bridge는 로컬 host 0과 소유한 in-process 원격 loopback host 1을 관측했습니다. 연결 2/Offline 0/session 0에서 문서화된 stalled 시나리오로 연결 1/Offline 1, 32초 반복 stalled 관찰 뒤 새 부모/입장 없이 연결 1/Offline 0이 됐고 connected 복귀는 로컬 native generation을 유지하며 연결 2를 복원했습니다. 설치된 생산 플러그인 연결·실제 원격 agent·외부 네트워크 장애·semantic ask 근거는 아닙니다. 사용자가 승인한 권한 없는 AppKit/IPC/선택·cursor 검증은 실제 카드 클릭 자동화나 물리 IME 인증을 주장하지 않습니다.

**현재 차단 사항:** 이전 실제 SDK 탐색 창에는 host 0·로컬 agent 5개뿐이고 CLI 원격 host 목록은 비었습니다. 실제 원격 agent 노출은 연결된 원격-agent 창이 필요하며 위의 이후 소유 loopback 검증은 빈 목록만 사용했습니다. 동시 변경된 플러그인 설치 정책은 서명된 `DesktopPet.app/Contents/MacOS/desktop-pet`·짝을 이루는 중립 helper·`integration_api:1`·`protocol:4`·`observation_mode:2`를 요구합니다. 이 native 소스는 여전히 `HerdrDesktopPet.app/herdr-desktop-pet`·updater protocol 3/mode 2이고 installation API가 없습니다. 따라서 **실제 native/plugin 설치·연결 일치는 차단되어 입증되지 않았으며**, 이 체크아웃의 소스 빌드만으로 해결되지 않습니다. 정책을 우회하지 않습니다. AX false·System Events timeout·window/region capture 실패·ScreenCaptureKit -3811로 호스트 label/검색/정렬·read-only composer·네 언어·layout의 실제 시각 검증을 주장하지 않습니다. 아래 공개 v1·이전 semantic delivery 기록은 역사적 증거이지 현재 v2 증거가 아닙니다.

50 ms SDK 콜백 수정은 신선한 목록을 네 번의 `tern.sleep(0):next` microtask로 나눕니다. tabs·sessions·agents는 각각 별도 콜백, hosts·panes는 마지막 콜백에서 함께 읽고 순수 검증을 수행합니다. 콜백당 목록 API는 최대 두 개(API당 12 ms fixture에서 24 ms)입니다. busy/active-work가 비동기 교환 전체의 incarnation/프로필/단계를 보호하고 최종 중요 local/pane/agent 검사는 일관된 신선한 context를 사용합니다. accepted·failed 보고는 기존의 신선한 전체 목록 교환을 즉시 시작하며 변경 없는 1200 ms 주기 타이머를 기다리지 않습니다. 캐시 목록·ask 재시도·새 프로토콜·네이티브 5초 수락 영수증 기한 증가는 없습니다. 데스크톱 부하에서 1 ms 타이머 콜백 지터가 997 ms까지 측정돼 목록의 coarse timer 인계를 제거했습니다. 개별 SDK 호출이나 OS 선점은 이 예산 모델 밖이며 무기한 지연 보장이나 일반적인 프로세스 오버헤드 벤치마크는 아닙니다.

요청 한도는 정확히 JSON 256 KiB이며 선택적 framing LF/CRLF는 세지 않습니다. 응답은 LF 포함 전체 512 KiB 그대로입니다. 이전 실제 CLI 요청 411918바이트는 수정 전 허용됐지만 이제 IPC 전에 거절합니다. 소스 한도 때문에 입장이 거절된 건강한 Herdr 연결은 유지하고 여유가 생기면 정상 다음 일관된 스냅샷에서 허용합니다. 첫 기준선은 과거 완료를 만들어내지 않으며 별도 재시도 스레드/backoff는 없습니다.

**공개 전 수행한 수정 검증 기록:** 네이티브 674개 통과·1개 무시와 실제 AppKit composer 실행 파일, 코디네이터 70개(라이브러리 49·바이너리 21), TypeScript/Bun 58개, arm64 릴리스/패키지 및 ad-hoc strict/deep 서명 검증을 확인했습니다. 실제 Tern 0.6.1의 생산 `window.luau`를 제어된 SDK로 실행한 **44/44개**가 통과했습니다. 기존 사례, API 예산, coarse timer와 microtask 기한 모델, awaitable 실패 정리 및 신원/프로필/local/pane/host fence를 포함합니다. 독립 모델이 아닌 실제 생산 Luau의 제어된 SDK 근거이며 의미 있는 전달 전체 증거는 아닙니다. 설정 소켓 부모가 없는 서명 Tern 전용 UI 준비와 바인딩/source-retired wire 검사를 확인했습니다. 실제 300 KiB ASCII와 이스케이프 Unicode 원본 300000바이트는 각각 307361·390161바이트 응답으로 보존됐지만 주입한 수락 영수증이지 실제 모델 전달은 아닙니다. 제어 문자 이스케이프 입력은 offer 없이 `prompt_too_large`로 확정 거절했습니다. 게시·커밋은 주장하지 않습니다.

실제 패키지 Tern 데몬의 lifecycle 검사에서 신선한 소스 64개를 허용했고 5.25초 뒤 연결 소스는 0개였습니다. 새 관측을 허용한 뒤 축출된 기존 바인딩 창은 `source_retired`를 반환했습니다. 이는 소스 한도/만료 wire 근거이지 도우미 거래나 의미 있는 ask의 증거는 아닙니다.

**이전 공개 전 도우미 근거:** 당시 서명된 Tern 도우미와 빌드한 legacy base-HEAD 도우미는 설정 소켓 부모 없이 staging·사전 검사를 통과했습니다. 당시 도우미는 UI 승인 없는 수동 fixture에서 `update plan or operation changed; check again` guard에 도달했습니다. 정확한 Original NoSpawn 복구는 `Failed`/`no_spawn`, `installed:false`, `applied:false`, 예약 해제 및 기존 PID/instance/image/source/profile 유지를 입증했습니다. 모드 capability 없는 `{"protocol":3}` legacy 도우미는 키 없는 기본 Herdr에서 통과했습니다. 당시 capability-1 소스에서는 명시적 Herdr 등 모드 키가 있으면 capability 1이 필요했습니다. **Completed GUI Apply·새 Candidate 인계 증거는 아닙니다.** 자동화의 `AXIsProcessTrusted=false`였고 네이티브는 UI 승인 없는 계획을 거절했습니다.

이전 실제 소유 에이전트 transcript에서 `TERN_REPAIR_SMOKE_OK`를 관측했고 중간 6단계의 `unknown_delivery`는 `TERN_COOPERATIVE_SMOKE_OK` transcript로 조정했으며 맹목 재전송하지 않았습니다. 최종 네 microtask 생산 코드는 reload grace 뒤 현재 live 소스에서 실제 `agents:ask`를 기한 내 수락했습니다: `prompt_accepted`, `committed:false`, `native_applied:false`, `delivery:"accepted_may_be_queued"`. 이후 소유 assistant가 정확히 `TERN_MICROTASK_SMOKE_OK`로 답해 해당 end-to-end 답변을 입증했지만 일반 완료 이벤트나 무기한 신뢰성 보장은 아닙니다. 15초 동안 연결 소스 표본 다섯 번과 실제 정규 A→B 프로필 전환을 관측했고 6.25초 뒤 기존 A는 끊기고 새 B는 연결됐습니다. 무제한 실행 안정성의 증거는 아닙니다.

이전 소스 검사에서 네이티브 테스트 650개(무시 1개)와 실제 AppKit composer 실행 파일, 코디네이터 테스트 67개, TypeScript와 Bun 테스트 58개, arm64 릴리스 앱/도우미 빌드 및 ad-hoc 패키징이 통과했습니다. 격리된 서명 패키지의 Tern 전용 앱은 네이티브 창과 `observation_mode:tern`을 표시했고 `exit_with_herdr:true`여도 91초 넘게 실행되었습니다. **주입한** exchange 목록에서 유니코드/여러 줄 명령이 한 번 제시되었고 주입한 수락 결과는 `prompt_accepted`, `committed:false`, `delivery:"accepted_may_be_queued"`로 기록되었습니다. 목록 만료는 완료 결과 없이 Offline이 되었으며 오래된 워크트리 inspect와 다른 호스트 키의 재사용은 거절되었습니다. 이는 이전의 주입 브리지 관측이며 **실제 Tern 에이전트 전송·완료 증거는 아닙니다**. 이전에 설치된 로그아웃 상태 Tern 0.6.1 창은 별도 플러그인을 열어 `agents:list`/`process.run`을 호출했으나 목록이 비었습니다. 실제 에이전트 목록과 의미 있는 ask는 **그 이전 로그아웃 실행에서만** 미검증이었습니다. 인증 우회는 없었고 이 검사로 GitHub 릴리스·tap·사이트 배포를 수행하지 않았습니다.

추가로 일반 로그아웃 상태 Tern 창과 패키지 바이너리·짧은 격리 XDG 프로필에서 실제 hosts/panes/tabs/sessions/agents(빈 목록) API, 타이머, `process.run`, 전용 교환을 플러그인 오류 없이 실행했습니다. 네이티브 상태는 `observation_mode:tern`, `data_connected:true`, `connected_sources:1`, `sessions:0`, `ui_ready:true`였습니다. 실제 창을 닫은 뒤에도 앱은 실행 중이었고 6초 후에는 `data_connected:false`, `disconnected_sources:1`이었습니다. 주입한 명령 결과 누락은 재제시 없이 `unknown_delivery`가 되었으며 epoch 교체 후 옛 epoch 요청은 `ok:false`로 거절되었습니다. 캡처한 모드로 다시 시작해 새 Tern 인스턴스에 도달했지만 **도우미 주도 전체 업데이트 거래의 완료 증거는 아닙니다**. 위 테스트 수는 메인 649개 통과·1개 무시(메인 총 650개 항목)이며 중첩된 자식 도우미는 별도입니다.

공개 전 이후 검증에서 **이미 실행 중인 현재 사용자 Tern 창**에서 명시적으로 설정한 전용 검증 프로필·플러그인 kv와 Tern CLI로 생성한 본인 소유 일회용 OMP 탭을 사용해, 실제 창 플러그인이 에이전트가 있는 목록을 전용 소켓으로 교환했습니다. 별도의 기존 사용자 pane도 네이티브/플러그인 행에서 확인했습니다. 소유한 pane에 보낸 네이티브 `sessions prompt`는 실제 Tern `agents:ask` 수락 결과 뒤 `prompt_accepted`, `committed:false`, `delivery:"accepted_may_be_queued"`를 반환했습니다. 같은 소유 pane의 Tern `agents:transcript`에는 사용자 요청 `Reply with exactly TERN_PET_SMOKE_OK. Do not use tools or modify any files.`와 assistant 응답 `TERN_PET_SMOKE_OK`가 확인되었고, 에이전트 상태는 idle이며 마지막 메시지도 일치했습니다. 이는 해당 ask와 응답의 증거이지 권위 있는 완료 이벤트나 성공 애니메이션의 증거가 아닙니다. idle만으로 완료를 인증할 수 없습니다. 이전의 새 CLI 창 Keychain 오류는 그 별도 창에 한정되며 현재 사용자 로그인 장애가 아닙니다. 도우미 주도 전체 업데이트 거래·앱 확장·검증 에이전트의 도구/파일 변경·공개 플러그인/아카이브 게시는 없었습니다.

현재 소스의 updater 프로토콜·계획·저널은 **3**, 앱/도우미 capability는 `{"protocol":3,"observation_mode":2}`입니다. 실제 선택 호스트를 캡처해 Herdr 키는 생략하고 그 외 정규 `auto`/`tern`을 기록하며 무관하게 상속한 모드를 유지하지 않습니다. 이전 context 키 부재는 Herdr입니다. Auto는 서명된 후보 앱·도우미 capability 2와 정확한 Auto 준비 상태가 필요합니다. 이전 capability 1은 Herdr/Tern에만 보수적으로 허용하며 이전 `both` context를 읽어도 Auto에는 안 됩니다. capability 없는 protocol-3 후보는 Herdr 전용이고 capability 없는 도우미는 키 없는 Herdr에서만 허용합니다. 이전 명시적 Herdr context는 알려진 capability 1/2가 필요하며 미지·잘못된·미래 값은 거절합니다. 성공한 호스트 변경은 캐시 context를 갱신해 이전 진행 중 확인 결과·후보·동의를 무효화하므로 새 확인/동의가 필요합니다. 이력·도우미 소유 계획·저널·lease·예약은 보존하고 실제 준비 중에는 충돌하는 설정을 `update_preparing`으로 거절합니다. 정확한 Original/Candidate 출처 pin·고정 소켓·사용자 Stop·기존 복구 허용 규칙은 그대로이며 불명확한 manager 실행은 설치 재실행 허가가 아닙니다.

[Herdr 호스트 연동 안내](../integrations/herdr/README.md#build-in-separate-checkouts)의 별도 체크아웃 절차를 먼저 따르세요. 공식 Herdr 0.9.3 소스 커밋 `7b116c05bfda646af39d2524c54e70c751f57ee8`에 [client-attach 패치](../integrations/herdr/client-attached.patch)를 적용해 호스트를 빌드하고, 빌드된 실행 파일에 `PATCHED_HERDR="$HERDR_SOURCE/target/release/herdr"`를 지정합니다. 해당 안내의 고정 호스트 도구와 격리된 XDG/세션 설정을 **먼저** 준비하세요. 플러그인 링크는 호스트나 앱을 빌드하지 않습니다. 순정 Herdr 0.9.0과 0.9.3에는 `client.attached` 훅이 없고 `min_herdr_version = "0.9.3"`은 패치 기준일 뿐 훅 지원의 증거가 아닙니다. 실제로 훅 지원을 명시한 향후 호스트 릴리스에서만 패치를 대체할 수 있습니다. 연결 경고·패치된 세션의 API 구독/스키마·안내의 클라이언트 attach probe를 확인하세요. 알 수 없는 훅 경고나 연결 성공만으로는 성공이 아닙니다. `PATH`의 다른 `herdr`가 아닌 동일한 패치 바이너리와 세션을 확인해야 합니다.

연결할 개발 체크아웃은 먼저 직접 빌드하세요(관리형 설치와 달리 `plugin link`는 `[[build]]`를 실행하지 않습니다).

```sh
bash scripts/install.sh --source
"$PATCHED_HERDR" plugin link "$PWD" --enabled
"$PATCHED_HERDR" plugin action invoke start --plugin desktop-pet
```

소스 설치기는 고정된 Bun·npm 의존성을 설치하고 Rust 실행 파일·업데이트 코디네이터를 빌드한 뒤 네이티브 rig 런타임·캐릭터 제작 리소스를 패키징하고 앱을 검증합니다. Rust/Cargo·Bun·Node.js/npm·Xcode Command Line Tools·`codesign`이 필요하며 별도 호스트 빌드 조건은 연동 안내를 따릅니다. 이미 실행 중인 서버에서 연결·활성화만 해도 캐릭터가 시작되는 것은 아닙니다. `start`를 한 번 실행하거나 `auto_start`가 켜졌다면 이후 정상적인 shell/terminal 클라이언트 attach를 기다리세요. 서버 시작 시에도 `ensure`가 자동으로 실행됩니다. 플러그인 액션에는 실행 중인 활성 호스트가 필요하지만 패키지 네이티브 바이너리의 오프라인 설정 명령에는 필요하지 않습니다. 소스 자동화에는 `./dist/HerdrDesktopPet.app/Contents/MacOS/herdr-desktop-pet`을 사용하세요. 기존 데몬을 종료하거나 `start`·명령·`stop` 모두에 일관된 **설정과 상태** 격리 경로를 사용하세요. 소스 CLI 버전만으로 자산 게시나 구형 실행 데몬의 교체를 증명하지 못하므로 `status`의 실제 실행 파일을 확인하세요.

공통 [설치기](../scripts/install.sh)는 세 모드를 제공합니다. 옵션이 없으면 매니페스트에 고정된 사전 빌드를 시도하고 **없거나 검증 실패한 경우에만** 소스 빌드로 전환합니다. `--prebuilt`는 실패하고 `--source`는 이 체크아웃을 빌드합니다(Bun/npm 의존성은 내려받을 수 있음). 관리형 Herdr 플러그인 설치는 `[[build]]`를 통해 설치기 실행 후 플러그인 디렉터리의 `dist/`에 앱을 둡니다. 매니페스트는 공개 `0.3.6` 사전 빌드를 고정하며 fallback 소스 빌드와 공개 자산 검증은 별개입니다. 성공한 사전 빌드 설치에는 소스 빌드 도구가 필요 없습니다. 설치기는 기존처럼 `HerdrDesktopPet.app/`만 포함한 압축 파일을 받아 체크섬·네이티브 서명을 검증하고 별도 rig 출처 기록을 설치 입력으로 사용하지 않습니다. client-attach 자동 시작에는 여전히 기능이 있는 패치 호스트가 필요합니다.

저장소의 `herdr-plugin` 토픽으로 [Herdr 마켓](https://herdr.dev/plugins/) 자동 수집 대상에 등록됩니다. 인덱스는 30분마다 갱신되며 **심사받지 않은 커뮤니티 목록**이지 호스트 호환성 검토가 아닙니다. Homebrew [개인 tap](https://github.com/hanbong5938/homebrew-tap) 포뮬러는 `HerdrDesktopPet.app`을 `/Applications`가 아닌 Homebrew prefix에 설치하고 CLI를 `PATH`에 놓습니다. Herdr 플러그인이나 패치 호스트는 설치하지 않으므로 실행 관리 훅에는 플러그인, client-attach 자동 실행에는 호스트 패치가 별도로 필요합니다. 포뮬러 로딩이 거부된 경우에만 `brew trust --formula hanbong5938/tap/herdr-desktop-pet`으로 이 포뮬러만 신뢰하세요. 업그레이드 전 데몬을 종료하세요. `start`는 기존 데몬을 재사용하므로 CLI `--version`과 재시작 후 `status`의 `app_version` 및 실행 파일을 함께 확인하세요. 옛 cask는 `/Applications`에 앱을 복사했습니다. `brew uninstall --cask herdr-desktop-pet` 후 `brew install hanbong5938/tap/herdr-desktop-pet`으로 포뮬러에 이전할 수 있으며 사용자 데이터는 유지됩니다. 안정판·베타 프로필 격리와 이전은 [한영 0.3 이전 안내](releases/0.3.md#한국어--v03-계열)를 참고하세요. 소스 버전만으로 공개 자산이 존재한다고 추정하지 마세요.

## 네이티브 빌드와 서명

소스 설치 후 네이티브 검사는 다음 명령으로 실행합니다.

```sh
bun run check
bun run test:native
```

네이티브 hit-overlay 혼합 계산은 릴리스 러너의 Xcode 16.4 Swift 컴파일러와 호환되도록 타입을 지정한 부분 식을 사용합니다. 오버레이를 고칠 때 이 타입 경계를 유지하세요. 소스 빌드에는 여전히 Xcode Command Line Tools가 필요합니다.

debug·release 네이티브 빌드는 `codesign`을 필요로 하며 [`native/rig/RigDecodeWorker.entitlements.plist`](../native/rig/RigDecodeWorker.entitlements.plist)의 `com.apple.security.cs.allow-jit=true`를 **`rig-decode-worker`에만** 부여해 ad-hoc 서명합니다. Cargo는 plist 변경을 추적해 worker를 다시 빌드·서명합니다. JavaScriptCore가 신뢰된 디코더 번들을 JIT 컴파일할 수 있게 하지만 앱 본체와 네이티브 rig 라이브러리에는 **JIT 권한이 없으며** worker 격리·검증·자원 제한은 유지합니다. `bun run package:native`는 선택한 서명 identity로 worker 전용 권한을 다시 적용하고 외부 앱 서명 후 실제 entitlement 및 worker·앱의 엄격한 서명을 검증합니다. 서명·검증 실패 시 **패키징을 중단**합니다. 기본 ad-hoc 패키지는 로컬 실행용이며 Developer ID 서명·공증이 아닙니다.

Rust/Swift rig 경계는 타입 있는 asset/token을 넘기기 **전에** ABI 버전과 C 구조체 일곱 개의 크기를 확인합니다. 초기 motion은 네이티브 호스트가 복사하는 봉인된 바이트이며 옛 경로 조회 방식으로 돌아가지 않습니다. Metal은 같은 인코더의 색상 그리기 전에 실제 흰자 구멍을 L/R/무방향 stencil로 준비하고 CPU 의미론적 hit·참조 래스터도 같은 소유권 규칙을 적용합니다. 흰자가 없는 rig의 기존 홍채 표시 동작은 유지합니다. `RIG_PROBE_FAULTS` probe의 `--eye-stencil-checks --output ABS`는 실제 픽셀·의미론적 fixture를 저장하지만 과거 취소/연결 해제 상황의 시각 현상 원인을 입증하지 않습니다.

`scripts/build-rig-native.mjs`는 범위 제한된 소스/컴파일러/출력 해시를 `native/target/rig-native/release/rig-native.json`에 기록합니다. 패키징은 소스·생성 출력의 해시를 확인하고 원본 기록을 `Contents/Resources/rig-native-build.json`에 포함하며 독립 업데이트 코디네이터와 protocol/서명 검사도 유지합니다. 앱 서명 **이후** 코디네이터를 포함한 `Contents`의 모든 배포 파일 해시를 외부 `dist/HerdrDesktopPet.app.rig-native.json`에 기록해 서명 번들 안의 자체 해시 순환을 피합니다. 릴리스 워크플로는 기존 앱 전용 압축 파일·체크섬 두 파일 외에 별도 `HerdrDesktopPet-v0.3.6-rig-native.json` 자산을 게시합니다. 설치기/업데이터의 아카이브 입력 형식은 바꾸지 않으며 실제 추출된 서명 후 파일과 사이드카의 해시를 비교해야 배포 바이트 출처를 주장할 수 있습니다.

## 저장소 구조

| 경로 | 역할 |
| --- | --- |
| [`herdr-plugin.toml`](../herdr-plugin.toml) | Herdr 플러그인 실행 관리·액션 정의 |
| [`native/`](../native/) | Rust 앱·네이티브 UI·세션 처리·캐릭터 관리 |
| [`native/rig/`](../native/rig/), [`web/rig/`](../web/rig/) | Rig 렌더링·디코딩·제작 지원 |
| [`assets/`](../assets/) | 기본 루벨리아 팩·메뉴 썸네일만 번들 |
| [`tools/`](../tools/) | 범용 캐릭터 제작·rig 검사 도구 |
| [`scripts/`](../scripts/) | 설치·네이티브 빌드·앱 패키징 |
| [`.github/workflows/release.yml`](../.github/workflows/release.yml) | 릴리스 패키징 워크플로 |

## 네이티브 현지화

네이티브 UI 문구는 정적 카탈로그 [`native/src/i18n.rs`](../native/src/i18n.rs)에 있으며 중국어 간체와 일본어 카탈로그는 각각 [`native/src/i18n/zh_hans.rs`](../native/src/i18n/zh_hans.rs), [`native/src/i18n/ja.rs`](../native/src/i18n/ja.rs)입니다. UI 메시지를 추가할 때는 모든 카탈로그를 갱신하고 런타임 값·경로·ID·팩 소유 메타데이터는 번역하지 마세요. 환경 설정의 정규 저장 토큰은 `system|ko|en|zh-Hans|ja`, 대화문 override 토큰은 `ko|en|zh-Hans|ja`이며 언어별 초안은 독립적입니다. CLI/패치 검사는 엄격하게 유지하고 기존 저장 설정의 알 수 없는 값에는 관대한 fallback을 유지하세요. `system`은 OS 선호 언어를 순서대로 검사합니다. 일본어 태그는 대소문자와 무관하며 중국어 `zh`·명시적 Hans·CN/SG 지역은 간체로 해석합니다. 명시적 Hant 또는 Hans가 없는 TW/HK/MO는 다음 지원 언어로 넘기고 없으면 영어를 사용합니다. 외부 카탈로그 메타데이터는 `ko`에 한국어, `en`·`zh-Hans`·`ja`에 영어 fallback을 사용하고 작성된 대화문과 fallback은 팩 소유를 유지합니다.

네이티브 문구나 CLI 언어 계약을 바꿀 때 [CLI 계약](cli.md), README [English](../readme.md)·[한국어](../readme.ko.md)·[简体中文](../readme.zh-Hans.md)·[日本語](../readme.ja.md), 사용 안내 [English](usage.md)·[한국어](usage.ko.md)·[简体中文](usage.zh-Hans.md)·[日本語](usage.ja.md)를 함께 갱신하세요. 이 문서들은 네이티브 앱에 관한 것이며 홍보 웹사이트의 네 언어 번역은 별도로 관리하세요.

## 캐릭터 제작

팩 제작·검증은 [캐릭터 제작 가이드](../.agents/skills/character-creator/SKILL.md)와 [`tools/character-pack.py`](../tools/character-pack.py)를 참고하세요. 배포 앱에도 `Contents/Resources/creator/SKILL.md`·`Contents/Resources/creator/character-pack.py`가 포함됩니다. 소스 체크아웃의 [로컬 Qwen + See-through 제작 스킬](../.agents/skills/create-pet-character/SKILL.md)은 **승인된 그림에서** 독립된 10포즈를 만듭니다. 외부 엔진·모델 가중치는 번들에 포함하지 않습니다. 루벨리아가 기본 rig 템플릿입니다. PNG 템플릿·Aurora·Coding Cat·선택형 루벨리아 의상과 각각의 그림 생성기는 별도 [herdr-characters](https://github.com/hanbong5938/herdr-characters)에서 관리합니다. 외부 템플릿은 `--templates-root PATH`로 지정해야 하며 번들에 있다고 가정하지 마세요.

가져온 팩의 라이선스·출처·사용 조건 파일을 함께 보관하세요. 기본 루벨리아의 [라이선스](../assets/rubelia-default/LICENSE.txt)·[출처](../assets/rubelia-default/ATTRIBUTION.txt)·[제작 기록](../assets/rubelia-default/source-record.json)·[배경 보정 기록](../assets/rubelia-default/background-repair.json)이 그림 기준입니다. [`LICENSE.txt`](../LICENSE.txt)는 원래 Coding Cat 고지를 유지하지만 Coding Cat과 그 그림 생성 소스는 별도 캐릭터 저장소로 이동했습니다. 이 고지들이 저장소의 모든 구성 요소에 일괄 라이선스를 부여하지는 않습니다. 그림 소유자의 승인은 모델 권리를 부여하지 않으며 소프트웨어 MIT는 그림 라이선스를 바꾸지 않습니다. Qwen 연구 라이선스는 제작 기록으로 보존하고 모델 가중치·엔진은 번들에 넣지 않습니다. 버전별 다시 그리기·얼굴 보정·당시 한계는 [v0.1.6](releases/v0.1.6.md)·[v0.1.9](releases/v0.1.9.md)·[v0.1.10](releases/v0.1.10.md)을 참고하세요.

## 웹사이트 (GitHub Pages)

현재 사이트의 제품 시각 자료는 정적 이미지이며 로컬 Pages 작업에 동영상·MP4 자산이 필요하지 않습니다.

캡처를 게시하기 전에 화면 내용을 직접 확인하세요. 격리된 시연 프로필에서 앱만 담고 실제 세션 내용·계정명·로컬 경로·호스트명·이메일·인증 정보를 포함하지 마세요. 공개 PNG에는 이미지·색상 청크만 허용하며 `check`와 `stage`는 EXIF·텍스트·프로필 등 메타데이터를 거부합니다. 이 검사는 화면에 보이는 글자를 판별하지 않으므로 개인정보 수동 검토도 반드시 필요합니다.

한국어 기본 홍보 웹사이트는 [`site/`](../site/)에 있습니다. 네이티브 macOS 앱을 설명하는 사이트이며 브라우저에서 앱을 실행하지 않습니다. Pages 주소: [한국어](https://hanbong5938.github.io/herdr-desktop-pet/) · [영어](https://hanbong5938.github.io/herdr-desktop-pet/en/) · [중국어 간체](https://hanbong5938.github.io/herdr-desktop-pet/zh-Hans/) · [일본어](https://hanbong5938.github.io/herdr-desktop-pet/ja/). 제품 문구를 바꿀 때 네 페이지의 번역과 언어 전환 링크를 함께 갱신하고 `ko`·`en`·`zh-Hans`·`ja`의 상호 `hreflang` 및 한국어 루트를 가리키는 `x-default`를 유지하세요. 각 페이지의 canonical URL과 Open Graph 언어도 해당 페이지에 맞춰야 합니다. 외부 [캐릭터 갤러리](https://github.com/hanbong5938/herdr-characters)는 별도로 유지되며 한국어·영어만 제공합니다. 과거 v0.3.2 스크린샷을 최신 화면이나 번역된 스크린샷처럼 소개하지 마세요. 영어 소셜 미리보기 이미지를 공유해도 페이지 언어는 바뀌지 않습니다. 제목은 화면 폭에 맞춘 간결한 크기와 좁은 주변 여백을 사용하며 본문·터미널 명령어·버튼 크기는 제목과 독립적으로 유지합니다. 저장소 루트에서 실행하세요.

```sh
bun test tools/pages.test.ts
node tools/pages.mjs check
node tools/pages.mjs serve --port 4173
# http://127.0.0.1:4173/herdr-desktop-pet/ 접속 (영어: /herdr-desktop-pet/en/; 중국어 간체: /herdr-desktop-pet/zh-Hans/; 일본어: /herdr-desktop-pet/ja/)
node tools/pages.mjs stage /tmp/herdr-pages
```

사이트 CLI에는 **Node.js 22**를 사용하고 **Bun 1.4.2**는 실제 Node 서버를 실행하는 HTTP 회귀 테스트의 러너로만 사용합니다. CI는 staging 전에 이 회귀 테스트를 실행합니다. `check`는 내부 링크·앵커·언어별 메타데이터·에셋을 확인하며 외부 링크를 네트워크로 검사하지 않습니다. 로컬 미리보기는 공개 파일 중 일반 파일만 제공합니다. 최종 `404.html` 대체 파일이 없거나 심볼릭 링크·일반 파일이 아니라면 파일을 전송하는 대신 `nosniff`가 있는 일반 텍스트 `500`을 반환하며 `HEAD`에는 본문이 없습니다. `stage`는 먼저 검사한 뒤 저장소 **밖의 새 디렉터리 또는 빈 디렉터리**에 공개할 `site/` 파일만 복사하며 배포하지 않습니다. Pages 워크플로는 PR을 검사하고 검토용 아티팩트만 첨부하며 배포하지 않습니다. Pages 배포 소스는 **GitHub Actions**이고, 웹사이트 변경을 `main`에 병합하거나 `main`에서 워크플로를 수동 실행했을 때 조건을 만족하는 `main` 실행만 배포합니다. 이 로컬 명령은 릴리스 바이너리·Wiki·저장소 Pages 설정을 바꾸지 않습니다.
