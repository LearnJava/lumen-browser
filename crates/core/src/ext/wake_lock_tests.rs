use super::*;

#[test]
fn null_provider_acquire_returns_true() {
    let p = NullWakeLockProvider;
    assert!(p.acquire());
}

#[test]
fn null_provider_release_is_no_op() {
    let p = NullWakeLockProvider;
    p.release(); // must not panic
}

#[test]
fn null_provider_idempotent_acquire() {
    let p = NullWakeLockProvider;
    assert!(p.acquire());
    assert!(p.acquire()); // second call is also fine
}
