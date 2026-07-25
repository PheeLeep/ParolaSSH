//! Shutting down and rebooting a remote machine.
//!
//! # Why this is not one command string
//!
//! Every family spells this differently, and the differences are not cosmetic:
//!
//! | | reboot now | reboot in 10 min | cancel |
//! |---|---|---|---|
//! | Linux/BSD | `shutdown -r now` | `shutdown -r +10` | `shutdown -c` |
//! | macOS | `shutdown -r now` | `shutdown -r +10` | `killall shutdown` |
//! | Windows | `shutdown /r /t 0` | `shutdown /r /t 600` | `shutdown /a` |
//!
//! Unix counts the delay in **minutes**, Windows in **seconds**. macOS has no
//! `-c`. So the remote OS is detected first, and the command is built for it.
//!
//! # Privilege: sudo vs UAC
//!
//! These two look similar and are not.
//!
//! *Unix* elevation is a **credential** check. `sudo -S` reads the password
//! from stdin, so it works fine over a non-interactive SSH channel: we send
//! the password down the channel we already have. The password never appears
//! in the command line, so it stays out of the remote process list.
//!
//! *Windows* elevation is a **token** decision made at logon, and UAC's
//! consent prompt is drawn on the interactive desktop — a session that has no
//! desktop cannot answer it. There is no `sudo -S` equivalent: `runas` needs a
//! console, and Windows 11's `sudo` just triggers the same undismissable
//! prompt. What saves us is that OpenSSH on Windows does not apply UAC
//! filtering to its logons: **an account in Administrators arrives already
//! holding its full token**, so `shutdown /r` simply works. A standard user
//! cannot be elevated at all over SSH, and this module says so plainly instead
//! of returning "Access is denied.(5)".

use serde::{Deserialize, Serialize};

use super::client::Session;
use super::{CommandOutput, OsFamily};
use crate::ssh::{SshError, SshResult};

/// What to do to the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PowerAction {
    Shutdown,
    Reboot,
    /// Call off a shutdown or reboot that has not fired yet.
    Cancel,
}

/// A power request from the UI.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerRequest {
    pub action: PowerAction,
    /// Minutes to wait. `0` means immediately. Ignored by `Cancel`.
    #[serde(default)]
    pub delay_minutes: u32,
    /// Windows only: close applications without waiting for them to agree.
    #[serde(default)]
    pub force: bool,
    /// Wall message shown to logged-in users.
    #[serde(default)]
    pub message: Option<String>,
}

/// Longest delay we will accept, in minutes (about a year).
///
/// Windows caps `/t` at 315360000 seconds; this stays well inside that and
/// rejects a fat-fingered delay before it reaches the remote host.
const MAX_DELAY_MINUTES: u32 = 525_600;

/// How this account will gain the privilege to power the machine off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Elevation {
    /// Already root. Nothing to do.
    NotNeeded,
    /// `sudo` is configured NOPASSWD for this command.
    SudoNoPassword,
    /// `sudo` will need the account password, sent over stdin.
    SudoPassword,
    /// Windows: the SSH logon already carries the full administrator token.
    WindowsAdminToken,
    /// No route to elevation. `reason` is shown to the user verbatim.
    Unavailable { reason: String },
}

impl Elevation {
    pub fn is_usable(&self) -> bool {
        !matches!(self, Self::Unavailable { .. })
    }

    /// Whether the account password must be supplied to run the command.
    pub fn needs_password(&self) -> bool {
        matches!(self, Self::SudoPassword)
    }
}

/// What the UI shows before arming the button.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivilegeReport {
    pub os: OsFamily,
    pub os_detail: String,
    pub user: String,
    pub elevation: Elevation,
    /// A plain-language sentence explaining the elevation decision.
    pub explanation: String,
    /// Whether `force` does anything on this OS.
    pub supports_force: bool,
    /// Whether a pending shutdown can be called off.
    pub supports_cancel: bool,
}

/// Detect the remote OS.
///
/// `uname -s` answers on every Unix. On Windows it is not a command, so the
/// `||` branch runs and `ver` reports the Windows version — this works whether
/// sshd hands us cmd.exe or PowerShell, because both understand `||` here.
pub async fn detect_os(session: &Session) -> SshResult<(OsFamily, String)> {
    let output = session.exec("uname -s 2>/dev/null || ver", None).await?;
    let text = format!("{} {}", output.stdout.trim(), output.stderr.trim());
    Ok((classify_os(&text), text.trim().to_string()))
}

/// Map the probe output onto a family.
fn classify_os(text: &str) -> OsFamily {
    let lower = text.to_lowercase();

    // Check Windows first: its output mentions "Windows", never "Linux".
    if lower.contains("windows") || lower.contains("microsoft") {
        return OsFamily::Windows;
    }
    if lower.contains("darwin") {
        return OsFamily::Macos;
    }
    if lower.contains("linux") {
        return OsFamily::Linux;
    }
    if lower.contains("bsd") || lower.contains("dragonfly") {
        return OsFamily::Bsd;
    }
    OsFamily::Unknown
}

/// Work out how — or whether — this account can power the machine down.
pub async fn check_privileges(session: &Session) -> SshResult<PrivilegeReport> {
    let (os, os_detail) = detect_os(session).await?;

    match os {
        OsFamily::Windows => check_windows_privileges(session, os_detail).await,
        family if family.is_unix() => check_unix_privileges(session, family, os_detail).await,
        _ => Ok(PrivilegeReport {
            os,
            os_detail,
            user: String::new(),
            elevation: Elevation::Unavailable {
                reason: "The remote operating system could not be identified, so no \
                         power command can be chosen safely."
                    .into(),
            },
            explanation: "Neither `uname` nor `ver` gave a usable answer. This is \
                          normal for network appliances and restricted shells."
                .into(),
            supports_force: false,
            supports_cancel: false,
        }),
    }
}

async fn check_unix_privileges(
    session: &Session,
    os: OsFamily,
    os_detail: String,
) -> SshResult<PrivilegeReport> {
    // One round trip: who am I, am I root, and does sudo need a password?
    // `sudo -n true` fails without prompting when a password would be needed.
    let probe = session
        .exec(
            "id -un; id -u; command -v sudo >/dev/null 2>&1 && \
             { sudo -n true 2>/dev/null && echo SUDO_NOPASSWD || echo SUDO_PASSWD; } \
             || echo SUDO_MISSING",
            None,
        )
        .await?;

    let mut lines = probe.stdout.lines().map(str::trim);
    let user = lines.next().unwrap_or("").to_string();
    let uid = lines.next().unwrap_or("").to_string();
    let sudo = lines.next().unwrap_or("SUDO_MISSING").to_string();

    let (elevation, explanation) = if uid == "0" {
        (
            Elevation::NotNeeded,
            "You are connected as root, so the power command runs directly.".to_string(),
        )
    } else {
        match sudo.as_str() {
            "SUDO_NOPASSWD" => (
                Elevation::SudoNoPassword,
                format!("`{user}` may run sudo without a password, so no prompt is needed."),
            ),
            "SUDO_PASSWD" => (
                Elevation::SudoPassword,
                format!(
                    "`{user}` needs a password for sudo. It is sent over the existing \
                     encrypted channel to `sudo -S`, never as part of the command line, \
                     so it stays out of the remote process list."
                ),
            ),
            _ => (
                Elevation::Unavailable {
                    reason: format!(
                        "`sudo` is not installed on this host and `{user}` is not root."
                    ),
                },
                "Connect as root, or install and configure sudo for this account."
                    .to_string(),
            ),
        }
    };

    Ok(PrivilegeReport {
        os,
        os_detail,
        user,
        elevation,
        // `shutdown` on Unix has no force flag; the delay is the safety valve.
        supports_force: false,
        // macOS `shutdown` has no `-c`, so a pending job is killed instead.
        supports_cancel: true,
        explanation,
    })
}

async fn check_windows_privileges(
    session: &Session,
    os_detail: String,
) -> SshResult<PrivilegeReport> {
    // `net session` needs administrator rights, so its exit status is a
    // reliable read of whether this token is elevated — which over SSH is
    // decided at logon, not by any prompt we could answer.
    let probe = session
        .exec(
            "whoami & net session >nul 2>&1 && echo ELEVATED || echo LIMITED",
            None,
        )
        .await?;

    let text = probe.stdout.replace('\r', "");
    let mut lines = text.lines().map(str::trim).filter(|line| !line.is_empty());
    let user = lines.next().unwrap_or("").to_string();
    let elevated = text.contains("ELEVATED");

    let (elevation, explanation) = if elevated {
        (
            Elevation::WindowsAdminToken,
            format!(
                "`{user}` is an administrator. Windows OpenSSH does not apply UAC \
                 filtering to its logons, so this session already holds the full \
                 token and `shutdown` runs without any consent prompt."
            ),
        )
    } else {
        (
            Elevation::Unavailable {
                reason: format!("`{user}` is a standard user, and UAC cannot be answered over SSH."),
            },
            "UAC's consent prompt is drawn on the interactive desktop, which an SSH \
             session does not have — so unlike sudo, there is no password we could \
             send to elevate. Connect as a member of Administrators, or grant this \
             account the “Force shutdown from a remote system” right."
                .to_string(),
        )
    };

    Ok(PrivilegeReport {
        os: OsFamily::Windows,
        os_detail,
        user,
        elevation,
        // `/f` closes applications without waiting for them to save.
        supports_force: true,
        supports_cancel: true,
        explanation,
    })
}

/// The exact command that will run, plus whether it needs a password on stdin.
///
/// Returned to the UI before anything is executed: showing the literal string
/// is the difference between trusting a button and knowing what it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerPlan {
    pub command: String,
    pub needs_password: bool,
    pub summary: String,
}

/// Build the command for a request. Pure, so it is exhaustively unit-tested
/// below rather than against a machine we would have to keep rebooting.
pub fn plan(
    os: OsFamily,
    elevation: &Elevation,
    request: &PowerRequest,
) -> SshResult<PowerPlan> {
    if let Elevation::Unavailable { reason } = elevation {
        return Err(SshError::invalid(format!(
            "This account cannot power the machine off: {reason}"
        )));
    }

    if request.delay_minutes > MAX_DELAY_MINUTES {
        return Err(SshError::invalid(format!(
            "A delay of {} minutes is longer than the one-year maximum.",
            request.delay_minutes
        )));
    }

    let command = match os {
        OsFamily::Windows => windows_command(request),
        OsFamily::Macos => unix_command(request, true),
        family if family.is_unix() => unix_command(request, false),
        _ => {
            return Err(SshError::unsupported(
                "The remote operating system is unknown, so no power command can be \
                 chosen safely.",
            ))
        }
    };

    let needs_password = elevation.needs_password();
    let command = if needs_password || elevation == &Elevation::SudoNoPassword {
        // `-S` reads the password from stdin; `-p ''` suppresses the prompt so
        // it does not end up mixed into the command's output.
        format!("sudo -S -p '' {command}")
    } else {
        command
    };

    Ok(PowerPlan {
        summary: summarise(os, request),
        command,
        needs_password,
    })
}

fn unix_command(request: &PowerRequest, is_macos: bool) -> String {
    match request.action {
        // macOS `shutdown` has no `-c`, so the scheduled process is killed.
        PowerAction::Cancel if is_macos => "killall shutdown".to_string(),
        PowerAction::Cancel => "shutdown -c".to_string(),
        action => {
            // Unix counts the delay in minutes; `now` is the idiomatic zero.
            let when = if request.delay_minutes == 0 {
                "now".to_string()
            } else {
                format!("+{}", request.delay_minutes)
            };
            let flag = if action == PowerAction::Reboot { "-r" } else { "-h" };

            match request.message.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
                Some(message) => {
                    format!("shutdown {flag} {when} {}", single_quote(message))
                }
                None => format!("shutdown {flag} {when}"),
            }
        }
    }
}

fn windows_command(request: &PowerRequest) -> String {
    match request.action {
        PowerAction::Cancel => "shutdown /a".to_string(),
        action => {
            let flag = if action == PowerAction::Reboot { "/r" } else { "/s" };
            // Windows counts the delay in seconds, not minutes.
            let seconds = request.delay_minutes as u64 * 60;
            let mut command = format!("shutdown {flag} /t {seconds}");

            if request.force {
                command.push_str(" /f");
            }
            if let Some(message) = request
                .message
                .as_deref()
                .map(str::trim)
                .filter(|m| !m.is_empty())
            {
                command.push_str(&format!(" /c {}", double_quote(message)));
            }
            command
        }
    }
}

fn summarise(os: OsFamily, request: &PowerRequest) -> String {
    match request.action {
        PowerAction::Cancel => format!("Cancel the pending shutdown on this {} host", os.label()),
        action => {
            let verb = if action == PowerAction::Reboot { "Reboot" } else { "Shut down" };
            match request.delay_minutes {
                0 => format!("{verb} immediately"),
                1 => format!("{verb} in 1 minute"),
                minutes => format!("{verb} in {minutes} minutes"),
            }
        }
    }
}

/// Wrap for a POSIX shell. An embedded `'` is closed, escaped, and reopened —
/// the standard trick, and the reason a message cannot break out of the quotes.
fn single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

/// Wrap for cmd.exe, which has no escape for `"` inside a quoted string.
/// Dropping the character is the only safe option; `&`, `|` and friends are
/// inert once quoted.
fn double_quote(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "'"))
}

/// Run a power request against a live session.
///
/// `password` is only read when the plan says a password is needed, and is
/// written to the command's stdin rather than interpolated into it.
pub async fn execute(
    session: &Session,
    os: OsFamily,
    elevation: &Elevation,
    request: &PowerRequest,
    password: Option<&str>,
) -> SshResult<PowerOutcome> {
    let plan = plan(os, elevation, request)?;

    let stdin = if plan.needs_password {
        let password = password.ok_or_else(|| {
            SshError::invalid("This host needs your account password for sudo.")
        })?;
        Some(format!("{password}\n").into_bytes())
    } else {
        None
    };

    let output = session.exec(&plan.command, stdin.as_deref()).await?;
    interpret(&plan, request, output)
}

/// The result of a power request, as the UI reports it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PowerOutcome {
    pub command: String,
    pub summary: String,
    pub succeeded: bool,
    pub message: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<u32>,
}

/// Decide whether a power command worked.
///
/// An immediate reboot is a special case: sshd dies mid-command, so a missing
/// exit status is the *expected* outcome and must not be reported as failure.
fn interpret(
    plan: &PowerPlan,
    request: &PowerRequest,
    output: CommandOutput,
) -> SshResult<PowerOutcome> {
    let immediate = request.action != PowerAction::Cancel && request.delay_minutes == 0;
    let died_mid_command = output.exit_code.is_none();

    let succeeded = output.succeeded() || (immediate && died_mid_command);

    let message = if !succeeded {
        let text = output.failure_text();
        // The most common failure by far, and the least self-explanatory.
        if text.contains("incorrect password") || text.contains("Sorry, try again") {
            "sudo rejected the password.".to_string()
        } else if text.contains("Access is denied") {
            "Windows refused the request: this account lacks shutdown rights on that machine."
                .to_string()
        } else {
            text
        }
    } else if immediate && died_mid_command {
        "The command was accepted and the connection dropped, which is what a \
         successful immediate reboot looks like."
            .to_string()
    } else if request.action == PowerAction::Cancel {
        "Any pending shutdown has been cancelled.".to_string()
    } else {
        let scheduled = output.stdout.trim();
        if scheduled.is_empty() {
            format!("{} — scheduled.", plan.summary)
        } else {
            scheduled.to_string()
        }
    };

    Ok(PowerOutcome {
        command: plan.command.clone(),
        summary: plan.summary.clone(),
        succeeded,
        message,
        stdout: output.stdout,
        stderr: output.stderr,
        exit_code: output.exit_code,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(action: PowerAction, delay_minutes: u32) -> PowerRequest {
        PowerRequest {
            action,
            delay_minutes,
            force: false,
            message: None,
        }
    }

    fn command_for(os: OsFamily, elevation: &Elevation, request: &PowerRequest) -> String {
        plan(os, elevation, request).unwrap().command
    }

    #[test]
    fn detects_the_os_from_probe_output() {
        assert_eq!(classify_os("Linux"), OsFamily::Linux);
        assert_eq!(classify_os("Darwin"), OsFamily::Macos);
        assert_eq!(classify_os("FreeBSD"), OsFamily::Bsd);
        assert_eq!(classify_os("OpenBSD"), OsFamily::Bsd);
        // What `ver` prints when `uname` is not a command.
        assert_eq!(
            classify_os("Microsoft Windows [Version 10.0.22631.4317]"),
            OsFamily::Windows
        );
        // cmd.exe's complaint about `uname` also names Windows; either way the
        // answer is the same, which is why Windows is tested first.
        assert_eq!(
            classify_os("'uname' is not recognized as an internal or external command"),
            OsFamily::Unknown
        );
        assert_eq!(classify_os(""), OsFamily::Unknown);
    }

    #[test]
    fn linux_uses_minutes_and_root_needs_no_sudo() {
        let root = Elevation::NotNeeded;

        assert_eq!(
            command_for(OsFamily::Linux, &root, &request(PowerAction::Reboot, 0)),
            "shutdown -r now"
        );
        assert_eq!(
            command_for(OsFamily::Linux, &root, &request(PowerAction::Reboot, 10)),
            "shutdown -r +10"
        );
        assert_eq!(
            command_for(OsFamily::Linux, &root, &request(PowerAction::Shutdown, 5)),
            "shutdown -h +5"
        );
        assert_eq!(
            command_for(OsFamily::Linux, &root, &request(PowerAction::Cancel, 0)),
            "shutdown -c"
        );
    }

    #[test]
    fn windows_uses_seconds_and_never_gets_sudo() {
        let token = Elevation::WindowsAdminToken;

        assert_eq!(
            command_for(OsFamily::Windows, &token, &request(PowerAction::Reboot, 0)),
            "shutdown /r /t 0"
        );
        // 10 minutes must become 600 seconds, not 10.
        assert_eq!(
            command_for(OsFamily::Windows, &token, &request(PowerAction::Reboot, 10)),
            "shutdown /r /t 600"
        );
        assert_eq!(
            command_for(OsFamily::Windows, &token, &request(PowerAction::Shutdown, 1)),
            "shutdown /s /t 60"
        );
        assert_eq!(
            command_for(OsFamily::Windows, &token, &request(PowerAction::Cancel, 0)),
            "shutdown /a"
        );
    }

    #[test]
    fn force_is_windows_only() {
        let mut forced = request(PowerAction::Reboot, 0);
        forced.force = true;

        assert_eq!(
            command_for(OsFamily::Windows, &Elevation::WindowsAdminToken, &forced),
            "shutdown /r /t 0 /f"
        );
        // Unix `shutdown` has no force flag, so the request must not invent one.
        assert_eq!(
            command_for(OsFamily::Linux, &Elevation::NotNeeded, &forced),
            "shutdown -r now"
        );
    }

    #[test]
    fn macos_cancels_by_killing_the_pending_job() {
        // macOS `shutdown` has no `-c`, so `shutdown -c` would be a silent no-op.
        assert_eq!(
            command_for(OsFamily::Macos, &Elevation::NotNeeded, &request(PowerAction::Cancel, 0)),
            "killall shutdown"
        );
        // Scheduling is the same as other Unixes, though.
        assert_eq!(
            command_for(OsFamily::Macos, &Elevation::NotNeeded, &request(PowerAction::Reboot, 3)),
            "shutdown -r +3"
        );
    }

    #[test]
    fn sudo_wraps_only_when_elevation_calls_for_it() {
        let reboot = request(PowerAction::Reboot, 0);

        let with_password = plan(OsFamily::Linux, &Elevation::SudoPassword, &reboot).unwrap();
        assert_eq!(with_password.command, "sudo -S -p '' shutdown -r now");
        assert!(with_password.needs_password);

        // NOPASSWD still needs the sudo prefix — just not a password.
        let no_password = plan(OsFamily::Linux, &Elevation::SudoNoPassword, &reboot).unwrap();
        assert_eq!(no_password.command, "sudo -S -p '' shutdown -r now");
        assert!(!no_password.needs_password);

        let as_root = plan(OsFamily::Linux, &Elevation::NotNeeded, &reboot).unwrap();
        assert_eq!(as_root.command, "shutdown -r now");
        assert!(!as_root.needs_password);
    }

    #[test]
    fn a_standard_windows_user_is_refused_before_anything_runs() {
        let blocked = Elevation::Unavailable {
            reason: "standard user".into(),
        };
        assert!(plan(OsFamily::Windows, &blocked, &request(PowerAction::Reboot, 0)).is_err());
    }

    #[test]
    fn an_unknown_os_is_refused_rather_than_guessed() {
        assert!(plan(
            OsFamily::Unknown,
            &Elevation::NotNeeded,
            &request(PowerAction::Reboot, 0)
        )
        .is_err());
    }

    #[test]
    fn messages_cannot_break_out_of_their_quoting() {
        let mut hostile = request(PowerAction::Reboot, 5);
        hostile.message = Some("it's time; rm -rf /".into());

        let unix = command_for(OsFamily::Linux, &Elevation::NotNeeded, &hostile);
        // The `;` stays inside the quotes, so it is text and not a separator.
        assert_eq!(unix, r"shutdown -r +5 'it'\''s time; rm -rf /'");

        hostile.message = Some("say \"hi\" & del C:\\".into());
        let windows = command_for(OsFamily::Windows, &Elevation::WindowsAdminToken, &hostile);
        assert_eq!(windows, "shutdown /r /t 300 /c \"say 'hi' & del C:\\\"");
    }

    #[test]
    fn absurd_delays_are_rejected() {
        let mut far_future = request(PowerAction::Reboot, MAX_DELAY_MINUTES + 1);
        assert!(plan(OsFamily::Linux, &Elevation::NotNeeded, &far_future).is_err());

        // The boundary itself is allowed.
        far_future.delay_minutes = MAX_DELAY_MINUTES;
        assert!(plan(OsFamily::Linux, &Elevation::NotNeeded, &far_future).is_ok());
    }

    #[test]
    fn a_dropped_connection_means_success_only_for_an_immediate_action() {
        let reboot_now = request(PowerAction::Reboot, 0);
        let plan_now = plan(OsFamily::Linux, &Elevation::NotNeeded, &reboot_now).unwrap();

        // sshd dies before reporting an exit status — the expected outcome.
        let dropped = CommandOutput {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
        };
        assert!(interpret(&plan_now, &reboot_now, dropped.clone()).unwrap().succeeded);

        // The same silence for a *scheduled* reboot is a real failure: the
        // command should have returned promptly and said what it scheduled.
        let scheduled = request(PowerAction::Reboot, 10);
        let plan_later = plan(OsFamily::Linux, &Elevation::NotNeeded, &scheduled).unwrap();
        assert!(!interpret(&plan_later, &scheduled, dropped).unwrap().succeeded);
    }

    #[test]
    fn a_rejected_sudo_password_is_named_as_such() {
        let reboot = request(PowerAction::Reboot, 0);
        let plan = plan(OsFamily::Linux, &Elevation::SudoPassword, &reboot).unwrap();

        let refused = CommandOutput {
            stdout: String::new(),
            stderr: "sudo: 1 incorrect password attempt".into(),
            exit_code: Some(1),
        };
        let outcome = interpret(&plan, &reboot, refused).unwrap();
        assert!(!outcome.succeeded);
        assert_eq!(outcome.message, "sudo rejected the password.");
    }

    #[test]
    fn windows_access_denied_explains_the_missing_right() {
        let reboot = request(PowerAction::Reboot, 0);
        let plan = plan(OsFamily::Windows, &Elevation::WindowsAdminToken, &reboot).unwrap();

        let denied = CommandOutput {
            stdout: String::new(),
            stderr: "Access is denied.(5)".into(),
            exit_code: Some(5),
        };
        let outcome = interpret(&plan, &reboot, denied).unwrap();
        assert!(!outcome.succeeded);
        assert!(outcome.message.contains("shutdown rights"));
    }
}
