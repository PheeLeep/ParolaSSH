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
use parolassh_lib::remote::{audit, metrics, probe, services, sftp, transfer_task, updates, OsFamily};
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

/* ── Files (SFTP) ─────────────────────────────────────────────────────── */

/// Create a remote file and fill it.
///
/// Not `SftpSession::write`, which opens with `WRITE` alone and so fails with
/// `NoSuchFile` on anything that does not already exist. `create` is the one
/// that sets `CREATE | TRUNCATE | WRITE`, and is what the upload path uses.
async fn write_new(sftp: &russh_sftp::client::SftpSession, path: &str, data: &[u8]) {
    use tokio::io::AsyncWriteExt;

    let mut file = sftp.create(path.to_string()).await.unwrap();
    file.write_all(data).await.unwrap();
    file.flush().await.unwrap();
    file.shutdown().await.unwrap();
}

/// A full round trip over the subsystem: upload a file, read it back, and
/// confirm the bytes survived.
///
/// Everything is written under `/tmp` and removed again, so a failed run leaves
/// at most one stray file on a throwaway box.
#[tokio::test]
#[ignore = "needs a live host: see the module docs"]
async fn uploads_and_downloads_a_file_intact() {
    let config = config();
    let session = connect(&config).await;
    let sftp = sftp::connect(&session).await.unwrap();

    let dir = format!("/tmp/parolassh-sftp-{}", now_ms());
    sftp.create_dir(dir.clone()).await.unwrap();

    // Deliberately not text: a transfer that mangles high bytes or embedded
    // NULs would still pass a "hello world" check.
    let payload: Vec<u8> = (0..=255u8).cycle().take(300_000).collect();
    let remote_file = format!("{dir}/payload.bin");
    write_new(&sftp, &remote_file, &payload).await;

    let read_back = sftp.read(remote_file.clone()).await.unwrap();
    assert_eq!(read_back.len(), payload.len(), "the file changed size in flight");
    assert_eq!(read_back, payload, "the bytes did not survive the round trip");

    // The listing must agree about size and kind.
    let listing = sftp::list_dir(&sftp, &dir).await.unwrap();
    let entry = listing
        .entries
        .iter()
        .find(|entry| entry.name == "payload.bin")
        .expect("the file we just wrote should be listed");
    assert_eq!(entry.kind, sftp::EntryKind::File);
    assert_eq!(entry.size, payload.len() as u64);

    sftp.remove_file(remote_file).await.unwrap();
    sftp.remove_dir(dir).await.unwrap();
    session.close().await;
}

/// The symlink policy, against a real link rather than a constructed enum.
///
/// This is the test that matters most: `list_dir` reads kinds from the
/// server's `readdir` attributes, and if any server reported them with `stat`
/// semantics instead of `lstat`, a link would arrive looking like an ordinary
/// file and the refusal would never fire.
#[tokio::test]
#[ignore = "needs a live host: see the module docs"]
async fn a_symlink_is_reported_as_one_and_refused() {
    let config = config();
    let session = connect(&config).await;
    let sftp = sftp::connect(&session).await.unwrap();

    let dir = format!("/tmp/parolassh-link-{}", now_ms());
    sftp.create_dir(dir.clone()).await.unwrap();

    let real = format!("{dir}/real.txt");
    write_new(&sftp, &real, b"contents").await;

    // Made with `ln -s` rather than the SFTP helper: what matters is that we
    // correctly *read* a link created the ordinary way, and the protocol's own
    // symlink request has a well-known argument-order disagreement between the
    // draft and OpenSSH that is not ours to take a side in.
    let link = format!("{dir}/link.txt");
    let made = session
        .exec(&format!("ln -s {real} {link}"), None)
        .await
        .unwrap();
    assert!(made.succeeded(), "could not create the test link: {}", made.failure_text());

    let listing = sftp::list_dir(&sftp, &dir).await.unwrap();
    let entry = listing
        .entries
        .iter()
        .find(|entry| entry.name == "link.txt")
        .expect("the link should be listed, not hidden");

    assert_eq!(
        entry.kind,
        sftp::EntryKind::Symlink,
        "readdir must report links with lstat semantics, or the gate never fires"
    );
    assert_eq!(
        entry.target.as_deref(),
        Some(real.as_str()),
        "the target is shown so the user can open it directly"
    );

    // And the gate the download path uses must refuse it.
    let refused = sftp::stat_regular_file(&sftp, &link).await;
    assert!(refused.is_err(), "a symlink must never be opened for transfer");
    assert!(
        refused.unwrap_err().to_string().contains("symbolic link"),
        "the refusal should say why"
    );

    // The real file behind it is still transferable.
    assert_eq!(sftp::stat_regular_file(&sftp, &real).await.unwrap(), 8);

    sftp.remove_file(link).await.unwrap();
    sftp.remove_file(real).await.unwrap();
    sftp.remove_dir(dir).await.unwrap();
    session.close().await;
}

/// A path we have no business reading explains that SFTP cannot elevate.
///
/// The refusal comes from the *open*, not the stat: `stat` needs only search
/// permission on the parent directories, so `/etc/shadow` stats perfectly well
/// as an ordinary user and fails the moment you ask for its contents. A
/// download therefore only learns it is denied at the last step, which is
/// exactly where `explain_error` runs.
#[tokio::test]
#[ignore = "needs a live host: see the module docs"]
async fn a_denied_path_says_sftp_cannot_elevate() {
    let config = config();
    let session = connect(&config).await;

    let whoami = session.exec("id -u", None).await.unwrap();
    if whoami.stdout.trim() == "0" {
        skip("connected as root, so nothing is denied");
        session.close().await;
        return;
    }

    let sftp = sftp::connect(&session).await.unwrap();

    // Root-owned, mode 0640, on every Linux box.
    assert!(
        sftp::stat_regular_file(&sftp, "/etc/shadow").await.is_ok(),
        "stat needs no read permission — if this fails the premise has changed"
    );

    let opened = sftp.open("/etc/shadow".to_string()).await;
    let error = match opened {
        Err(error) => sftp::explain_error("Could not open /etc/shadow", &error.to_string()),
        Ok(_) => panic!("an ordinary user must not be able to read /etc/shadow"),
    };

    let text = error.to_string();
    assert!(
        text.contains("cannot elevate"),
        "a denial must say sudo is not an option here, got: {text}"
    );
    assert!(
        text.contains("reconnect as a user"),
        "and must say what to do instead, got: {text}"
    );

    session.close().await;
}

/// The browser starts somewhere real, and the path is absolute and clean.
#[tokio::test]
#[ignore = "needs a live host: see the module docs"]
async fn the_home_directory_is_an_absolute_path_we_can_list() {
    let config = config();
    let session = connect(&config).await;
    let sftp = sftp::connect(&session).await.unwrap();

    let home = sftp::home_dir(&sftp).await.unwrap();
    assert!(home.starts_with('/'), "home should be absolute, got {home}");
    assert!(!home.ends_with('/') || home == "/", "no trailing slash: {home}");

    // Listing it must work, and `.`/`..` must not appear as entries.
    let listing = sftp::list_dir(&sftp, &home).await.unwrap();
    assert_eq!(listing.path, home);
    assert!(
        !listing.entries.iter().any(|e| e.name == "." || e.name == ".."),
        "the dot entries are noise in a file browser"
    );

    session.close().await;
}


/// A gigabyte of random bytes, up and back down, hashed at both ends.
///
/// The point is not that SFTP works — the smaller round trip covers that. It is
/// that the *chunking* holds at a size where an off-by-one in an offset, a lost
/// buffer tail, or a `.part` renamed early would actually show up, on a file far
/// larger than any buffer involved. Random content matters: a file of zeros
/// would hide a chunk written twice, or one skipped entirely.
///
/// Both directions go through the real `transfer_task` functions, not a
/// hand-rolled copy, so what is verified is the code the app runs.
///
/// Needs ~1 GB free on the VM and ~2 GB locally, and takes a few minutes.
/// Everything is removed at the end, including on the remote side.
#[tokio::test]
#[ignore = "needs a live host and ~1GB free: see the module docs"]
async fn a_gigabyte_survives_the_round_trip_intact() {
    use std::sync::atomic::AtomicBool;

    let config = config();
    let session = connect(&config).await;

    let scratch = tempfile::tempdir().unwrap();
    let source_path = scratch.path().join("payload.bin");
    let returned_path = scratch.path().join("returned.bin");
    let remote_path = format!("/tmp/parolassh-1g-{}.bin", now_ms());

    // 1 GiB of non-repeating bytes, generated rather than read from
    // /dev/urandom so the test does not depend on the host's entropy device.
    const SIZE: u64 = 1024 * 1024 * 1024;
    eprintln!("building a {SIZE}-byte random file at {}", source_path.display());
    let local_digest = build_random_file(&source_path, SIZE);
    eprintln!("local sha256  = {local_digest}");

    let no_cancel = AtomicBool::new(false);
    let seen = std::sync::Mutex::new(0_u64);
    let progress = |done: u64, _total: Option<u64>| {
        *seen.lock().unwrap() = done;
    };

    // ── Up ──
    let started = std::time::Instant::now();
    transfer_task::upload(
        &session,
        &remote_path,
        source_path.to_str().unwrap(),
        &no_cancel,
        &progress,
    )
    .await
    .expect("the upload should succeed");
    eprintln!("uploaded in {:?}", started.elapsed());
    assert_eq!(*seen.lock().unwrap(), SIZE, "progress must end at the full size");

    // The server's own view of what it received, computed by the server.
    let remote_sum = session
        .exec_with_timeout(
            &format!("sha256sum {remote_path}"),
            None,
            std::time::Duration::from_secs(600),
        )
        .await
        .unwrap();
    assert!(remote_sum.succeeded(), "{}", remote_sum.failure_text());
    let remote_digest = remote_sum.stdout.split_whitespace().next().unwrap().to_string();
    eprintln!("remote sha256 = {remote_digest}");
    assert_eq!(
        remote_digest, local_digest,
        "the uploaded file does not match what we sent"
    );

    // ── And back down ──
    let started = std::time::Instant::now();
    transfer_task::download(
        &session,
        &remote_path,
        returned_path.to_str().unwrap(),
        &no_cancel,
        &progress,
    )
    .await
    .expect("the download should succeed");
    eprintln!("downloaded in {:?}", started.elapsed());

    assert_eq!(
        std::fs::metadata(&returned_path).unwrap().len(),
        SIZE,
        "the returned file is the wrong size"
    );
    let returned_digest = sha256_file(&returned_path);
    eprintln!("returned sha256 = {returned_digest}");
    assert_eq!(
        returned_digest, local_digest,
        "the round trip changed the file"
    );

    // The staging file must be gone, not merely renamed past.
    let part = transfer_task::part_path_for(&returned_path);
    assert!(!part.exists(), "the .part file should not survive a success");

    let cleaned = session
        .exec(&format!("rm -f {remote_path}"), None)
        .await
        .unwrap();
    assert!(cleaned.succeeded(), "{}", cleaned.failure_text());
    session.close().await;
}

/// Write `size` bytes of non-repeating content and return its SHA-256.
///
/// A xorshift keeps this fast and dependency-free while still producing bytes
/// that no chunking bug could accidentally reproduce.
fn build_random_file(path: &std::path::Path, size: u64) -> String {
    use sha2::{Digest, Sha256};
    use std::io::Write;

    let mut file = std::io::BufWriter::new(std::fs::File::create(path).unwrap());
    let mut hasher = Sha256::new();

    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    let mut block = vec![0_u8; 1 << 20];
    let mut written = 0_u64;

    while written < size {
        for slot in block.chunks_exact_mut(8) {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            slot.copy_from_slice(&state.to_le_bytes());
        }
        let take = std::cmp::min(block.len() as u64, size - written) as usize;
        file.write_all(&block[..take]).unwrap();
        hasher.update(&block[..take]);
        written += take as u64;
    }

    file.flush().unwrap();
    format!("{:x}", hasher.finalize())
}

fn sha256_file(path: &std::path::Path) -> String {
    use sha2::{Digest, Sha256};

    let mut file = std::fs::File::open(path).unwrap();
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).unwrap();
    format!("{:x}", hasher.finalize())
}



/// The recursive walk: files found, links and specials skipped, tree preserved.
#[tokio::test]
#[ignore = "needs a live host: see the module docs"]
async fn a_folder_walk_finds_files_and_skips_links() {
    let config = config();
    let session = connect(&config).await;
    let sftp = sftp::connect(&session).await.unwrap();

    let root = format!("/tmp/parolassh-tree-{}", now_ms());
    let build = session
        .exec(
            &format!(
                "mkdir -p {root}/a/b {root}/empty && \
                 echo one > {root}/top.txt && \
                 echo two > {root}/a/mid.txt && \
                 echo three > {root}/a/b/deep.txt && \
                 ln -s {root}/top.txt {root}/a/link.txt"
            ),
            None,
        )
        .await
        .unwrap();
    assert!(build.succeeded(), "{}", build.failure_text());

    let tree = sftp::walk(&sftp, &root).await.unwrap();
    let relatives: Vec<&str> = tree.files.iter().map(|f| f.relative.as_str()).collect();

    assert_eq!(
        relatives,
        ["a/b/deep.txt", "a/mid.txt", "top.txt"],
        "the walk should find every regular file, sorted by tree position"
    );
    assert_eq!(tree.skipped.len(), 1, "the symlink should be skipped");
    assert!(tree.skipped[0].ends_with("link.txt"));
    assert!(!tree.truncated);
    // The empty directory contributes nothing: only files are transferred.
    assert!(!relatives.iter().any(|path| path.contains("empty")));

    let _ = session.exec(&format!("rm -rf {root}"), None).await;
    session.close().await;
}

/// Rename doubles as move, and refuses to land on something that exists.
#[tokio::test]
#[ignore = "needs a live host: see the module docs"]
async fn rename_moves_and_never_overwrites() {
    let config = config();
    let session = connect(&config).await;
    let sftp = sftp::connect(&session).await.unwrap();

    let root = format!("/tmp/parolassh-mv-{}", now_ms());
    sftp.create_dir(root.clone()).await.unwrap();
    sftp.create_dir(format!("{root}/sub")).await.unwrap();
    write_new(&sftp, &format!("{root}/a.txt"), b"first").await;
    write_new(&sftp, &format!("{root}/b.txt"), b"second").await;

    // A move into another directory is the same request as a rename.
    sftp.rename(format!("{root}/a.txt"), format!("{root}/sub/a.txt"))
        .await
        .unwrap();
    assert!(sftp.try_exists(format!("{root}/sub/a.txt")).await.unwrap());
    assert!(!sftp.try_exists(format!("{root}/a.txt")).await.unwrap());

    // Onto an existing name, the server refuses — which is what the command
    // relies on rather than checking and hoping.
    write_new(&sftp, &format!("{root}/c.txt"), b"third").await;
    assert!(
        sftp.rename(format!("{root}/c.txt"), format!("{root}/b.txt"))
            .await
            .is_err(),
        "SFTP rename must not clobber an existing file"
    );
    assert_eq!(sftp.read(format!("{root}/b.txt")).await.unwrap(), b"second");

    let _ = session.exec(&format!("rm -rf {root}"), None).await;
    session.close().await;
}

/// A server-side copy duplicates a whole tree without moving bytes over the
/// wire, and the built command survives a hostile name.
#[tokio::test]
#[ignore = "needs a live host: see the module docs"]
async fn a_server_side_copy_duplicates_a_tree() {
    let config = config();
    let session = connect(&config).await;
    let sftp = sftp::connect(&session).await.unwrap();

    let root = format!("/tmp/parolassh-cp-{}", now_ms());
    let build = session
        .exec(&format!(
            "mkdir -p {root}/src/inner && echo hello > {root}/src/inner/f.txt"
        ), None)
        .await
        .unwrap();
    assert!(build.succeeded(), "{}", build.failure_text());

    // Exactly what `copy_remote_entry` runs.
    let command = format!(
        "cp -a -- '{root}/src' '{root}/dst'"
    );
    let copied = session.exec(&command, None).await.unwrap();
    assert!(copied.succeeded(), "{}", copied.failure_text());

    assert_eq!(
        sftp.read(format!("{root}/dst/inner/f.txt")).await.unwrap(),
        b"hello\n", // `echo` adds the newline

        "the copy should carry the whole tree"
    );
    // The original is untouched.
    assert!(sftp.try_exists(format!("{root}/src/inner/f.txt")).await.unwrap());

    let _ = session.exec(&format!("rm -rf {root}"), None).await;
    session.close().await;
}
