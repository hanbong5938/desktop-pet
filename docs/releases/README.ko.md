# Desktop Pet 버전 안내

**게시 경계:** [Desktop Pet v0.4.2](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.2)은 공개되었습니다(release `412241744`, `2026-10-14T01:09:50Z`, 불변 소스 `cb10ce2c3cc65402cbb914d60301f7627d5546d5`). 익명 자산 6개, strict/deep 서명, protocol 4 / capability 2 / integration API 1, 68개 배포 리소스 해시·정확한 closure를 검증했습니다. tap `32919c745864b54a2e716fcc5a3714d0d38d3e84`의 중립 포뮬러와 실제 공개 v0.4.1 → v0.4.2 Brew 업그레이드는 receipt/derivation·서명 5개·`brew test`·Auto 네이티브 UI/control ready(Herdr 0 / Tern 0)를 입증했습니다. 새 공개 standalone v0.4.1 설치 및 동시 설치기 2개의 실행 중 소유자 재사용을 확인했으며, 네이티브 Check가 실제 공개 v0.4.2를 제시하고 동의 후 `standalone_replace`·`manager_exited`·`installed:true`·`applied:true`, 같은 config/state·정확한 공개 v0.4.2 이미지의 준비 완료를 확인했습니다. Check는 locator/profile/lifecycle/locks/PID를 유지했지만 `preferences.json` 확인 캐시는 바꿨으므로 모든 설정 바이트 불변은 아닙니다. [수정 integration-only Herdr v0.4.1](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.4.1)도 공개되었습니다(release `412252863`, 불변 ref `6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`). 공개 wrapper 2개가 숨김을 유지하며 같은 Brew v0.4.2 PID/instance/image를 재사용했고, 명시적 config/state와 실제 private patched Herdr 0.9.3 socket으로 Auto Herdr 1 / Tern 0에 도달했습니다. 새 환경의 공개 bootstrap 2개는 OS-home 앱 하나를 설치/재사용했으며 연동-local native 파일이나 데몬 시작은 없었습니다. 이는 공개 Herdr manager 설치 receipt 증거가 아닙니다. 원래 Herdr v0.4.0의 생성 앱·실패 테스트 scaffold symlink 소스 결함은 보존하며 integration-only였다고 주장하지 않습니다. [Bridge v0.3.7](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.7)도 공개되었습니다(release `412267469`, 불변 tag `987a4b859f95e922c1c5443ead372d10d0dddb24`). 정확한 Core `cb10ce2`·수정 Herdr `6d2ac04`에서 빌드했고 익명 자산 5개·서명·68개 파일 closure를 검증했습니다. tap `657f32cc4cc563ae40ae0432ac5123dcb3f58426`에서 old stable 포뮬러만 v0.3.7로 변경했으며 beta는 그대로입니다. manual-origin private 실제 호스트 실행은 Herdr 1 / Tern 0 ready를 입증했지만 managed A→B 증거는 아닙니다. 실제 동결 공개 Brew v0.3.6 → v0.3.7 A `6a10663c2d2f0177990623a969238af4`와 별도 동의 Core v0.4.2 B `c66961f9e5a91c0023545065ae61385b`는 실제 Brew retirement(exit 0, quiescent)까지 완료했습니다. B 전후 모든 설정 파일 해시와 이전 A 증거 87개 파일은 불변이며 실제 Core v0.4.2 UI는 Desktop Pet / Herdr Waiting 0 / Tern Disabled 0입니다. 반면 실제 managed v0.3.7 A `a1aec5902020de7bcaaf583b1a5f50a5`는 native `/tmp` 프로필 검증에 실패했습니다. checkout 교체·registry 등록 전 manager exit 1이 발생했고 동결 helper는 Unknown / `manager_exited`, installed/applied false를 보존합니다. VM·journal·reservation은 recovery/replay 없이 유지합니다. 전체 matrix 완료는 아닙니다. 명시적 definite-failed-retirement resume·실제 SDK 양쪽 시작 순서의 같은 인스턴스/재연결·계정 수락은 미입증입니다. Core v0.4.3 + bridge v0.3.8은 승인된 수정 버전이지만 실제 게시·managed 수락 대기 중이며 공개 v0.4.2/v0.3.7의 대체물이 아닙니다. [Core v0.4.3 / bridge v0.3.8](https://github.com/hanbong5938/desktop-pet/blob/main/docs/releases/v0.4.3.md). Tern v0.2.0·호환 Core v0.4.1 pin은 그대로이며 실제 Tern keychain/provider 로그인과 OMP providerSignIn/NoModel 해결이 필요합니다. 과거 태그·본문·자산·소스는 보존합니다. Core Pages `cb10ce2` 배포와 EN/KO 브라우저 확인은 성공했지만 JA/ZH 시각 확인·최종 현재 문서 배포는 대기 중입니다. Linux Wiki catalog 렌더는 통과했지만 전용 `WIKI_PUBLISH_TOKEN`·소유자의 Wiki Home 초기화가 필요합니다. Wiki 링크는 공개 증거가 아니며 바이너리 release job 성공과 뒤이은 Wiki 실패는 별개입니다.

[실제 Desktop Pet v0.4.2 release](https://github.com/hanbong5938/desktop-pet/releases/tag/v0.4.2) · [v0.4.2 게시 기록](v0.4.2.md) · [Desktop Pet v0.4.1 게시 및 검증 기록](v0.4.1.md) · [과거 v0.4.0 게시 및 검증 기록](v0.4.0.md) · [A → B migration](../migrations/desktop-pet.md) · [Core release authority](https://github.com/hanbong5938/desktop-pet/releases)

**Core 수정 게시 완료, 수락 대기:** [Core v0.4.2 기록](v0.4.2.md)은 실제 공개 core를 기록합니다. 공개 prebuilt·재현 가능한 source 명령은 불변 `v0.4.2`를 사용하며 중립 포뮬러도 공개되었습니다. 수정 [Herdr v0.4.1](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.4.1)(`6d2ac0446f6ecb84f179e21fac3c5b02e07dc8e8`, release `412252863`)·[bridge v0.3.7](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.7)(`987a4b859f95e922c1c5443ead372d10d0dddb24`, release `412267469`)은 실제 게시되었습니다. 공개 standalone v0.4.1 → v0.4.2 네이티브 업데이트와 같은 프로필 준비는 완료했고, 실제 동결 공개 Brew v0.3.6 → v0.3.7 A `6a10663c2d2f0177990623a969238af4`와 별도 동의 Core v0.4.2 B `c66961f9e5a91c0023545065ae61385b`는 실제 Brew retirement(exit 0, quiescent)까지 완료했습니다. B 전후 모든 설정 파일 해시와 이전 A 증거 87개 파일은 불변이며 실제 Core v0.4.2 UI는 Desktop Pet / Herdr Waiting 0 / Tern Disabled 0입니다. 반면 실제 managed v0.3.7 A `a1aec5902020de7bcaaf583b1a5f50a5`는 native `/tmp` 프로필 검증에 실패했습니다. checkout 교체·registry 등록 전 manager exit 1이 발생했고 동결 helper는 Unknown / `manager_exited`, installed/applied false를 보존합니다. VM·journal·reservation은 recovery/replay 없이 유지합니다. 전체 matrix 완료는 아닙니다. 명시적 definite-failed-retirement resume·실제 SDK 양쪽 시작 순서의 같은 인스턴스/재연결·계정 수락은 미입증입니다. Core v0.4.3 + bridge v0.3.8은 승인된 수정 버전이지만 실제 게시·managed 수락 대기 중이며 공개 v0.4.2/v0.3.7의 대체물이 아닙니다. [Core v0.4.3 / bridge v0.3.8](https://github.com/hanbong5938/desktop-pet/blob/main/docs/releases/v0.4.3.md). 검증한 공개 SHA-256: archive `1693fcc5a5780fd08eab6f3b15c62f6a3be54a86574a557fd431ab48aa251427`, installer `a0ec644c3baecc7b81baddd29fbbbd3a8f25777e804a6d9940b78820bacc08ec`, rig `b5fa61ad316e52ed3b2024201b009b2dffd8fc1481530c5eb69549b329101284`.

[English](README.md) · **한국어**

이 저장소에서 검토한 Markdown이 버전 문서의 원본입니다. GitHub Wiki는 원본에서 생성한 읽기용 화면이며 따로 편집하는 원본이 아닙니다. 개요·설치는 [한국어 루트 안내](../../readme.ko.md), 현행 조작·운영은 [사용 안내](../usage.ko.md), 소스·웹사이트 개발은 [개발 안내](../development.ko.md), Herdr 호스트 연동은 [연동 안내(영어)](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)를 기준으로 합니다.

현재 앱 문서·설치: [English](../../readme.md) · [한국어](../../readme.ko.md) · [日本語](../../readme.ja.md) · [简体中文](../../readme.zh-Hans.md). 현재 앱 사용법: [English](../usage.md) · [한국어](../usage.ko.md) · [日本語](../usage.ja.md) · [简体中文](../usage.zh-Hans.md). 4개 언어 앱 사이트와 생성된 앱 Wiki는 core 소유이며 이전 Herdr 사이트·문서는 연동·이전 입구입니다. 실제 공개 여부는 별도로 확인합니다.

## 목적에 맞는 문서 선택

| 필요한 정보 | 문서 |
| --- | --- |
| 현행 앱 조작·캐릭터·관찰·문제 해결 | [사용 안내](../usage.ko.md) · [CLI 계약](../cli.md#한국어) |
| 소스 빌드·캐릭터 제작·홍보 웹사이트 개발 | [개발 안내](../development.ko.md) |
| 공개 v0.3.6 안정판·Tern 연동·4개 언어·protocol-3 베타 명시적 동의 | [v0.3.6 정본 노트·업그레이드 안내](v0.3.6.md) · [0.3 게시 증거](0.3.md#v036-실제-게시-확인) |
| 공개 v0.2.1 안정판 호환성 패치·Arin 압축 팩 | [0.2 릴리스 계열](0.2.md) · [v0.2.1 한영 노트](v0.2.1.md) · [v0.1.11 → v0.2.0 한영 이전 안내](../migrations/v0.1.11-to-v0.2.0.md) |
| 이전 0.1 계열 공개 바이너리(마지막 v0.1.11) | [0.1 릴리스 계열](0.1.md) · [v0.1.11 Release](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.1.11) |
| 과거 입력창에서 카드별 인라인 답장으로의 전환 | [v0.1.4 → v0.1.6 마이그레이션](../migrations/v0.1.4-to-v0.1.6.md) |
| 버전·호환성 원칙 | [버전 및 호환성 정책](policy.md) |
| 같은 원본을 Wiki와 새 Release 본문에 게시하는 방법 | [게시 절차](publishing.md) |
| 실제 공개 릴리스와 자산 상태 | [Core GitHub Releases](https://github.com/hanbong5938/desktop-pet/releases), 생성된 core Wiki의 **Release-Status** 및 **Older-Releases** 페이지 |

역사적 Herdr 게시 기록 — **v0.3.6은 공개 안정판이며 비초안·비프리릴리스이고 GitHub의 관측된 `latest`입니다.** [정확한 Release](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6)에 자산 4개가 모두 게시되어 있으며 [현행 게시·소비자 증거](0.3.md#v036-실제-게시-확인)에 익명 다운로드 대조·압축 파일 SHA-256·stable tap 전환·실제 v0.3.5 → v0.3.6 Homebrew 업그레이드·Cargo/Bun/Node/npm 없는 고정 사전 빌드 설치를 기록했습니다. 동결된 [정본 노트](v0.3.6.md)는 Tern 연동·4개 언어·protocol-3 베타 명시적 동의를 설명하며 게시 전 표현은 당시 기록으로 보존합니다. 소비자 경로는 `brew install hanbong5938/tap/herdr-desktop-pet`로 Rust 없이 네이티브 사전 빌드 앱을 설치하고 `tern plugin install https://github.com/hanbong5938/tern-desktop-pet`로 Tern 연결 플러그인을 명시적으로 설치합니다. 같은 앱의 기본 호스트는 Herdr이며 Tern은 [사용 안내](../usage.ko.md)의 명시적 `--host tern` 시작과 별도 설정 **및** 상태 경로를 사용합니다. 플러그인은 앱을 몰래 다운로드·빌드·실행하지 않습니다. 소비자 smoke는 네이티브 Tern 전용 시작과 빈 snapshot 교환을 점검했으며 공개 플러그인의 실제 CLI 설치는 발견 증거이지 GUI 창 연결이나 실제 에이전트 전달 증거가 아닙니다. [이전 v0.3.5 게시 기록](0.3.md)과 기존 베타 기록은 과거 증거로 보존합니다.

생성된 Wiki의 릴리스 상태는 GitHub의 실제 Release 목록과 모두 업로드된 자산으로 판단하며 문서 파일이나 태그만으로 게시 완료를 판단하지 않습니다. 연결된 v0.3.6 증거는 관측된 게시 기록이며 계속 갱신되는 API 상태표는 아닙니다. 과거 0.1 계열의 **v0.1.5는 태그만 있고 공개 바이너리가 없습니다.**

## 릴리스 계열

| 계열 | 범위 |
| --- | --- |
| [0.3](0.3.md) | 공개 v0.3.6 안정판 [게시 증거](0.3.md#v036-실제-게시-확인)·동결 [정본 노트/업그레이드 안내](v0.3.6.md); 이전 v0.3.5와 별도 베타 증거는 과거 기록으로 보존 |
| [0.2](0.2.md) | 이전 v0.2.1 안정판 호환성 패치 [정본 한영 노트](v0.2.1.md), v0.2.0 [안정판 노트](v0.2.0.md)·[한영 업그레이드 안내](../migrations/v0.1.11-to-v0.2.0.md) |
| [0.1](0.1.md) | 과거 공개 v0.1.4 및 v0.1.6–v0.1.11, 태그만 있는 v0.1.5 |

이후 minor 계열에는 별도의 `X.Y.md` 개요와 필요에 따라 버전 노트·이전 안내를 둡니다. 이 문서는 지원 기간을 약속하지 않습니다.

## 게시와 과거 기록

새 릴리스는 정확한 릴리스 태그의 검토된 `docs/releases/vX.Y.Z.md`를 Wiki 버전 페이지와 새 GitHub Release 본문의 공통 원본으로 사용합니다. [게시 절차](publishing.md)와 [호환성 정책](policy.md)을 참고하세요.

v0.1.4–v0.1.11 태그에는 이 문서 트리가 없었습니다. 따라서 과거 노트는 당시 근거를 바탕으로 현재 소스에서 재구성한 문서이며, 과거 태그의 원본과 실제 Release를 연결합니다. 과거 태그·배포 파일·Release 본문을 다시 쓰지 않습니다. 과거 검증 내용은 당시 보고된 근거이지 이 문서 작업에서 새로 실행한 검증이 아닙니다.

## 업그레이드 안전 요약

v0.3.6으로 옮길 때는 기존 데몬을 먼저 종료하고 [v0.3.6 정본 업그레이드 안내](v0.3.6.md)를 확인하세요. 공개 자산 4개와 stable 포뮬러의 정확한 태그 URL·검증된 SHA는 [현행 게시 기록](0.3.md#v036-실제-게시-확인)에 있습니다. 업그레이드 후 CLI 버전뿐 아니라 다시 시작한 앱의 `status` 버전·실행 경로도 확인하세요. Herdr/Tern 프로필은 설정과 상태를 모두 분리하며 이전 protocol-1/2 updater 기록·예약은 보존하고 해당 helper로 복구합니다. 이전 v0.3.5 게시와 업그레이드는 [0.3 과거 기록](0.3.md), v0.1.11 → v0.2.0 전환은 [과거 한영 안내](../migrations/v0.1.11-to-v0.2.0.md), Arin 수정은 [v0.2.1 패치](v0.2.1.md)를 참고하세요.
