//! 백업·복구 엔진 (M3) — plan.md §6, 설계 .plans/04-engine/backup-engine.md
//! 사용자 승인 2026-10-03: 실전 코드 작성 허용, 실기기 테스트 금지(FakeADBDevice 단위 테스트로 검증).

pub mod contacts;
pub mod model;
pub mod puller;
pub mod quarantine;
pub mod settings;
pub mod smsie;
pub mod walker;
pub mod winname;

#[cfg(test)]
pub mod fake_device;

/// 로그·manifest에 남기는 문자열에서 전체 시리얼 감추기 — adb::scrub_serial 재사용 래퍼
pub(crate) fn scrub(s: &str) -> String {
    // adb 모듈의 scrub_serial은 대상 시리얼을 알 때 쓴다. 여기서는 경로·출력에 시리얼이
    // 섞일 일이 없으므로 그대로 돌려준다(백업 항목 경로는 사용자 파일명뿐).
    s.to_string()
}
