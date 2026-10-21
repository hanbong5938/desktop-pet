# Desktop Pet

## rig CPU 및 숨김 host 검사

beta.9의 rig 변경은 표시 60 Hz를 유지하고 active-hidden의 지속적인 surface 작업만 중단합니다. 후보 준비·명시적 preview·일회성 모델/복구 준비는 허용하고 말풍선/provider는 독립적으로 동작합니다. Hair powers는 Float 양자화와 논리 quota를 유지하며 순증가 payload 상한은 모델당 640 KiB입니다(array/allocator 제외).

수정 전 실제 소스 바이트를 보존하세요. CPU runner는 양쪽에 동일한 현재 production consumer·생성 limits·컴파일러·SDK·release flags를 사용합니다. 모든 정점/alpha/hit bit pattern의 정확한 일치와 의미/자원 회귀를 검사하고 12쌍 교대 벤치마크·checksum·생성 시간·별도 process RSS를 기록합니다. 기준선이 없으면 회귀만 검사하고 성능 개선을 주장하지 않습니다.

```sh
node scripts/check-rig-deformation.mjs --baseline-source "$BASELINE_SOURCE" --output "$WORK/rig-cpu"
node scripts/check-rig-host-visibility.mjs --source "$PWD" \
  --runtime "$PWD/native/target/neutral/rig-native/release" \
  --assets "$PWD/assets/rubelia-default" --expect hidden-stopped --output "$WORK/rig-host"
```

runtime을 먼저 빌드합니다. 절대 경로와 새/빈 출력 디렉터리만 허용합니다. host runner는 서명 worker/resources를 복사하고 실제 AppKit/Metal로 초기 준비·동기화한 숨김 경합·재표시·preview·복구·취소·catalog 전환을 검사합니다. `--expect existing-hidden-render`와 `--source "$BASELINE_SOURCE"`로 옛 동작을 재현합니다. GUI/Metal/drawable 실패는 skip-success가 아니라 명시적 실패입니다. `RIG_HOST_VISIBILITY_CHECKS` 계측은 shipping binary에 없습니다. [beta.9 범위와 검증 한계](releases/v0.4.13-beta.9.md)를 참고하세요.


**현재 후속판: Core v0.4.12 / genuine bridge v0.3.16 — 아직 검증·공개되지 않았습니다.** Core v0.4.11은 release `414563938`, 공개 시각 `2026-10-16T05:16:03Z`, source `7d5215f4b4a2442d18438b453cc5afae14c2f4a0`으로 공개되었고 6개 공개 자산을 독립 검증했습니다. 별도 Wiki 작업은 `WIKI_PUBLISH_TOKEN`이 없었습니다. Genuine bridge v0.3.15 CI `38855045701`은 성공했지만 태그·릴리스는 공개되지 않았습니다. 공개 출처 검증은 과거 이관 복구 수락 검증이 아닙니다. See [current successor ledger](releases/v0.4.12.md).

**과거 시작 결함과 영구 수정:** 소유 VM의 실제 공개 v0.4.11 과거 작업 실행은 `Migration history: current native daemon does not prove the migrated image/profile readiness`로 실패했습니다. 새 receipt·SDK 전달은 없었고 SDK 0.1은 `/opt/homebrew/bin/herdr-desktop-pet` 누락 오류를 유지했습니다. UI Ready 전 관찰 오류가 보존 작업을 잃게 했습니다. v0.4.12는 읽기 전용 discovery로 Ready 전에 작업을 보존하고 기존의 승인 검증된 observation/poll이 나중에 전달합니다. 현재 image/owner/profile·signed-stage·정당한 runtime 시작·Stop·Unknown guard를 유지하며 옛 manager 재실행·plan/journal 재작성·수동 재연결은 하지 않습니다. 대상 소스 회귀 검증 통과는 end-to-end 성공이 아닙니다.

**수락 계약:** 최초 Update 승인 한 번으로 조용한 이관·서명 SDK 교체/로드·정확한 저장 route 전환·같은 Tern 창의 실제 fresh binding·자동 native receipt 뒤에만 승인된 옛 소유자를 폐기합니다. 두 번째 동의·Configure·Connect·ACK·Resume은 없습니다. 각 원래 config/state profile·캐릭터·대사/이력·계정/provider·무관 profile과 Stop·Unknown을 보존합니다. 로컬 7c signed-out GUI/frame-driven sequence 47 및 재시작 sequence 13은 component 증거일 뿐 quiet/authenticated 전달·전체 lifecycle·다른 컴퓨터 복구 증거가 아닙니다.

**Historical records below:** earlier “current” publication, selector and acceptance statements retain their named release scope; they are not v0.4.12 / v0.3.16 qualification. Consumer metadata stays Core v0.4.8 pending actual public materialization; Tern production remains frozen at `69696023e00a0fcda8b85b332a075bfa26d6c26d`.

독립적인 네이티브 macOS 데스크톱 캐릭터입니다. 루벨리아는 터치에 반응하고 Herdr·Tern 등 등록된 제공자의 세션을 표시하며 해당 출처가 허용한 경우 답장을 보냅니다. 호스트 없이 캐릭터만 실행할 수도 있습니다.

[English](../readme.md) · [한국어](../readme.ko.md) · [日本語](../readme.ja.md) · [简体中文](../readme.zh-Hans.md)

## 현재 검증·게시 경계 — 2026-10-15

현재 관측과 미실행 항목의 정본은 [Core v0.4.7 수락 기록](releases/v0.4.7.md)입니다. 불변 [Core v0.4.7](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.7)은 release `413584934`, 게시 `2026-10-15T08:39:17Z`, 태그 역참조 소스 `52514da5115f09aac22de5429c08f7acdd993258`입니다. [Release CI 38706583599](https://github.com/hanbong5938/desktop-pet/actions/runs/38706583599) / job `116327375871` 통과. 익명 자산 6개·strict/deep ad-hoc 서명·일치 protocol 4 / observation 2 / API 1·정확한 68개 리소스 closure 통과. Archive SHA-256 `f89ea6c52197bd4c7fb70336846099f1e5eb553f2de9bb4cd43b14b1134da8fa`, installer `f64a27593058a68c5a5e112f448be644c6abfbb760be109acf992e2f415f5829`.

Genuine [bridge v0.3.11](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.11)은 같은 정확한 Core 소스로 빌드한 원래 legacy 이름·protocol 3 / observation 1 / API 1 앱/도우미입니다. Release `413623057`, 게시 `2026-10-15T09:25:08Z`, annotated tag object `a907424e89c10d484f8646f41d5f020fa93ba8bc`, 역참조 metadata-only ingress commit `fa56caa32ea06d86932c5b4cc3eb230010c6dd9c`. [Bridge CI 38707461727](https://github.com/hanbong5938/desktop-pet/actions/runs/38707461727) / job `116330312000`은 실제 논리 `TMPDIR=/tmp` native 검증·생성 앱 Git admission 통과. 공개 자산 5개는 CI 바이트와 일치하며 서명·11개 소스 파일 closure·배포 파일 68개 통과. Archive SHA-256 `2075baefbf212f5cbfbdc0163ce0e6887fa6d05f20e2a24c47ae24012649ab60`.

공개 tap `6693cd9db26e6dba9a257bbe76acaf78cb502c2f`는 Core4.7/bridge3.11을 제공하며 beta는 불변입니다. 실제 공개 Brew3.10→일반3.11 A′는 원래 helper로 정착했고, 독립 동의 공개 B4.7은 native readiness/Commit·실제 소유 Brew retirement를 완료했습니다. 동결3.6 compatibility·beta2→일치3.6 경로는 별개입니다.

정확한 태그52514da의 neutral768/1 ignored·genuine legacy788/1 ignored, 현재 Bun62/746 expectations·TypeScript/format/release-docs·consumer adapter104개가 통과했습니다. 실제 공개 B에서 원래 프로필 바이트·가져온 캐릭터·대사·이력·무관 프로필을 보존했습니다. 4개 언어 review/Cancel, rollback, early/late Stop, ManagerUnknown 보존, 확정 실패 후 독립 동의 same-plan Resume도 통과했습니다. 정확한 범위와 보존한 v0.4.6 false-Unknown 실패는 [정본 원장](releases/v0.4.7.md)을 따르며 Unknown을 clear/replay하지 않습니다.

현재 Herdr scripts/evidence 8개는 공개 Core4.7에서 materialize했고 같은 이전 완료 공개 앱·프로필을 설치·시작·Herdr 재설치 없이 재사용했습니다. 자산 없는 Herdr4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`은 Herdr-managed retirement 전용이며 새 연동 태그·native build는 없습니다. 실제 Mac native 승인과 main/Pages 제공 관측은 원장에서 별도 기록합니다. H1/T0는 실제 Tern-account/양쪽 순서 증거가 아닙니다.

Wiki job `116329481248`은 render 후 전용 `WIKI_PUBLISH_TOKEN` 부재로 실패했으며 broad credential 대체·binary 재실행은 없습니다. 소스·CI·게시·설치 인수·브라우저로 관측한 Pages 배포는 서로 다른 증거입니다.

## v0.4.5 / bridge v0.3.10까지의 과거 검증 기록

이 절의 “현재/실제/이제” 표현은 당시 버전·날짜에만 해당합니다. 옛 PID·digest·Unknown은 보존하며 v0.4.7 / v0.3.11 수락이나 오늘의 배포 pin으로 해석하지 않습니다.

**현재 공개 Core:** [Desktop Pet v0.4.5](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.5), release `412667385`, `2026-10-14T12:01:32Z`, source `51dcf22415b1ff0faf07651cb83c1c696c980f43`. 익명 자산 6개·strict 서명·protocol 4 / capability 2 / API 1·정확한 68개 리소스 해시 closure가 통과했습니다. Archive SHA-256 `41024ed9d82e5be7169c166bcd6e443c319f6b8916cad63b8399cd4ad02bb45a`; installer SHA-256 `fdcd9d80f8672b54ceca967bfea6f72223d2e5caed97f845140a68b9e7414a16`; checksums SHA-256 `8c9c8ab020bc4561c4a2c17f4c6d20b3e8a18672b84f508426e68ed259598c5e`. [Release CI 38560075408](https://github.com/hanbong5938/desktop-pet/actions/runs/38560075408), job `115838673595`: PASSED. Exact-source gates: neutral **743** primary Rust / 1 ignored; genuine legacy **758** primary Rust / 1 ignored; 62 Bun / 746 assertions / TS / fmt / locked tree / full package / 68-resource closure; genuine native `TMPDIR=/tmp` and real Git admission/dirty-user guard. Neutral formula tap `d66411383141644f01f1ca839086b6d505a997ac`. Wiki job `115840956329`의 render는 성공했지만 게시가 실패했습니다. 소유자의 Wiki Home 초기화와 전용 `WIKI_PUBLISH_TOKEN`이 여전히 필요하며 broad token fallback·binary 재실행은 없습니다. Pages 배포·공개 네 언어 render는 별도 관측 검증이 필요하며 릴리스/native 수락은 배포 증거가 아닙니다.

**실제 공개 Brew v0.4.4 → v0.4.5 일반 업데이트 완료:** operation `a6991e69233377c32b150186c62888fd`, `homebrew_upgrade` / `manager_exited`, installed/applied true. PID `10377` / instance `37953e017fc9208bfcc785a4ff138494`, Brew 파생 image `a06f7e7045a1cfa83a3d7ed7d7a135eab4f8d911a3275647b89743fe5815c500`이 정확한 옛 private profile·저장된 Herdr(H1/T0)로 UI/control ready에 도달했습니다. 업데이트 후 native Check는 녹색 **No update from selected source / 0.4.5 / Homebrew**입니다. 적용 전 Check는 source preferences만 변경했고, 적용은 `source.log`와 새 operation 파일 외 모든 옛 config/evidence·무관한 기본 tree를 보존했습니다. 무관한 과거 hidden PID `2641`은 옛 A/B 증거를 봉인한 뒤에만 명시 Stop했습니다. Herdr 재설치는 없습니다. Actual manager `/opt/homebrew/bin/brew`: PID = PGID `9853`, exit 0, drained true; retained helper strict-signature result 0, stdout upgraded only desktop-pet v0.4.4 → v0.4.5.

**수동 소유자 복구(자가 업데이트 아님):** 차단된 공개 v0.4.3 checker는 스스로 업데이트하지 않았습니다. 서명된 원래 앱 백업·명시 Stop `9340`·고정 공개 v0.4.5 prebuilt 설치·자동 시작 없음 검사·명시 Start 후 실제 PID `13055` / instance `93f6048e2ae1266c9d5a42ede0f2da51`이 원래 profile·저장 Herdr H0/T0·정확한 공개 native image `693bad6245d28be4350b661f018e34416ca950b37f49a6a4a3967c5d1a67eacb`로 ready입니다. Start transport는 20초 timeout 후 재시도하지 않았고 이후 실제 GUI/readiness를 관측했습니다. daemon log 외 모든 옛 config·history 321개 파일은 보존됐습니다. 최종 native GUI Check는 녹색 **No update from selected source / 0.4.5 / Standalone installation**이며 원래 v0.4.2 history를 유지했습니다. 최종 Check는 예상 preferences만 바꿨으며 Start 재시도·Unknown journal replay는 없습니다.

**실제 새 공개 managed v0.3.6 → v0.3.10 A → v0.4.5 B: COMPLETE.** Genuine private Herdr 0.9.3 / protocol 22로 정확한 동결 v0.3.6 pair를 설치했습니다. Native Check는 source preference check metadata만 바꾸고 설치/process·다른 네 tree bytes는 유지했습니다. 별도 A 동의로 operation `20cca91548584437af112bb04cc260b0` 완료; 보존한 서명 v0.3.6 helper는 B 후에도 completed / manager_exited / installed=true / applied=true입니다. 정확한 공개 bridge v0.3.10 PID `2894` / instance `8f8f97c36aed6625166a679396596a66`, raw image `0b1c8e63c6cee6904208bc3d90f184806c8f1216b8114a2b30da009d92be65a6`, protocol 3 / observation 1 / API 1·native UI/control ready·Herdr 1 / Tern 0에 도달했습니다. Bridge Check로 A를 정리하고 녹색 **No update from selected source / 0.3.10 / Herdr plugin manager**를 관측했습니다.

별도 Move preview·strict/deep 검증 공개 v0.4.5 protocol-4 native **Migrate** 동의는 B `6ee850aa5577314c6d9cf6ebc7eae981`를 한 번 승인했습니다(plan SHA-256 `b6fe929da173e648fd2ad8e2845b7fda00461e3e6002e7bb9113155d403127de`). Durable phase=complete, native_ready=true, target_committed=true, error=null. Source `2894`의 Stop 확인 후 Core v0.4.5 PID `3594` / instance `2ee0995b8b3dc3abe18e6238a91929d9`은 같은 원래 private config/state·저장 Herdr·Herdr 1 / Tern 0으로 실제 UI/control ready입니다. 적격 Brew image `a06f7e7045a1cfa83a3d7ed7d7a135eab4f8d911a3275647b89743fe5815c500`를 재사용했고 `target_manager:null`이므로 중립 install/upgrade manager는 없습니다. Genuine Herdr retirement PID = PGID `3648`, exit 0, quiescent=true로 불변 integration v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`를 설치했습니다. Enabled registry·clean checkout은 정확한 ref이며 actions 21개/startup/client.attached가 있고 옛 생성 app은 없으며 새 integration tag는 없습니다. 네 봉인 tree의 모든 기존 일반 파일(source config 5/state 164; 무관한 기본 config 5/state 2)·**원래 A 파일 87개**·helper/plan/records는 B 후 byte-identical이며 **옛 daemon-log 예외도 없습니다**. 새 B 파일만 추가했습니다. 무관한 기본 PID `1971` / instance `27752246b4da5c6f3964a74dd22881a2`는 running+Hidden·Auto/H0/T0 그대로이며 Stop/restart/merge가 없습니다. Adoption은 private XDG namespace 안에만 있습니다. B 후 native settings는 exit 0·같은 ready instance입니다. 실제 이관 PID `3594`의 native B 후 Check는 녹색 **No update from selected source / 0.4.5 / Homebrew**이며 migration/legacy retirement `6ee850aa5577314c6d9cf6ebc7eae981` 완료와 함께 확인됐습니다. Source `preferences.json` check metadata만 변경됐으며 원래 A 파일 87개·모든 B proof를 포함한 현재 state 파일 239개와 무관한 기본 tree는 byte-identical입니다. Pages 배포·공개 네 언어 render는 별도 관측 검증이 필요하며 native 수락/source 편집은 배포 증거가 아닙니다. 아래 v0.3.9/v0.4.4 operation은 과거 증거입니다.

실제 고정 beta route는 **공개 v0.3.9 entrypoint에서 PASSED**: 원래 protocol-2 archive `d49824778dc0d898b4386cc04a4fbed34da40ebfe990148441d365984555d1aa`, live/direct-bridge guard·동결 일치 v0.3.6 pair·자동 시작 없음·명시 ready Start·서명 옛 helper 백업; non-app Git 38개 파일·user edits·profile은 보존됐습니다. v0.3.10 beta entrypoint 실행은 아닙니다. history 검증은 private 읽기 전용 구조 검사와 정확한 현재 strict fresh read/write/execute/startup/resume authority를 분리합니다. 수정 전 실패한 회귀 2개·coordinator test 70개가 통과했고 실제 v0.4.5 읽기 전용 옛 history probe에서 config·state 321개 해시는 불변입니다. 옛 Unknown은 원래 scope/evidence에 묶인 읽기 전용이며 clear·replay·promote하지 않습니다.

**실제 새 공개 Brew v0.4.5 설치 통과:** 설치 전 모든 중립 app/config/state/process root 부재를 기록했습니다(74개 파일 / 44.3 MB, 내부·외부 재서명 5개, 다른 formula 업그레이드 없음). 적격 읽기 전용 helper discover는 정확한 Brew 파생 image `a06f7e7045a1cfa83a3d7ed7d7a135eab4f8d911a3275647b89743fe5815c500`을 검증했고 설치·검사는 중립 config/state·standalone app·process를 만들지 않았습니다. **명시 Start 이후에만** 기본 PID `1971` / instance `27752246b4da5c6f3964a74dd22881a2`, v0.4.5 / protocol 4 / capability 2 / API 1·Auto / H0/T0·실제 UI/control ready에 도달했습니다. 실제 Settings는 Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0·내장 Rubelia를 표시했습니다. 명시 Hide는 같은 기본 instance를 hidden·running으로 유지하며 완료한 새 A/B와 무관합니다. 새 설치는 managed A/B 수락이 아닙니다.

**과거 v0.4.4 새 Brew 설치 증거:** 중립 v0.4.4 포뮬러는 tap `2aa9a4be9517e0232a3f2b8e2b3c748045c69586`에 공개됐습니다. 적격 Brew v0.4.4 새 설치가 통과했습니다(74개 파일 / 44.3 MB, 내부·외부 재서명 5개). 인증된 검사로 Homebrew 파생 이미지 `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312`를 확인했습니다. 설치·검사는 중립 config/state·standalone 앱을 만들거나 데몬을 시작하지 않았습니다. 명시적 수동 **Start** 이후에만 실제 v0.4.4 UI/control ready, PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470`가 확인됐습니다. 루벨리아와 실제 Settings는 **Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0**을 표시했고 이후 명시적 Hidden은 같은 기본 인스턴스를 보존했습니다. 새 설치의 증거이지 v0.4.4 일반 업데이트·managed 이전 완료 증거는 아닙니다.

**현재 공개 bridge:** [Herdr bridge v0.3.10](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.10), release `412684474`, `2026-10-14T12:20:45Z`, source `67eb44b53bda6ba96eaf68671b67a94cab7b304d`, common Core `51dcf22415b1ff0faf07651cb83c1c696c980f43`. Genuine legacy protocol 3 / capability 1 / API 1, original bundle/executable/helper; five anonymous public assets, strict signatures and exact 68-resource closure: PASSED. Archive SHA-256 `a75f4219f1d8bc7ef8c3bee29c030b0cd6c06853bab4e193e40838ef5d49b927`; rig SHA-256 `b3a5cded2b80e5f773fbcd3aa49d1e7d8d0868b27e9a2e872d03f3752cadc5a7`; raw native SHA-256 `0b1c8e63c6cee6904208bc3d90f184806c8f1216b8114a2b30da009d92be65a6`. [Bridge CI 38561115955](https://github.com/hanbong5938/desktop-pet/actions/runs/38561115955), job `115842136732`: PASSED, native `/tmp` and real Git gates. Source-only publication gate: exactly 11 regular UTF-8 metadata files, nine exact materializer bytes plus LICENSE/readme, only `/dist/HerdrDesktopPet.app/` ignore; no native/common implementation. Old stable formula tap `7bf1fd7f1b89bd77e68179fcee95cacf044927dc`; beta unchanged. Herdr v0.4.1 retirement ref `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8` unchanged.

**과거 v0.3.9/v0.4.4 managed A 완료:** 동결 v0.3.6 Check는 공개 v0.3.9를 선택하고 `preferences/last_update_check`만 바꿨습니다(`1791963743` → `1791965310`). Check 중 다른 네 tree bytes·source PID `2467`·무관한 기본 PID `2641`은 불변입니다. 정확한 source `0f33f28` native A 동의로 operation `d5b2bf6f02e031eabb5a8c8fdd805cab`가 완료됐습니다(`completed` / `manager_exited`, installed/applied true). 실제 공개 v0.3.9 native PID `3417` / instance `b42b25d48315e66bd87e7685e58cfb14`, image `0ed12d159b6bbb83ada833d39b4524ac5f7eccb1253b7acaf9aa2b71cbf833ce`는 정확한 옛 private config/state에서 일치 protocol 3 / capability 1 / API 1·UI/control ready·Herdr 1 / Tern 0에 도달했습니다. 실제 managed source Git status는 완전히 clean입니다. 보존한 원래 v0.3.6 helper의 strict/deep 서명과 실제 completed status도 통과했습니다. baseline은 옛 A 증거 87개와 전체 state 파일 91개를 해시로 보존합니다. 

**과거 v0.3.9/v0.4.4 managed B 완료:** 별도 Move preview와 독립 strict/deep 검증을 통과한 공개 v0.4.4 coordinator가 native **Migrate** 동의를 받아 operation `d729fd740f4e459945edda00bf48d944`, plan SHA-256 `6471e928254229a66a6fc64a88f67095777d6bd890d621feb36b08a4c7d1fc69`으로 **COMPLETE**에 도달했습니다. Native Prepare/Stop으로 공개 v0.3.9 PID `3417` / instance `b42b25d48315e66bd87e7685e58cfb14`를 종료하고 적격 기존 Brew v0.4.4 app root/image `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312`를 사용했습니다. `target_manager:null`이므로 **재설치가 없었습니다**. Target PID `4104` / instance `8912b3aad5419b1c14a78ef3418ae36f`는 정확한 옛 private config/state와 저장된 Herdr 선택(Herdr 1 / Tern 0)을 유지하며 protocol 4 / capability 2 / API 1·실제 UI/control ready에 도달했고 native_ready·target_committed가 true입니다. 실제 Herdr retirement attempt 1은 PID = PGID `4140`, exit 0, quiescent true, error null입니다. 실제 plugin list는 불변 Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`의 enabled integration-only Desktop Pet을 표시했고 managed Git HEAD도 일치합니다. 실제 manager 출력은 v0.4.1·actions 21개·startup/client-attached 훅입니다. Managed dist/app 및 옛 PID `3417`은 사라졌고 standalone `DesktopPet.app`은 만들지 않았습니다. 정확한 retired v0.4.1 태그의 역사적 Core v0.4.2-minimum wrapper는 같은 v0.4.4 PID/instance를 실제 재사용했습니다. 새 앱·새 태그는 없습니다. 원래 A 증거 87개 SHA-256·기존 옛 source-config 해시는 모두 불변이고 무관한 기본 config/state 두 tree는 byte-identical입니다. 숨겨진 기본 v0.4.4 PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470`은 변경 없이 실행 중입니다. Private XDG base profile만 adopt했고 global default와 병합하지 않았습니다. 실제 GUI는 Desktop Pet / 내장 루벨리아·Settings **Herdr Connected 1 / Tern Disabled 0**으로 시각 확인했으며 clean-image 캡처는 아직 대기 중입니다. Fault 없는 최종 공개 v0.3.9/v0.4.4 A/B happy path는 별도로 통과한 과거 working metadata `f7edf5e` definite failure/명시 Resume 검증과 구분합니다.

**consumer·SDK 경계:** Herdr 연동은 공개 v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`(자산 없는 release `412252863`)을 유지합니다. 역사적 Core v0.4.2 minimum은 동결 상태이고 정상 Core v0.4.3 재사용은 관측됐습니다. 현재 Herdr consumer scripts 및 metadata/evidence 파일 8개는 공개 Core v0.4.5 release `412667385` / 소스 `51dcf22415b1ff0faf07651cb83c1c696c980f43`에서 실제 materialize됐습니다. 새 Herdr 태그는 없으며 최종 공개 managed retirement와 불변 wrapper의 v0.4.4 재사용은 위 실제 B 결과로 별도 확인했습니다. Tern v0.2.0과 호환 Core v0.4.1 pin은 불변입니다. Tern은 검증된 실행 파일과 실제 config/state 쌍을 명시적 **Configure → Save → Connect**로 연결하며 앱을 설치·자동 시작하지 않습니다. 실제 Tern keychain/provider 로그인 및 genuine OMP 18.10.3 providerSignIn/NoModel은 양쪽 시작 순서의 동일 instance/prompt/transcript/reconnect 수락을 여전히 막습니다. 자격 증명 복사·가짜 계정·ACK·완료를 만들지 않습니다. **전체 matrix는 완료되지 않았습니다.** 실제 현재 Herdr v0.4.5 reviewed-consumer VM smoke 통과: materialized wrapper는 같은 Brew v0.4.5 PID `10377` / instance `37953e017fc9208bfcc785a4ff138494`을 발견했습니다. Hide 후 ensure는 bootstrap 없이 Hidden / visible false·같은 instance를 유지했고 명시 Show로 복원했습니다. 현재 Herdr source 검증 통과: integration-only ownership v0.4.1·shell/manifests/TypeScript·고정 contracts `85c644cd90d8c6a9e97e9dc313cc4750f344235f`·**Rust adapter test 104개와 포함 test의 child 재실행 1개**(서로 다른 105개 test·Bun 아님). Native app/release 빌드는 없습니다.

**과거 v0.4.2 / v0.3.7 근거(v0.4.4 실행 증거 아님):** **게시 경계:** [Desktop Pet v0.4.2](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.2)은 공개되었습니다(release `412241744`, `2026-10-14T01:09:50Z`, 불변 소스 `cb10ce2c3cc65402cbb914d60301f7627d5546d5`). 익명 자산 6개, strict/deep 서명, protocol 4 / capability 2 / integration API 1, 68개 배포 리소스 해시·정확한 closure를 검증했습니다. tap `32919c745864b54a2e716fcc5a3714d0d38d3e84`의 중립 포뮬러와 실제 공개 v0.4.1 → v0.4.2 Brew 업그레이드는 receipt/derivation·서명 5개·`brew test`·Auto 네이티브 UI/control ready(Herdr 0 / Tern 0)를 입증했습니다. 새 공개 standalone v0.4.1 설치 및 동시 설치기 2개의 실행 중 소유자 재사용을 확인했으며, 네이티브 Check가 실제 공개 v0.4.2를 제시하고 동의 후 `standalone_replace`·`manager_exited`·`installed:true`·`applied:true`, 같은 config/state·정확한 공개 v0.4.2 이미지의 준비 완료를 확인했습니다. Check는 locator/profile/lifecycle/locks/PID를 유지했지만 `preferences.json` 확인 캐시는 바꿨으므로 모든 설정 바이트 불변은 아닙니다. [수정 integration-only Herdr v0.4.1](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.4.1)도 공개되었습니다(release `412252863`, 불변 ref `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`). 공개 wrapper 2개가 숨김을 유지하며 같은 Brew v0.4.2 PID/instance/image를 재사용했고, 명시적 config/state와 실제 private patched Herdr 0.9.3 socket으로 Auto Herdr 1 / Tern 0에 도달했습니다. 새 환경의 공개 bootstrap 2개는 OS-home 앱 하나를 설치/재사용했으며 연동-local native 파일이나 데몬 시작은 없었습니다. 이는 공개 Herdr manager 설치 receipt 증거가 아닙니다. 원래 Herdr v0.4.0의 생성 앱·실패 테스트 scaffold symlink 소스 결함은 보존하며 integration-only였다고 주장하지 않습니다. [Bridge v0.3.7](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.7)도 공개되었습니다(release `412267469`, 불변 tag `987a4b859f95e922c1c5443ead372d10d0dddb24`). 정확한 Core `cb10ce2`·수정 Herdr `6d2ac04`에서 빌드했고 익명 자산 5개·서명·68개 파일 closure를 검증했습니다. tap `657f32cc4cc563ae40ae0432ac5123dcb3f58426`에서 old stable 포뮬러만 v0.3.7로 변경했으며 beta는 그대로입니다. manual-origin private 실제 호스트 실행은 Herdr 1 / Tern 0 ready를 입증했지만 managed A→B 증거는 아닙니다. 실제 동결 공개 Brew v0.3.6 → v0.3.7 A `6a10663c2d2f0177990623a969238af4`와 별도 동의 Core v0.4.2 B `c66961f9e5a91c0023545065ae61385b`는 실제 Brew retirement(exit 0, quiescent)까지 완료했습니다. B 전후 모든 설정 파일 해시와 이전 A 증거 87개 파일은 불변이며 실제 Core v0.4.2 UI는 Desktop Pet / Herdr Waiting 0 / Tern Disabled 0입니다. 반면 실제 managed v0.3.7 A `a1aec5902020de7bcaaf583b1a5f50a5`는 native `/tmp` 프로필 검증에 실패했습니다. checkout 교체·registry 등록 전 manager exit 1이 발생했고 동결 helper는 Unknown / `manager_exited`, installed/applied false를 보존합니다. VM·journal·reservation은 recovery/replay 없이 유지합니다.

**과거 공개 v0.3.8 managed 결과·안전한 successor 경계:** managed v0.3.6 → 공개 v0.3.8 A는 완료됐고(`16b6307413ec409efe3eb5a04c9cc21f`) 원래 서명된 v0.3.6 helper가 completed status를 확인했습니다. 명시적 Check 이후 B preflight는 `managed source changed; migration never resets user changes`로 거부됐습니다. `dist/HerdrDesktopPet.app`의 생성 파일 정확히 68개가 untracked였으며 B 동의·Stop 전이므로 B manager·fault flag는 없고 당시 source PID `3563`은 불변이었습니다. 원래 공개 v0.3.8 태그는 일반 UTF-8 Git 파일 10개이고 `.gitignore`가 없습니다. 이 B 제한은 역사적 사실이며 공개 v0.3.9가 과거 태그를 고치지 않습니다. 수동 ignore·`git clean`·reset으로 옛 태그를 통과시키지 마세요. 이미 managed v0.3.8 A를 완료한 사용자는 보존된 **일치하는 서명 helper**로 알려진 terminal 옛 operation을 확인하고 실제 config/state·옛 증거를 백업한 뒤 source를 명시적으로 **Stop**하고 **실제 소유 Herdr manager**로 successor를 명시 설치해야 합니다. Unknown manager operation은 절대 replay하지 말고 VM·journal·reservation을 보존합니다.

**과거 working-metadata·일반 업데이트 증거(최종 v0.3.9/v0.4.4 수락 아님):** 실제 managed B는 working metadata `f7edf5e0eba374248b9a5ee9d66d26109b39eca2`·불변 공개 v0.3.8 native·공개 Core v0.4.3으로 NativeReady/Commit 이후 definite retirement failure(manager exit 1, quiescent)에 도달했습니다. 도입한 소유 `uchg` flag만 제거하고 같은 plan SHA `3615fa140fd1b0d49910f5f2ef170b64122243c6c65d0098d4ecfd729e951f16`에 독립 명시 Resume 동의를 받아 attempt 2가 완료됐습니다(실제 CLI exit 0, quiescent). 옛 managed app/source를 retirement했고 Herdr v0.4.1 `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`은 같은 v0.4.3 instance를 재사용했습니다. 옛 A 증거 87개·first-attempt/plan/consent 5개·옛 config·무관한 기본 프로필 해시는 불변입니다. 실제 공개 중립 v0.4.2 → v0.4.3 일반 업데이트도 완료됐습니다(`81c82d837b65fb03584bfb1e668119c3`, `standalone_replace`). 같은 프로필/Herdr 0 ready·루벨리아 UI를 확인했고 Herdr 재설치는 없었습니다. Check는 `last_update_check`만 바꿨고 Check 이후 모든 config bytes와 옛 A 증거 87개는 업데이트 전후 불변입니다. 이는 최종 공개 v0.3.9 A/B나 v0.4.4 업데이트 증거가 아닙니다.


## 요구 사항

Apple Silicon Mac, macOS 13 이상. 소스 빌드는 Rust/Cargo·Bun·Node.js/npm·Xcode Command Line Tools가 필요하며 사전 빌드 설치에는 필요 없습니다. 앱은 ad-hoc 서명·미공증입니다. Herdr 스냅샷에는 호환 로컬 서버(0.9.0 이상), 답장에는 별도 `agent.prompt`가 필요합니다. 선택형 client-attach 자동 시작에는 공식 Herdr 0.9.3에 [연동 저장소의 패치와 호스트 빌드 절차](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)를 적용하거나 실제 훅 지원 호스트를 사용해야 합니다. 순정 0.9.0/0.9.3에는 훅이 없고 플러그인 설치는 호스트를 패치하지 않습니다.

## 설치와 선택형 연동

**현재 소스 설치 계약:** Standalone과 현재 consumer fallback은 `HOME`/XDG가 아닌 실제 OS home의 `.local/share/desktop-pet/libexec/DesktopPet.app`입니다. Brew는 검증된 formula prefix의 `libexec/DesktopPet.app`, source는 정확한 checkout의 `dist/DesktopPet.app`을 유지합니다. 일반 설치는 기존 Applications 소유자를 이동하지 않습니다. 공개 설치기·동결 노트는 각 버전의 기록이며 옛 설치기 재실행으로 잘못된 목적지가 복구되지 않습니다. 일회성 복구는 **Bash로 비공개 제공**하며 배포 제품 기능·Rust runtime/route API가 아닙니다. 저장된 Tern KV의 정확한 원래 프로필과 원본 데이터·백업·실행 중 Tern 작업을 보존하세요. [비공개 복구 안내](usage.ko.md#일회성-applications-복구)를 보세요. 이전 공개 Python 복구 자산은 철회됐으며 동결 beta.8 노트는 과거 기록이지 현재 실행 안내가 아닙니다. 새 릴리스·버전 변경은 없습니다.

### 과거 고정 설치 예시

**과거 v0.4.4 새 Brew 설치 증거:** 중립 v0.4.4 포뮬러는 tap `2aa9a4be9517e0232a3f2b8e2b3c748045c69586`에 공개됐습니다. 적격 Brew v0.4.4 새 설치가 통과했습니다(74개 파일 / 44.3 MB, 내부·외부 재서명 5개). 인증된 검사로 Homebrew 파생 이미지 `8ec4308c59c874140c431a2f86ad3d0a1f13a8365bdd994e9da14d7788e5c312`를 확인했습니다. 설치·검사는 중립 config/state·standalone 앱을 만들거나 데몬을 시작하지 않았습니다. 명시적 수동 **Start** 이후에만 실제 v0.4.4 UI/control ready, PID `2641` / instance `b530f4ff84abaa4e64fcacf3828ad470`가 확인됐습니다. 루벨리아와 실제 Settings는 **Desktop Pet / Auto / Herdr Waiting 0 / Tern Waiting 0**을 표시했고 이후 명시적 Hidden은 같은 기본 인스턴스를 보존했습니다. 새 설치의 증거이지 v0.4.4 일반 업데이트·managed 이전 완료 증거는 아닙니다. 설치 명령:

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
# Published immutable v0.4.7 installer and pinned assets:
git checkout --detach v0.4.7
bash scripts/install.sh --prebuilt
# Explicit source installation uses a separate checkout pinned to public v0.4.7.
# Replace with the installer-reported executable and your actual profile paths:
/absolute/reported/desktop-pet --config-dir /absolute/dev/config --state-dir /absolute/dev/state start
```

**v0.4.7 소스 설치**는 실제 공개 태그에 고정한 별도 체크아웃에서 명시적으로 실행합니다:

```sh
git clone https://github.com/hanbong5938/desktop-pet.git desktop-pet-source
cd desktop-pet-source
git checkout --detach v0.4.7
bash scripts/install.sh --source
```

불변 v0.4.7의 기본/`--prebuilt`는 정확한 공개 고정 자산을 사용합니다(과거 체크아웃은 각 pin 유지). 실패 시 source fallback은 없고 명시적 `--source`는 실제 공개 v0.4.7 태그를 포함해 선택한 core 체크아웃을 빌드합니다. standalone prebuilt는 **실제 OS 계정 home**의 `Applications/DesktopPet.app`, 승인된 source는 core의 `dist/DesktopPet.app`을 사용합니다. admission은 검증된 정상 기존 소유자를 재사용할 수 있으므로 설치기가 보고한 실행 파일을 사용하세요. 공유 locator는 계정 home `.local/share/desktop-pet/installation.json`에 원자 저장하며 탐색 근거일 뿐 권한이 아닙니다. 잘못된 명시 경로/locator는 fallback하지 않고 실행 파일·서명·capability·origin을 다시 검증합니다. Brew는 실제 formula/prefix를 소유하고 live/예약된 설치를 몰래 인수하지 않습니다.

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

중립 제품은 `DesktopPet.app`(`io.github.hanbong5938.desktop-pet`), CLI `desktop-pet`, helper `desktop-pet-update-coordinator`, release authority `hanbong5938/desktop-pet`, formula `hanbong5938/tap/desktop-pet`을 소유합니다. 현재 소스의 Standalone은 실제 OS 계정 home의 `.local/share/desktop-pet/libexec/DesktopPet.app`, Brew는 검증된 formula prefix의 `libexec/DesktopPet.app`, 승인된 source는 정확한 core의 `dist/DesktopPet.app`입니다. 공개 beta.7과 위 과거 고정 설치기는 옛 Applications 동작을 유지합니다. 영구 old-name alias나 Applications launcher는 현재 소스의 정상 탐색 경로가 아닙니다.

프로필 우선순위는 **명시적 `--config-dir`/`--state-dir` > `DESKTOP_PET_CONFIG_DIR`/`DESKTOP_PET_STATE_DIR` > default-profile record > 중립 XDG 기본값**입니다. 기본값은 `${XDG_CONFIG_HOME:-$HOME/.config}/desktop-pet`, `${XDG_STATE_HOME:-$HOME/.local/state}/desktop-pet`입니다. 기본 config root의 `default-profile.json`은 `{ "version":1, "config_dir":"/absolute/config", "state_dir":"/absolute/state" }`로 두 실제 경로를 원자 기록하며 재귀 profile pointer는 금지합니다. `desktop-pet profile show --json`은 daemon/디렉터리를 만들지 않는 읽기 전용입니다.

`bash scripts/run-native.sh --config-dir /absolute/dev-config --state-dir /absolute/dev-state start`는 개발 config/state **둘 다** 대응하는 기본 디렉터리와 물리적으로 달라야 합니다. Bash만 사용하는 비교는 기존 디렉터리와 symlink를 먼저 따라간 뒤 `..`를 처리하므로 아직 없는 leaf를 포함한 별칭도 확인하며 디렉터리를 만들지 않습니다. 빈/상대 경로, CR/LF, 일반 파일 component, dangling/looping symlink와 접근 불가 디렉터리는 거부합니다. 원래 인자를 native 실행 파일에 그대로 전달하므로 비교용 정규화가 더 엄격한 native profile 검증을 우회하지 않습니다. Bundle 서명 및 native 설치/origin 검사도 그대로 필요합니다.

이전은 기존 두 디렉터리를 **그 자리에서 채택**합니다. preferences/lifecycle/characters/menu-bar-icons와 lock/control socket, 구 `updates/` journal/helper를 이동·병합·삭제하지 않습니다. neutral default가 아직 미선택일 때만 legacy base pair를 기록합니다. 다른 pair를 가리키는 기존 default는 유지하고 이전 profile은 명시 경로로 사용하세요. 별도 Tern/beta/custom profile은 분리 상태를 유지합니다. default를 의도적으로 선택하려면:

```sh
desktop-pet profile adopt --config-dir /absolute/config --state-dir /absolute/state
```

adopt는 pair 선택이지 데이터 복사가 아닙니다. active update/예약, 다른 live daemon, invalid/symlink/nonowned 경로는 변경 전에 실패합니다. 실제 config/state **둘 다** 백업하고 start/status/command/stop에 같은 pair를 사용하세요. core는 `HERDR_PLUGIN_*`를 암묵적 app 기본값으로 해석하지 않습니다. Herdr wrapper만 host 주입 우선순위를 명시 native 인자로 바꾸고 Tern은 검증된 pair를 명시 전달하며 일치하는 중립 환경값을 설정합니다. status/Connection은 실제 실행 파일과 선택 profile을 표시합니다.

공유 locator는 **실제 OS 계정 home**의 `.local/share/desktop-pet/installation.json`이며 `{ "version":1, "executable":"/absolute/path/to/desktop-pet", "kind":"homebrew" }`입니다. kind는 `homebrew|standalone|source`만 허용하고 같은 디렉터리의 install lock 아래 원자 저장합니다. 현재 소스의 탐색 순서는 **사용자 명시 절대 실행 파일 → 유효 locator → PATH `desktop-pet` → 실제 `brew --prefix desktop-pet` 실행 파일 → `~/.local/share/desktop-pet/libexec/DesktopPet.app`**입니다. 공개 beta.7/과거 consumer에 이 fallback이 소급 반영되지는 않습니다. invalid 명시 경로/locator는 fallback하지 않습니다. locator는 권한이 아니며 소유·private regular file, 실제 executable/bundle/signature, capability와 origin을 매번 검증합니다. 실행 이미지/profile 불일치는 몰래 재사용하지 않고 표시합니다. 서명 없는 script·사용자 plugin·버전 문자열은 권한을 얻지 못합니다. 업데이트/migration의 locator 변경은 예약 아래 같은 계약을 따릅니다.

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

`scripts/build-rig-native.mjs`는 범위 제한된 소스/컴파일러/출력 해시를 `native/target/rig-native/release/rig-native.json`에 기록합니다. 패키징은 소스·생성 출력의 해시를 확인하고 원본 기록을 `Contents/Resources/rig-native-build.json`에 포함하며 독립 업데이트 코디네이터와 protocol/서명 검사도 유지합니다. 앱 서명 **이후** 코디네이터를 포함한 `Contents`의 모든 배포 파일 해시를 외부 `dist/DesktopPet.app.rig-native.json`에 기록해 서명 번들 안의 자체 해시 순환을 피합니다. 릴리스 워크플로는 기존 앱 전용 압축 파일·체크섬 두 파일 외에 별도 `DesktopPet-v0.4.7-rig-native.json`을 게시하고 versioned installer/checksum을 포함해 자산 6개를 제공합니다. 설치기/업데이터의 아카이브 입력 형식은 바꾸지 않으며 실제 추출된 서명 후 파일과 사이드카의 해시를 비교해야 배포 바이트 출처를 주장할 수 있습니다.

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

네이티브 문구나 CLI 언어 계약을 바꿀 때 [CLI 계약](cli.md), README [English](../readme.md)·[한국어](../readme.ko.md)·[简体中文](../readme.zh-Hans.md)·[日本語](../readme.ja.md), 사용 안내 [English](usage.md)·[한국어](usage.ko.md)·[简体中文](usage.zh-Hans.md)·[日本語](usage.ja.md)를 함께 갱신하세요. 이 문서들은 네이티브 앱에 관한 것이며 홍보 웹사이트의 네 언어 번역은 별도로 관리하세요.

## 캐릭터 제작

팩 제작·검증은 [캐릭터 제작 가이드](../.agents/skills/character-creator/SKILL.md)와 [`tools/character-pack.py`](../tools/character-pack.py)를 참고하세요. 배포 앱에도 `Contents/Resources/creator/SKILL.md`·`Contents/Resources/creator/character-pack.py`가 포함됩니다. 소스 체크아웃의 [로컬 Qwen + See-through 제작 스킬](../.agents/skills/create-pet-character/SKILL.md)은 **승인된 그림에서** 독립된 10포즈를 만듭니다. 외부 엔진·모델 가중치는 번들에 포함하지 않습니다. 루벨리아가 기본 rig 템플릿입니다. PNG 템플릿·Aurora·Coding Cat·선택형 루벨리아 의상과 각각의 그림 생성기는 별도 [herdr-characters](https://github.com/hanbong5938/herdr-characters)에서 관리합니다. 외부 템플릿은 `--templates-root PATH`로 지정해야 하며 번들에 있다고 가정하지 마세요.

가져온 팩의 라이선스·출처·사용 조건 파일을 함께 보관하세요. 기본 루벨리아의 [라이선스](../assets/rubelia-default/LICENSE.txt)·[출처](../assets/rubelia-default/ATTRIBUTION.txt)·[제작 기록](../assets/rubelia-default/source-record.json)·[배경 보정 기록](../assets/rubelia-default/background-repair.json)이 그림 기준입니다. [`LICENSE.txt`](../LICENSE.txt)는 원래 Coding Cat 고지를 유지하지만 Coding Cat과 그 그림 생성 소스는 별도 캐릭터 저장소로 이동했습니다. 이 고지들이 저장소의 모든 구성 요소에 일괄 라이선스를 부여하지는 않습니다. 그림 소유자의 승인은 모델 권리를 부여하지 않으며 소프트웨어 MIT는 그림 라이선스를 바꾸지 않습니다. Qwen 연구 라이선스는 제작 기록으로 보존하고 모델 가중치·엔진은 번들에 넣지 않습니다. 버전별 다시 그리기·얼굴 보정·당시 한계는 [v0.1.6](releases/v0.1.6.md)·[v0.1.9](releases/v0.1.9.md)·[v0.1.10](releases/v0.1.10.md)을 참고하세요.

## 웹사이트 (GitHub Pages)

현재 사이트의 제품 시각 자료는 정적 이미지이며 로컬 Pages 작업에 동영상·MP4 자산이 필요하지 않습니다.

캡처를 게시하기 전에 화면 내용을 직접 확인하세요. 격리된 시연 프로필에서 앱만 담고 실제 세션 내용·계정명·로컬 경로·호스트명·이메일·인증 정보를 포함하지 마세요. 공개 PNG에는 이미지·색상 청크만 허용하며 `check`와 `stage`는 EXIF·텍스트·프로필 등 메타데이터를 거부합니다. 이 검사는 화면에 보이는 글자를 판별하지 않으므로 개인정보 수동 검토도 반드시 필요합니다.

현재 v0.4.7 / v0.3.11 Pages 배포는 아직 관측하지 않았습니다. 아래 `cb10ce2` 배포 성공·영어/한국어 화면 검사는 과거 기록이며 링크·staged source는 오늘 공개 사이트가 바뀌었다는 증거가 아닙니다.

한국어 기본 홍보 웹사이트는 [`site/`](../site/)에 있습니다. 네이티브 macOS 앱을 설명하는 사이트이며 브라우저에서 앱을 실행하지 않습니다. Core Pages 주소(실제 `cb10ce2` 배포 성공, 영어/한국어 브라우저 화면 확인; 일본어/중국어 시각 검증 주장 없음): [한국어](https://hanbong5938.github.io/desktop-pet/) · [영어](https://hanbong5938.github.io/desktop-pet/en/) · [중국어 간체](https://hanbong5938.github.io/desktop-pet/zh-Hans/) · [일본어](https://hanbong5938.github.io/desktop-pet/ja/). 제품 문구를 바꿀 때 네 페이지의 번역과 언어 전환 링크를 함께 갱신하고 `ko`·`en`·`zh-Hans`·`ja`의 상호 `hreflang` 및 한국어 루트를 가리키는 `x-default`를 유지하세요. 각 페이지의 canonical URL과 Open Graph 언어도 해당 페이지에 맞춰야 합니다. 외부 [캐릭터 갤러리](https://github.com/hanbong5938/herdr-characters)는 별도로 유지되며 한국어·영어만 제공합니다. 과거 v0.3.2 스크린샷을 최신 화면이나 번역된 스크린샷처럼 소개하지 마세요. 영어 소셜 미리보기 이미지를 공유해도 페이지 언어는 바뀌지 않습니다. 제목은 화면 폭에 맞춘 간결한 크기와 좁은 주변 여백을 사용하며 본문·터미널 명령어·버튼 크기는 제목과 독립적으로 유지합니다. 저장소 루트에서 실행하세요.

```sh
bun test tools/pages.test.ts
node tools/pages.mjs check
node tools/pages.mjs serve --port 4173
# http://127.0.0.1:4173/desktop-pet/ 접속 (영어: /desktop-pet/en/; 중국어 간체: /desktop-pet/zh-Hans/; 일본어: /desktop-pet/ja/)
node tools/pages.mjs stage /tmp/herdr-pages
```

사이트 CLI에는 **Node.js 22**를 사용하고 **Bun 1.4.2**는 실제 Node 서버를 실행하는 HTTP 회귀 테스트의 러너로만 사용합니다. CI는 staging 전에 이 회귀 테스트를 실행합니다. `check`는 내부 링크·앵커·언어별 메타데이터·에셋을 확인하며 외부 링크를 네트워크로 검사하지 않습니다. 로컬 미리보기는 공개 파일 중 일반 파일만 제공합니다. 최종 `404.html` 대체 파일이 없거나 심볼릭 링크·일반 파일이 아니라면 파일을 전송하는 대신 `nosniff`가 있는 일반 텍스트 `500`을 반환하며 `HEAD`에는 본문이 없습니다. `stage`는 먼저 검사한 뒤 저장소 **밖의 새 디렉터리 또는 빈 디렉터리**에 공개할 `site/` 파일만 복사하며 배포하지 않습니다. Pages 워크플로는 PR을 검사하고 검토용 아티팩트만 첨부하며 배포하지 않습니다. Pages 배포 소스는 **GitHub Actions**이고, 웹사이트 변경을 `main`에 병합하거나 `main`에서 워크플로를 수동 실행했을 때 조건을 만족하는 `main` 실행만 배포합니다. 이 로컬 명령은 릴리스 바이너리·Wiki·저장소 Pages 설정을 바꾸지 않습니다.
