# Desktop Pet — Herdr integration

[English](readme.md) · [한국어](readme.ko.md) · [日本語](readme.ja.md) · [简体中文](readme.zh-Hans.md)

**게시 경계:** 기존 [Herdr v0.3.6](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6)은 protocol 3 / observation capability 1로 출시되었습니다. 독립 Desktop Pet v0.4.0(protocol 4 / capability 2 / integration API 1)과 이전용 bridge v0.3.7은 검토된 후보이며 공개 설치·런타임 검증은 아직 완료되지 않았습니다. 아래 명령은 중립 제품 cutover 안내이며 문서·태그·소스 버전은 게시 증거가 아닙니다.

필요한 연동만 설치하세요. Herdr는 얇은 consumer이며 Tern은 네이티브 앱을 다운로드·빌드·설치·자동 시작하지 않습니다. 플러그인 갱신·제거는 연동만 바꾸고 공용 앱·프로필·캐릭터는 남깁니다. 일반 앱 업데이트는 Herdr 플러그인을 재설치하지 않습니다.

[Desktop Pet / core app](https://github.com/hanbong5938/desktop-pet/blob/main/readme.ko.md) · [Herdr host / patch / OMP / link](integrations/herdr/README.md) · [Legacy A → B migration](https://github.com/hanbong5938/desktop-pet/blob/main/docs/migrations/desktop-pet.md)

중립 릴리스와 포뮬러가 실제로 게시된 뒤 공개 설치 명령은 다음과 같습니다:

```sh
brew install hanbong5938/tap/desktop-pet
herdr plugin install hanbong5938/herdr-desktop-pet
herdr plugin action invoke start --plugin desktop-pet
```

Apple Silicon Mac, macOS 13 이상. 소스 빌드는 Rust/Cargo·Bun·Node.js/npm·Xcode Command Line Tools가 필요하며 사전 빌드 설치에는 필요 없습니다. 앱은 ad-hoc 서명·미공증입니다. Herdr 스냅샷에는 호환 로컬 서버(0.9.0 이상), 답장에는 별도 `agent.prompt`가 필요합니다. 선택형 client-attach 자동 시작에는 공식 Herdr 0.9.3에 [연동 저장소의 패치와 호스트 빌드 절차](https://github.com/hanbong5938/herdr-desktop-pet/blob/main/integrations/herdr/README.md)를 적용하거나 실제 훅 지원 호스트를 사용해야 합니다. 순정 0.9.0/0.9.3에는 훅이 없고 플러그인 설치는 호스트를 패치하지 않습니다.

사전 빌드·소스 설치는 Herdr 연동이 아니라 **core** 체크아웃에서 실행합니다. 기본 설치는 prebuilt-only이며 다운로드·검증 실패를 소스 빌드로 숨기지 않습니다. `--source`는 현재 core 체크아웃만 빌드하고 그 `dist/DesktopPet.app`을 사용합니다.

> Native app features below are owned and documented by the independent core, not built in this integration checkout.

## 주요 기능

- 메뉴 막대에서 제어하는 네이티브 캐릭터: 터치·쓰다듬기·이동·크기 변경, 시스템 언어 또는 한국어/영어/중국어 간체/일본어 UI.
- 관측한 로컬·원격 세션의 상태 카드와 검색·정렬; 원격 카드는 읽기 전용.
- 로컬 에이전트 카드의 한 줄 답장과 별도 말풍선 표시·크기·대사 설정.
- 공식 캐릭터 브라우저의 선택형 팩 다운로드·적용 및 로컬 팩 가져오기.
- 실행 관리·관찰 대상 설정과 출처별 명시적 앱 업데이트; 자동 업데이트 확인은 읽기 전용.
- **현재 소스 전용:** 통합 연결 설정에서 Auto 또는 등록된 제공자(Herdr·Tern 등)를 선택하고 실제 제공자 상태를 확인하며 공용 프로필에 실행 중 변경을 저장합니다. 공개 v0.3.6에는 이 capability-2 UI가 없습니다.

### Tern 관찰: 소스 전용 exchange v2

Auto가 Tern을 선택하거나 Tern을 직접 선택하면 현재 exchange-v2 소스는 로컬 에이전트뿐 아니라 연결된 모든 Tern SDK 호스트를 자동 관찰합니다. 카드에는 호스트 이름과 로컬/원격 표기가 붙고, 호스트별 Offline 상태와 관찰 수명은 독립적입니다. Tern 호스트 관찰은 Herdr의 local/remote/machines 범위 설정과 별개이며 새 호스트 선택 UI는 없습니다. 답장은 로컬 전용이고 원격 카드에는 보낼 수 없습니다.

이는 공개 v0.3.6의 동작이 아니며 당시 로컬 전용 Tern 경로는 그대로입니다. 실제 원격 SDK 에이전트와 호환되는 네이티브/플러그인 설치는 **아직 검증되지 않았습니다**. 현재 플러그인의 중립 `DesktopPet.app` 설치 요구사항이 이 소스 브랜치의 `HerdrDesktopPet.app` 번들과의 호환성을 입증하지는 않습니다. 전체 소스 계약과 검증 경계는 [사용법](docs/usage.ko.md)과 [개발 / 검증 근거](docs/development.ko.md)를 참조하세요.


## Source / link

Install or build the core first (`bash scripts/install.sh --source` in the **desktop-pet core** checkout). Then link **this integration checkout**, not the core. `plugin link` does not run a build. The integration wrapper discovers a verified compatible native executable, preserving the exact profile/socket provenance; it never compiles a native app here. Client-attach startup remains optional and requires the actual patched host/hook; plugin install does not patch Herdr. See [separate checkouts](integrations/herdr/README.md#build-in-separate-checkouts).

## Ownership

This repository owns `native-adapter`, `herdr-plugin.toml`, thin install/run wrappers, `integrations/herdr` and `integrations/omp`, integration validation, and frozen historical releases. App/UI/rig/artwork/packaging/source instructions belong to [desktop-pet](https://github.com/hanbong5938/desktop-pet). `.herdrchar`, pack v5, `default@0`, rig ABI and [herdr-characters](https://github.com/hanbong5938/herdr-characters) catalog/gallery authority are unchanged. Historical [v0.3.6](https://github.com/hanbong5938/herdr-desktop-pet/releases/tag/v0.3.6) and older release bodies/tags/assets remain immutable; an integration-only v0.4.0 does not carry a native archive.

## 그림 및 라이선스

소프트웨어 MIT 라이선스와 그림·모델 사용 조건은 서로 다릅니다. 기본 루벨리아의 [라이선스](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/LICENSE.txt)·[출처](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/ATTRIBUTION.txt)·[제작 기록](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/source-record.json)·[배경 보정 기록](https://github.com/hanbong5938/desktop-pet/blob/main/assets/rubelia-default/background-repair.json), 원본 Coding Cat 고지가 남은 [LICENSE.txt](LICENSE.txt)를 각각 확인하세요. 그림 소유자 승인은 모델 권리를 부여하지 않으며 소프트웨어 MIT가 그림을 재라이선스하지 않습니다. Qwen 연구 라이선스는 제작 기록에 보존되고 모델 가중치·엔진은 번들에 없습니다. 가져온 팩의 라이선스·출처·사용 조건도 함께 보관하세요. [그림 제작 상세](https://github.com/hanbong5938/desktop-pet/blob/main/docs/development.ko.md#캐릭터-제작).
