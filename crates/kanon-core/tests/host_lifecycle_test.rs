//! Tests for the plugin-host lifecycle: graceful stop, escalation, and orphan prevention.

use std::time::Duration;

use kanon_core::supervisor::{HOST_SHUTDOWN_GRACE, terminate_child};
use tokio::io::{AsyncBufReadExt, BufReader};
use std::process::Stdio;
use tokio::process::{Child, Command};

/// Spawns a shell child that has *already installed* its signal disposition.
///
/// The readiness marker matters: signalling `sh` before it executes `trap` would exercise the
/// default disposition (immediate death), not the behaviour under test.
async fn shell_with_trap(trap: &str) -> Child {
    let script = format!("trap '{trap}' TERM; echo READY; while :; do sleep 0.05; done");
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(script)
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn test child");

    let stdout = child.stdout.take().expect("child stdout");
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    reader
        .read_line(&mut line)
        .await
        .expect("read readiness marker");
    assert_eq!(line.trim(), "READY", "child failed to install its trap");

    child
}

#[tokio::test]
async fn a_cooperative_host_is_stopped_by_sigterm() {
    // The child records that it received SIGTERM by exiting with a marker status.
    let mut child = shell_with_trap("exit 7").await;

    terminate_child(&mut child, Duration::from_secs(3))
        .await
        .expect("terminate child");

    // `wait` already reaped it inside `terminate_child`; a second wait reports the same status.
    let status = child.wait().await.expect("child status");
    assert_eq!(
        status.code(),
        Some(7),
        "the child must observe SIGTERM and run its trap, got {status}"
    );
}

#[tokio::test]
async fn a_host_that_ignores_sigterm_is_killed_after_the_grace_period() {
    // This is the wedged-host case: it never handles SIGTERM, so only escalation frees the slot.
    let mut child = shell_with_trap("").await;

    let started = std::time::Instant::now();
    terminate_child(&mut child, Duration::from_millis(200))
        .await
        .expect("terminate child");
    let elapsed = started.elapsed();

    let status = child.wait().await.expect("child status");
    assert!(
        !status.success(),
        "an ignoring host must be killed, not reported as a clean exit: {status}"
    );
    assert!(
        elapsed >= Duration::from_millis(200),
        "the grace period must be honoured before escalating, took {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "escalation must not hang, took {elapsed:?}"
    );
}

#[tokio::test]
async fn an_already_exited_host_is_reaped_without_error() {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg("exit 0")
        .kill_on_drop(true)
        .spawn()
        .expect("spawn test child");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Signalling a dead process must not surface as a lifecycle failure.
    terminate_child(&mut child, Duration::from_millis(200))
        .await
        .expect("terminating an exited child is a no-op");
    let status = child.wait().await.expect("child status");
    assert_eq!(status.code(), Some(0));
}

#[test]
fn the_shutdown_grace_period_is_bounded() {
    // A very long grace would stall core shutdown; a very short one would deny plugins their
    // unload hook. Pin the operational choice so it cannot drift silently.
    assert!(HOST_SHUTDOWN_GRACE >= Duration::from_secs(1));
    assert!(HOST_SHUTDOWN_GRACE <= Duration::from_secs(10));
}
