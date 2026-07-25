//! End-to-end checks against a real machine.
//!
//! Skipped unless `PAROLASSH_LIVE_HOST` is set, because it needs a throwaway
//! box you are willing to have rebooted at:
//!
//! ```sh
//! PAROLASSH_LIVE_HOST=192.168.56.10 \
//! PAROLASSH_LIVE_USER=pheeleep \
//! PAROLASSH_LIVE_PASSWORD=secret \
//! cargo test --test live_remote -- --nocapture --test-threads=1
//! ```
//!
//! The power test schedules a reboot far enough out to be harmless and then
//! cancels it, so the machine stays up. Nothing here reboots anything.

use parolassh_lib::remote::client::{Credentials, Session, Target};
use parolassh_lib::remote::power::{self, Elevation, PowerAction, PowerRequest};
use parolassh_lib::remote::{probe, OsFamily};
use zeroize::Zeroizing;

struct LiveConfig {
    hostname: String,
    port: u16,
    username: String,
    password: String,
}

fn config() -> Option<LiveConfig> {
    let hostname = std::env::var("PAROLASSH_LIVE_HOST").ok()?;
    Some(LiveConfig {
        hostname,
        port: std::env::var("PAROLASSH_LIVE_PORT")
            .ok()
            .and_then(|port| port.parse().ok())
            .unwrap_or(22),
        username: std::env::var("PAROLASSH_LIVE_USER").expect("PAROLASSH_LIVE_USER must be set"),
        password: std::env::var("PAROLASSH_LIVE_PASSWORD")
            .expect("PAROLASSH_LIVE_PASSWORD must be set"),
    })
}

async fn connect(config: &LiveConfig) -> Session {
    let target = Target {
        hostname: config.hostname.clone(),
        port: config.port,
        username: config.username.clone(),
    };
    let credentials = Credentials::Password(Zeroizing::new(config.password.clone()));

    // `true` mirrors answering "yes" to ssh's first-connection prompt: the key
    // is recorded in known_hosts and verified automatically from then on.
    Session::connect(&target, &credentials, true)
        .await
        .expect("connection failed")
}

#[tokio::test]
async fn probes_the_port_before_authenticating() {
    let Some(config) = config() else {
        eprintln!("skipping: PAROLASSH_LIVE_HOST not set");
        return;
    };

    let result = probe::probe(&config.hostname, config.port).await.unwrap();
    println!("probe: {}", result.message);

    assert!(result.reachable, "port {} is not open", config.port);
    assert!(result.is_ssh, "what answered is not an SSH server");
    assert!(result.banner.is_some());
}

#[tokio::test]
async fn a_wrong_password_is_refused_clearly() {
    let Some(config) = config() else {
        eprintln!("skipping: PAROLASSH_LIVE_HOST not set");
        return;
    };

    let target = Target {
        hostname: config.hostname.clone(),
        port: config.port,
        username: config.username.clone(),
    };
    let wrong = Credentials::Password(Zeroizing::new("definitely-not-the-password".into()));

    // `Session` is deliberately not `Debug` — it holds a live connection — so
    // the result is matched rather than unwrapped.
    let message = match Session::connect(&target, &wrong, true).await {
        Ok(session) => {
            session.close().await;
            panic!("a wrong password must not authenticate");
        }
        Err(error) => error.to_string(),
    };
    println!("rejected: {message}");
    // The message has to name the account, or it is useless in a list of ten.
    assert!(message.contains(&config.username));
}

#[tokio::test]
async fn authenticates_and_reports_how_it_would_elevate() {
    let Some(config) = config() else {
        eprintln!("skipping: PAROLASSH_LIVE_HOST not set");
        return;
    };

    let session = connect(&config).await;

    let report = power::check_privileges(&session).await.unwrap();
    println!(
        "os={:?} detail={:?} user={:?}\nelevation={:?}\n{}",
        report.os, report.os_detail, report.user, report.elevation, report.explanation
    );

    assert_ne!(report.os, OsFamily::Unknown, "the OS should be identifiable");
    assert!(
        report.elevation.is_usable(),
        "no route to elevation: {:?}",
        report.elevation
    );

    session.close().await;
}

#[tokio::test]
async fn runs_a_command_and_captures_both_streams() {
    let Some(config) = config() else {
        eprintln!("skipping: PAROLASSH_LIVE_HOST not set");
        return;
    };

    let session = connect(&config).await;

    let output = session
        .exec("echo to-stdout; echo to-stderr >&2; exit 3", None)
        .await
        .unwrap();

    assert_eq!(output.stdout.trim(), "to-stdout");
    assert_eq!(output.stderr.trim(), "to-stderr");
    assert_eq!(output.exit_code, Some(3));
    assert!(!output.succeeded());

    session.close().await;
}

#[tokio::test]
async fn schedules_a_reboot_over_sudo_then_cancels_it() {
    let Some(config) = config() else {
        eprintln!("skipping: PAROLASSH_LIVE_HOST not set");
        return;
    };

    let session = connect(&config).await;
    let report = power::check_privileges(&session).await.unwrap();

    // Far enough out that a failure to cancel is still harmless.
    let scheduled = PowerRequest {
        action: PowerAction::Reboot,
        delay_minutes: 600,
        force: false,
        message: Some("ParolaSSH integration test — will be cancelled".into()),
    };

    let plan = power::plan(report.os, &report.elevation, &scheduled).unwrap();
    println!("plan: {}", plan.command);
    assert_eq!(
        plan.needs_password,
        report.elevation == Elevation::SudoPassword
    );

    let outcome = power::execute(
        &session,
        report.os,
        &report.elevation,
        &scheduled,
        Some(&config.password),
    )
    .await
    .unwrap();

    println!("scheduled: succeeded={} {}", outcome.succeeded, outcome.message);
    assert!(outcome.succeeded, "scheduling failed: {}", outcome.message);

    // Now call it off, which is the half that actually protects the machine.
    let cancel = PowerRequest {
        action: PowerAction::Cancel,
        delay_minutes: 0,
        force: false,
        message: None,
    };

    let cancelled = power::execute(
        &session,
        report.os,
        &report.elevation,
        &cancel,
        Some(&config.password),
    )
    .await
    .unwrap();

    println!("cancelled: succeeded={} {}", cancelled.succeeded, cancelled.message);
    assert!(cancelled.succeeded, "cancel failed: {}", cancelled.message);

    // Prove it: a pending shutdown leaves a scheduled file behind on systemd.
    let check = session
        .exec("test -e /run/systemd/shutdown/scheduled && echo PENDING || echo CLEAR", None)
        .await
        .unwrap();
    println!("after cancel: {}", check.stdout.trim());
    assert!(
        check.stdout.contains("CLEAR"),
        "a reboot is still pending after cancelling"
    );

    session.close().await;
}

#[tokio::test]
async fn a_bad_sudo_password_fails_without_rebooting_anything() {
    let Some(config) = config() else {
        eprintln!("skipping: PAROLASSH_LIVE_HOST not set");
        return;
    };

    let session = connect(&config).await;
    let report = power::check_privileges(&session).await.unwrap();

    if report.elevation != Elevation::SudoPassword {
        eprintln!("skipping: this host does not need a sudo password");
        return;
    }

    let request = PowerRequest {
        action: PowerAction::Reboot,
        delay_minutes: 600,
        force: false,
        message: None,
    };

    let outcome = power::execute(
        &session,
        report.os,
        &report.elevation,
        &request,
        Some("wrong-sudo-password"),
    )
    .await
    .unwrap();

    println!("bad sudo: succeeded={} {}", outcome.succeeded, outcome.message);
    assert!(!outcome.succeeded, "a wrong sudo password must not succeed");
    assert!(
        outcome.message.contains("password"),
        "the failure should name the password: {}",
        outcome.message
    );

    session.close().await;
}

#[tokio::test]
async fn the_login_password_is_reused_for_sudo() {
    let Some(config) = config() else {
        eprintln!("skipping: PAROLASSH_LIVE_HOST not set");
        return;
    };

    let session = connect(&config).await;
    let report = power::check_privileges(&session).await.unwrap();

    if report.elevation != Elevation::SudoPassword {
        eprintln!("skipping: this host does not need a sudo password");
        return;
    }

    // What the UI does when "use the password I logged in with" is ticked:
    // the dialog sends nothing, and the session supplies its own.
    let request = PowerRequest {
        action: PowerAction::Reboot,
        delay_minutes: 600,
        force: false,
        message: None,
    };

    let outcome = power::execute(
        &session,
        report.os,
        &report.elevation,
        &request,
        Some(&config.password),
    )
    .await
    .unwrap();

    assert!(outcome.succeeded, "{}", outcome.message);
    println!("reused login password: {}", outcome.message);

    let cancel = PowerRequest {
        action: PowerAction::Cancel,
        delay_minutes: 0,
        force: false,
        message: None,
    };
    let cancelled = power::execute(
        &session,
        report.os,
        &report.elevation,
        &cancel,
        Some(&config.password),
    )
    .await
    .unwrap();
    assert!(cancelled.succeeded, "{}", cancelled.message);

    // And the liveness probe the heartbeat leans on must agree we are up.
    assert!(session.is_alive().await, "the session should still be alive");

    session.close().await;
}
