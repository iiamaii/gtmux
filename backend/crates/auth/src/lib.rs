//! gtmux-auth — 256-bit CSPRNG 토큰 발급 / 회전 / 상수시간 비교 / redaction.
//!
//! Bootstrap scaffold only. 실제 구현은 `ring::rand::SystemRandom` +
//! `ring::constant_time::verify_slices_are_equal` (R7 §3 + ADR-0011 D8).

#![forbid(unsafe_code)]

/// 부트스트랩 placeholder — 상수시간 토큰 검증 시그니처.
///
/// 실제 구현은 `ring::constant_time::verify_slices_are_equal(presented,
/// expected)` 호출로 timing side-channel을 차단한다.
pub fn verify_token(_presented: &[u8], _expected: &[u8]) -> bool {
    todo!("auth::verify_token — constant-time compare via ring")
}
