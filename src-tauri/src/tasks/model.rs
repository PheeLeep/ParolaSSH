//! What a task is, and what running one would actually execute.
//!
//! Two kinds share this shape. A **built-in** is authored in `catalog.rs`:
//! its command is constructed per OS in Rust and unit-tested. A **saved** task
//! is the operator's own text, run verbatim — this module never rewrites it,
//! never "fixes" it, and never claims it is safe. Both are shown in full
//! before anything runs, which is the only promise the app makes about either.
//!
//! Elevation is per task and opt-in: `elevated` is a field on the record, not
//! a property of the module, so the same list can hold a `df -h` that needs
//! nothing and a `systemctl restart` that needs root.

use serde::{Deserialize, Serialize};

use crate::remote::power::{single_quote, Elevation};
use crate::remote::OsFamily;
use crate::ssh::{SshError, SshResult};

use super::danger::{self, DangerAssessment};

/// Longest command we will store. Far above any real one-liner; the point is
/// that a paste accident cannot put a megabyte into the config file.
const MAX_COMMAND_LEN: usize = 4_096;
const MAX_NAME_LEN: usize = 80;

/// Where a task appears.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TaskScope {
    /// Offered on every host whose OS it supports. Pressing it still runs on
    /// exactly one machine — the one being looked at.
    Global,
    /// Offered on one host only.
    Host { host_id: String },
}

impl TaskScope {
    /// Whether this scope should be offered on `host_id`.
    pub fn applies_to(&self, host_id: &str) -> bool {
        match self {
            Self::Global => true,
            Self::Host { host_id: owner } => owner == host_id,
        }
    }
}

/// A saved task, as it lives in `tasks.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub command: String,
    /// Run through the session's elevation route. The operator's choice, per
    /// task — never inferred from what the command looks like.
    #[serde(default)]
    pub elevated: bool,
    pub scope: TaskScope,
    /// Families this task is offered on. Empty means every family.
    #[serde(default)]
    pub os_families: Vec<OsFamily>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub last_run_at: Option<String>,
}

impl TaskRecord {
    /// Whether this task should appear for a host on `os`.
    pub fn supports(&self, os: OsFamily) -> bool {
        self.os_families.is_empty() || self.os_families.contains(&os)
    }
}

/// A task as the form submits it. `id` present means edit, absent means create.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDraft {
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub command: String,
    #[serde(default)]
    pub elevated: bool,
    pub scope: TaskScope,
    #[serde(default)]
    pub os_families: Vec<OsFamily>,
}

/// A draft that passed `validate`. Only the store can turn one into a record,
/// so an unchecked draft can never reach the file.
pub struct ValidDraft {
    pub id: Option<String>,
    name: String,
    description: Option<String>,
    command: String,
    elevated: bool,
    scope: TaskScope,
    os_families: Vec<OsFamily>,
}

impl TaskDraft {
    /// Reject what cannot be stored. Deliberately thin: this is the operator's
    /// own command, and refusing it for looking dangerous is `danger.rs`'s job
    /// to *report*, not this one's to prevent. Only the impossible is rejected.
    pub fn validate(self) -> SshResult<ValidDraft> {
        let name = self.name.trim().to_string();
        if name.is_empty() {
            return Err(SshError::invalid("Give the task a name."));
        }
        if name.chars().count() > MAX_NAME_LEN {
            return Err(SshError::invalid(format!(
                "The name is longer than {MAX_NAME_LEN} characters."
            )));
        }

        // Trailing newlines are stripped, interior ones are not: a multi-line
        // script is a legitimate thing to save.
        let command = self.command.trim().to_string();
        if command.is_empty() {
            return Err(SshError::invalid("Give the task a command to run."));
        }
        if command.len() > MAX_COMMAND_LEN {
            return Err(SshError::invalid(format!(
                "The command is longer than {MAX_COMMAND_LEN} characters."
            )));
        }
        // A NUL cannot survive the trip to a shell, so it is a paste accident
        // rather than a command. Other control characters are left alone —
        // a tab in a script is fine.
        if command.contains('\0') {
            return Err(SshError::invalid(
                "The command contains a null byte, which no shell can receive.",
            ));
        }

        Ok(ValidDraft {
            id: self.id,
            name,
            description: self
                .description
                .map(|text| text.trim().to_string())
                .filter(|text| !text.is_empty()),
            command,
            elevated: self.elevated,
            scope: self.scope,
            os_families: self.os_families,
        })
    }
}

impl ValidDraft {
    /// Build the stored record. `created_at` is carried over on an edit so a
    /// rename does not reset the task's age.
    pub fn apply_to(self, id: String, created_at: Option<String>, last_run_at: Option<String>) -> TaskRecord {
        TaskRecord {
            id,
            name: self.name,
            description: self.description,
            command: self.command,
            elevated: self.elevated,
            scope: self.scope,
            os_families: self.os_families,
            created_at,
            last_run_at,
        }
    }
}

/// Exactly what a press would run, and what the app thinks of it.
///
/// Shown before execution, always. `command` is the literal string sent to the
/// host — including the `sudo` wrapper when the task asked for one — because a
/// display copy that drifts from what runs is worse than showing nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskPlan {
    pub command: String,
    /// The command as written, before any elevation wrapper. Shown alongside
    /// the real one so a long `sudo sh -c '…'` is still readable.
    pub inner_command: String,
    pub elevated: bool,
    /// Whether the account password must be sent to `sudo -S`.
    pub needs_password: bool,
    pub danger: DangerAssessment,
}

/// Build the plan. Pure — no session, no I/O, so every branch is unit-tested.
///
/// The danger assessment runs on the command as *written*, not on the wrapped
/// form: wrapping adds a `sudo` that the operator did not type, and reporting
/// it back as a finding of their own would be a lie.
pub fn plan(
    os: OsFamily,
    elevation: &Elevation,
    command: &str,
    elevated: bool,
) -> SshResult<TaskPlan> {
    let inner = command.trim();
    if inner.is_empty() {
        return Err(SshError::invalid("This task has no command to run."));
    }

    let danger = danger::assess(os, inner);

    if !elevated {
        return Ok(TaskPlan {
            command: inner.to_string(),
            inner_command: inner.to_string(),
            elevated: false,
            needs_password: false,
            danger,
        });
    }

    // Elevation was asked for, so a session with no route to it is an error
    // rather than a quiet downgrade: a task that says it runs as root and then
    // does not is the failure mode this app avoids everywhere else.
    let command = match elevation {
        Elevation::Unavailable { reason } => {
            return Err(SshError::invalid(format!(
                "This task is set to run with elevated privileges, and this session has \
                 no route to them: {reason}. Turn elevation off for this task, or \
                 connect as a user who can elevate."
            )))
        }
        // Already root, and Windows decided at logon — the command runs as-is.
        Elevation::NotNeeded | Elevation::WindowsAdminToken => inner.to_string(),
        Elevation::SudoNoPassword | Elevation::SudoPassword => {
            if !os.is_unix() {
                return Err(SshError::invalid(
                    "sudo is a Unix route to root, and this host is not a Unix host.",
                ));
            }
            // `sh -c` so a task with pipes, redirects or several statements
            // elevates as a whole rather than only its first word. `-p ''`
            // suppresses the prompt, which nothing is there to read.
            format!("sudo -S -p '' sh -c {}", single_quote(inner))
        }
    };

    Ok(TaskPlan {
        command,
        inner_command: inner.to_string(),
        elevated: true,
        needs_password: elevation.needs_password(),
        danger,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::danger::DangerLevel;

    fn draft(name: &str, command: &str) -> TaskDraft {
        TaskDraft {
            id: None,
            name: name.into(),
            description: None,
            command: command.into(),
            elevated: false,
            scope: TaskScope::Global,
            os_families: Vec::new(),
        }
    }

    #[test]
    fn a_plain_task_runs_exactly_what_was_typed() {
        let plan = plan(OsFamily::Linux, &Elevation::SudoPassword, "df -h", false).unwrap();
        assert_eq!(plan.command, "df -h");
        assert!(!plan.elevated);
        // Elevation is available but not asked for, so no password is wanted.
        assert!(!plan.needs_password);
    }

    #[test]
    fn elevation_wraps_the_whole_command_not_just_its_first_word() {
        let plan = plan(
            OsFamily::Linux,
            &Elevation::SudoPassword,
            "systemctl restart sshd && systemctl status sshd",
            true,
        )
        .unwrap();

        assert_eq!(
            plan.command,
            "sudo -S -p '' sh -c 'systemctl restart sshd && systemctl status sshd'"
        );
        assert!(plan.needs_password);
        // The readable form survives for display.
        assert!(plan.inner_command.starts_with("systemctl restart"));
    }

    #[test]
    fn a_quote_in_the_command_cannot_escape_the_wrapper() {
        let plan = plan(
            OsFamily::Linux,
            &Elevation::SudoNoPassword,
            "echo 'it''s fine'; id",
            true,
        )
        .unwrap();

        // Every apostrophe is closed and reopened, so the payload stays one
        // argument to `sh -c`.
        assert!(plan.command.starts_with("sudo -S -p '' sh -c '"));
        assert!(plan.command.ends_with('\''));
        assert!(!plan.needs_password, "NOPASSWD needs no password");
    }

    #[test]
    fn root_and_windows_need_no_wrapper() {
        let as_root = plan(OsFamily::Linux, &Elevation::NotNeeded, "id", true).unwrap();
        assert_eq!(as_root.command, "id");
        assert!(as_root.elevated);

        let windows = plan(
            OsFamily::Windows,
            &Elevation::WindowsAdminToken,
            "Get-Service",
            true,
        )
        .unwrap();
        assert_eq!(windows.command, "Get-Service");
    }

    #[test]
    fn asking_for_elevation_without_a_route_errors_rather_than_downgrading() {
        let refused = plan(
            OsFamily::Linux,
            &Elevation::Unavailable {
                reason: "sudo is not installed".into(),
            },
            "systemctl restart sshd",
            true,
        );
        let message = refused.unwrap_err().to_string();
        assert!(message.contains("sudo is not installed"), "{message}");

        // The same task without elevation still runs — the refusal is about
        // the promise, not about the command.
        assert!(plan(
            OsFamily::Linux,
            &Elevation::Unavailable {
                reason: "sudo is not installed".into()
            },
            "systemctl restart sshd",
            false,
        )
        .is_ok());
    }

    #[test]
    fn the_plan_carries_the_danger_of_what_was_typed() {
        let plan = plan(OsFamily::Linux, &Elevation::NotNeeded, "rm -rf /", true).unwrap();
        assert_eq!(plan.danger.level, DangerLevel::Destructive);
        // The wrapper's own `sudo` is not reported as the operator's doing.
        assert!(!plan
            .danger
            .reasons
            .iter()
            .any(|reason| reason.detail.contains("sudo -S")));
    }

    #[test]
    fn validation_rejects_only_the_impossible() {
        assert!(draft("", "df -h").validate().is_err());
        assert!(draft("Disk", "   ").validate().is_err());
        assert!(draft("Disk", "df -h\0").validate().is_err());

        // Dangerous is not invalid: it is saved, and reported at run time.
        let saved = draft("Wipe", "rm -rf /").validate().unwrap();
        assert_eq!(saved.command, "rm -rf /");

        // A multi-line script keeps its interior newlines.
        let script = draft("Two steps", "  cd /tmp\nls -la  ").validate().unwrap();
        assert_eq!(script.command, "cd /tmp\nls -la");
    }

    #[test]
    fn scope_decides_which_hosts_see_a_task() {
        assert!(TaskScope::Global.applies_to("h-anything"));

        let pinned = TaskScope::Host {
            host_id: "h-web01".into(),
        };
        assert!(pinned.applies_to("h-web01"));
        assert!(!pinned.applies_to("h-db01"));
    }

    #[test]
    fn an_empty_family_list_means_every_family() {
        let mut record = draft("Anywhere", "uptime")
            .validate()
            .unwrap()
            .apply_to("t-1".into(), None, None);
        assert!(record.supports(OsFamily::Linux));
        assert!(record.supports(OsFamily::Windows));

        record.os_families = vec![OsFamily::Linux];
        assert!(record.supports(OsFamily::Linux));
        assert!(!record.supports(OsFamily::Windows));
    }
}
