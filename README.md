# Xperia VoLTE Activator

Sony Xperia의 VoLTE 패치 과정을 통합하는 Windows용 도구입니다. Tauri 2, SvelteKit, TypeScript, Rust로 개발하며 EFS/NV 처리는 내장 Rust 모듈을 사용합니다.

## 기종별 검토 상태

현재 전체 절차를 검토하는 단말은 **Xperia 1 V JP 모델(XQ-DQ44)** 한 대입니다. 아래 표는 현재 코드의 기종 정보를 기준으로 정리했으며, 같은 기종의 다른 지역·모델까지 검토한 것으로 간주하지 않습니다.

- **전체 검토 중**: 해당 단말을 대상으로 전체 워크플로우를 검토하고 있습니다. 실기기 전체 절차 검증은 아직 완료되지 않았습니다.
- **검토 안 됨**: 해당 단말에서 이 프로그램의 전체 절차를 검토하지 않았습니다. 코드에 기종 분기나 참고 사항이 있어도 검토 완료를 뜻하지 않습니다.

<!-- 기종·모델·상태 표 형식 참고: https://github.com/openandroidinstaller-dev/openandroidinstaller/blob/main/README.md -->

| 기종 | 지역·모델 코드 | 검토 상태 |
| --- | --- | --- |
| **Xperia 1 V** | **JP · XQ-DQ44** | **전체 검토 중** |
| Xperia 1 V | 그 외 지역·모델(XQ-DQ* 중 XQ-DQ44 제외 포함) | 검토 안 됨 |
| Xperia 5 V | XQ-DE* | 검토 안 됨 |
| Xperia 1 VI | XQ-EC* | 검토 안 됨 |
| Xperia 1 IV | XQ-CT* | 검토 안 됨 |
| Xperia 5 IV | XQ-CQ* | 검토 안 됨 |
| Xperia 1 III | XQ-BC* | 검토 안 됨 |
| Xperia 5 III | XQ-BQ* | 검토 안 됨 |
| Xperia PRO-I | XQ-BE* | 검토 안 됨 |
| Xperia 1 II | XQ-AT* | 검토 안 됨 |
| Xperia 5 II | XQ-AS* | 검토 안 됨 |
| Xperia 10 IV | XQ-CC* | 검토 안 됨 |
| Xperia 10 V | XQ-DC* | 검토 안 됨 |
| Xperia 10 VI | XQ-ES* | 검토 안 됨 |
| 위에 없는 기종·지역·모델 | — | 검토 안 됨 |

`*`는 모델 코드 접미사를 묶어 표기한 것입니다. 실기기 단계별 검증 항목은 [기기 테스트 체크리스트](.plans/04-engine/device-test-checklist.md)에서 관리합니다.

## 개발용 단계 실행

내일 실기기 검토용 CLI는 Cargo `dev-cli` feature로 별도 빌드한다. 앱과 **같은 Rust 엔진**을 호출하므로 엔진을 수정하고 재빌드하면 양쪽에 적용된다. 한 명령씩 실행하고 결과를 남겨 필요한 단계만 다시 실행할 수 있다. 일반 앱의 쓰기 기능 기본값은 유지한다.

```powershell
cargo run --manifest-path src-tauri/Cargo.toml --bin xva-dev --features dev-cli -- commands
```

쓰기 feature를 포함한 빌드·단계별 요청·재시작·로그 확인은 [개발 CLI 사용법](.plans/04-engine/dev-cli.md)을 따른다. MSVC 개발 셸이 필요하다. CLI는 Rust 엔진 검토용이며 화면의 전체 단계 전환은 앱에서 따로 검토한다. 단계 순서·대기 시간·기종별 파티션 선택·수동 확인 같은 흐름은 앱(TypeScript 위자드)에만 있어 CLI 결과로 보장되지 않는다. 기기 명령은 GUI 앱을 닫고 실행하고, 초기화 단계는 백업 증명 옵션이 필요하다(사용법 문서의 "실기기 세션 규칙").

## Recommended IDE Setup

[VS Code](https://code.visualstudio.com/) + [Svelte](https://marketplace.visualstudio.com/items?itemName=svelte.svelte-vscode) + [Tauri](https://marketplace.visualstudio.com/items?itemName=tauri-apps.tauri-vscode) + [rust-analyzer](https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer).
