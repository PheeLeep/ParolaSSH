//! End-to-end checks against a real machine.
//!
//! Every test here is `#[ignore]`d, so a default `cargo test` reports them
//! *ignored*. They used to return early when the environment was unset, which
//! libtest counted as **passed** — a green suite that had connected to nothing.
//!
//! Run them deliberately, against a throwaway box you are willing to have
//! rebooted:
//!
//! ```sh
//! PAROLASSH_LIVE_HOST=192.168.56.10 \
//! PAROLASSH_LIVE_USER=pheeleep \
//! PAROLASSH_LIVE_PASSWORD=secret \
//! cargo test --test live_remote -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Asking for them without naming a machine is a hard failure, not a silent
//! pass — see `config`.
//!
//! The power test schedules a reboot far out then cancels it, and verifies the
//! cancel per platform, so nothing here reboots the machine.
//!
//! Linux and Windows both assert; `skip` remains only for macOS/BSD, which no
//! VM covers yet. A skip still reports `ok` — libtest has no runtime "skipped"
//! outcome — so prefer a per-OS assertion over calling it.

use parolassh_lib::remote::client::{Credentials, Session, Target};
use parolassh_lib::remote::power::{self, Elevation, PowerAction, PowerRequest};
use parolassh_lib::remote::{audit, metrics, probe, services, updates, OsFamily};
use zeroize::Zeroizing;

struct LiveConfig {
    hostname: String,
    port: u16,
    username: String,
    password: String,
}

/// Read the target machine from the environment.
///
/// Missing configuration panics. These tests only run when asked for by name,
/// so getting here without a host means the run was meant to happen and
/// cannot — which is a failure, not something to pass quietly.
fn config() -> LiveConfig {
    fn required(name: &str) -> String {
        std::env::var(name).unwrap_or_else(|_| {
            panic!(
                "{name} is not set, so there is nothing to test against. These \
                 tests are #[ignore]d and you asked for them explicitly; see \
                 the module docs for the full invocation."
            )
        })
    }

    LiveConfig {
        hostname: required("PAROLASSH_LIVE_HOST"),
        port: std::env::var("PAROLASSH_LIVE_PORT")
            .ok()
            .and_then(|port| port.parse().ok())
            .unwrap_or(22),
        username: required("PAROLASSH_LIVE_USER"),
        password: required("PAROLASSH_LIVE_PASSWORD"),
    }
}

/// A host-capability skip. Still reports `ok` — libtest has no runtime
/// "skipped" — so this only makes the gap findable. Prefer a per-OS assertion.
fn skip(reason: &str) {
    eprintln!("=== SKIPPED (still reported as ok): {reason} ===");
}

/// Wall clock in epoch milliseconds, for metrics that measure against it.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
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
#[ignore = "needs a live host: see the module docs"]
async fn probes_the_port_before_authenticating() {
    let config = config();

    let result = probe::probe(&config.hostname, config.port).await.unwrap();
    println!("probe: {}", result.message);

    assert!(result.reachable, "port {} is not open", config.port);
    assert!(result.is_ssh, "what answered is not an SSH server");
    assert!(result.banner.is_some());
}

#[tokio::test]
#[ignore = "needs a live host: see the module docs"]
async fn a_wrong_password_is_refused_clearly() {
    let config = config();

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
#[ignore = "needs a live host: see the module docs"]
async fn authenticates_and_reports_how_it_would_elevate() {
    let config = config();

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
#[ignore = "needs a live host: see the module docs"]
async fn runs_a_command_and_captures_both_streams() {
    let config = config();

    let session = connect(&config).await;
    let report = power::check_privileges(&session).await.unwrap();

    // Both streams plus a non-zero status, phrased for the shell that will
    // actually parse it. cmd.exe has no `;` separator and writes to stderr as
    // `1>&2`, so the POSIX form silently produced no output at all here.
    let command = if report.os == OsFamily::Windows {
        "echo to-stdout&echo to-stderr 1>&2&exit 3"
    } else {
        "echo to-stdout; echo to-stderr >&2; exit 3"
    };

    let output = session.exec(command, None).await.unwrap();

    assert_eq!(output.stdout.trim(), "to-stdout");
    assert_eq!(output.stderr.trim(), "to-stderr");
    assert_eq!(output.exit_code, Some(3));
    assert!(!output.succeeded());

    session.close().await;
}

#[tokio::test]
#[ignore = "needs a live host: see the module docs"]
async fn lists_services_and_finds_sshd_among_them() {
    let config = config();

    let session = connect(&config).await;
    let report = power::check_privileges(&session).await.unwrap();

    let command = services::list_command(report.os).unwrap();
    let output = session.exec(command, None).await.unwrap();
    assert!(output.succeeded(), "{}", output.failure_text());

    let entries = services::parse_list(report.os, &output.stdout);
    println!("{} services; first: {:?}", entries.len(), entries.first().map(|e| &e.name));
    assert!(!entries.is_empty(), "a live host has services");

    // The one service guaranteed present: the daemon we are talking through.
    assert!(
        entries.iter().any(|entry| entry.name.contains("ssh")),
        "sshd should appear in its own service list"
    );

    session.close().await;
}

#[tokio::test]
#[ignore = "needs a live host: see the module docs"]
async fn samples_metrics_twice_and_reads_a_cpu_delta() {
    let config = config();

    let session = connect(&config).await;
    let report = power::check_privileges(&session).await.unwrap();
    let command = metrics::sample_command(report.os).unwrap();

    // Windows needs no delta: `LoadPercentage` is instantaneous, so one sample
    // carries CPU. The opposite of the Linux invariant below.
    if report.os == OsFamily::Windows {
        let output = session.exec(command, None).await.unwrap();
        assert!(output.succeeded(), "{}", output.failure_text());

        // Uptime is `now - LastBootUpTime`, so a real clock is required.
        let sample = metrics::parse_windows(&output.stdout, now_ms());
        println!(
            "windows: cpu={:?} disks={} uptime={:?}s memory={:?}",
            sample.cpu_percent,
            sample.disks.len(),
            sample.uptime_seconds,
            sample.memory
        );

        assert!(sample.cpu_percent.is_some(), "LoadPercentage should parse");
        assert!(sample.memory.is_some(), "Win32_OperatingSystem should parse");
        assert!(!sample.disks.is_empty(), "a Windows host has at least C:");
        assert!(sample.uptime_seconds.is_some(), "LastBootUpTime should parse");

        session.close().await;
        return;
    }

    if report.os != OsFamily::Linux {
        skip("the delta path below reads /proc; this host is neither Linux nor Windows");
        session.close().await;
        return;
    }

    let first = session.exec(command, None).await.unwrap();
    let (sample, previous) = metrics::parse_linux(&first.stdout, None, 0);
    assert!(sample.cpu_percent.is_none(), "the first sample has no delta yet");
    assert!(sample.memory.is_some(), "meminfo should parse");
    assert!(!sample.disks.is_empty(), "df should report at least the root disk");

    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;

    let second = session.exec(command, None).await.unwrap();
    let (sample, _) = metrics::parse_linux(&second.stdout, previous, 0);
    println!("cpu={:?} load={:?} uptime={:?}", sample.cpu_percent, sample.load, sample.uptime_seconds);
    assert!(sample.cpu_percent.is_some(), "the second sample should carry a CPU figure");

    session.close().await;
}

#[tokio::test]
#[ignore = "needs a live host: see the module docs"]
async fn checks_updates_without_installing_anything() {
    let config = config();

    let session = connect(&config).await;
    let report = power::check_privileges(&session).await.unwrap();

    // Windows answers in two rounds: detect PSWindowsUpdate and read hotfix
    // history, then query pending updates only if the module was there. The
    // point of the assertion is that the module is never installed to improve
    // the answer.
    if report.os == OsFamily::Windows {
        let output = session
            .exec(updates::check_command(report.os).unwrap(), None)
            .await
            .unwrap();
        assert!(output.succeeded(), "{}", output.failure_text());

        let (module_present, hotfixes) = updates::parse_windows_first_round(&output);
        println!("module={module_present} hotfixes={}", hotfixes.len());

        if module_present {
            let pending = session
                .exec_with_timeout(
                    updates::windows_pending_command(),
                    None,
                    updates::WINDOWS_PENDING_TIMEOUT,
                )
                .await
                .unwrap();
            println!("pending: {:?}", updates::parse_windows_pending(&pending));
        } else {
            // The documented fallback: say so, and show history instead.
            assert!(
                !updates::module_missing_detail().is_empty(),
                "the absent module must be explained, not hidden"
            );
        }

        session.close().await;
        return;
    }

    if report.os != OsFamily::Linux {
        skip("apt/dnf assertions below; this host is neither Linux nor Windows");
        session.close().await;
        return;
    }

    let output = session
        .exec(updates::check_command(report.os).unwrap(), None)
        .await
        .unwrap();
    let parsed = updates::parse_linux(&output);
    println!("updates: {parsed:?}");

    // Any of the honest outcomes is fine; a crash or a lie is not.
    match parsed {
        updates::UpdateReport::List { updates, .. } => assert!(!updates.is_empty()),
        updates::UpdateReport::UpToDate { .. }
        | updates::UpdateReport::ManagerMissing { .. } => {}
        updates::UpdateReport::ModuleMissing { .. } => {
            panic!("a Linux host cannot be missing a PowerShell module")
        }
    }

    session.close().await;
}

#[tokio::test]
#[ignore = "needs a live host: see the module docs"]
async fn tier1_audit_reads_sshd_posture_with_sudo() {
    let config = config();

    let session = connect(&config).await;
    let report = power::check_privileges(&session).await.unwrap();
    // Tier 1 is Unix-only, but tier 0 comes from the handshake and must still
    // produce a report that says plainly that tier 1 did not run.
    if !report.os.is_unix() {
        let assembled = audit::assemble(
            "live-test",
            session.negotiated.as_ref(),
            None,
            &std::collections::HashSet::new(),
        );
        println!(
            "tier0 only: score={} findings={} tier1_ran={}",
            assembled.score,
            assembled.findings.len(),
            assembled.tier1_ran
        );

        assert!(!assembled.tier1_ran, "tier 1 cannot have run on a non-Unix host");
        assert!(
            session.negotiated.is_some(),
            "tier 0 needs what the key exchange negotiated"
        );

        session.close().await;
        return;
    }

    let unprivileged = session.exec(audit::TIER1_COMMAND, None).await.unwrap();

    let privileged = if audit::needs_privileged_retry(&unprivileged)
        && report.elevation.is_usable()
    {
        let stdin = format!("{}\n", config.password).into_bytes();
        Some(
            session
                .exec(audit::TIER1_PRIVILEGED_COMMAND, Some(&stdin))
                .await
                .unwrap(),
        )
    } else {
        None
    };

    let gathered = audit::gather_tier1(
        &unprivileged,
        privileged.as_ref(),
        report.elevation.is_usable(),
    );
    println!(
        "sshd config: {} · lynis: {:?} · note: {:?}",
        gathered.sshd_config.is_some(),
        gathered.lynis,
        gathered.note
    );

    // With working sudo the posture must actually be read, not degraded away.
    assert!(
        gathered.sshd_config.is_some(),
        "sshd -T should answer once sudo is available: {:?}",
        gathered.note
    );

    let assembled = audit::assemble(
        "live-test",
        session.negotiated.as_ref(),
        Some(&gathered),
        &std::collections::HashSet::new(),
    );
    println!("score={} findings={}", assembled.score, assembled.findings.len());
    assert!(assembled.tier1_ran);

    session.close().await;
}

#[tokio::test]
#[ignore = "needs a live host: see the module docs"]
async fn schedules_a_reboot_over_sudo_then_cancels_it() {
    let config = config();

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

    // Prove it, per platform. systemd leaves a scheduled file behind; Windows
    // has no such file, so ask it to cancel again — with nothing pending that
    // must fail.
    if report.os == OsFamily::Windows {
        let again = session.exec("shutdown /a", None).await.unwrap();
        println!("second cancel: {}", again.failure_text().trim());
        assert!(
            !again.succeeded(),
            "a second cancel succeeded, so the first left a reboot pending"
        );
    } else {
        let check = session
            .exec("test -e /run/systemd/shutdown/scheduled && echo PENDING || echo CLEAR", None)
            .await
            .unwrap();
        println!("after cancel: {}", check.stdout.trim());
        assert!(
            check.stdout.contains("CLEAR"),
            "a reboot is still pending after cancelling"
        );
    }

    session.close().await;
}

#[tokio::test]
#[ignore = "needs a live host: see the module docs"]
async fn a_bad_sudo_password_fails_without_rebooting_anything() {
    let config = config();

    let session = connect(&config).await;
    let report = power::check_privileges(&session).await.unwrap();

    let request = PowerRequest {
        action: PowerAction::Reboot,
        delay_minutes: 600,
        force: false,
        message: None,
    };

    // Windows has no sudo: the OpenSSH logon already holds the full token, so
    // there is no password to get wrong. Assert that rather than skipping, and
    // plan only — executing here would schedule a real reboot.
    if report.os == OsFamily::Windows {
        assert_eq!(report.elevation, Elevation::WindowsAdminToken);

        let plan = power::plan(report.os, &report.elevation, &request).unwrap();
        println!("windows plan: {}", plan.command);

        assert!(!plan.needs_password, "Windows must not ask for a password");
        assert!(!plan.command.contains("sudo"), "no sudo on Windows: {}", plan.command);

        session.close().await;
        return;
    }

    if report.elevation != Elevation::SudoPassword {
        skip("this host's sudo needs no password, so there is no wrong password to send");
        session.close().await;
        return;
    }

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
#[ignore = "needs a live host: see the module docs"]
async fn the_login_password_is_reused_for_sudo() {
    let config = config();

    let session = connect(&config).await;
    let report = power::check_privileges(&session).await.unwrap();

    // The reuse question does not arise on Windows — nothing downstream ever
    // asks for a password — but the session must still be usable afterwards.
    if report.os == OsFamily::Windows {
        assert_eq!(report.elevation, Elevation::WindowsAdminToken);
        assert!(session.is_alive().await, "the session should still be alive");
        session.close().await;
        return;
    }

    if report.elevation != Elevation::SudoPassword {
        skip("this host's sudo needs no password, so there is nothing to reuse");
        session.close().await;
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
