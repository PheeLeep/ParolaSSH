//! Tauri commands for the Tasks module.
//!
//! Reading and writing follow `hosts::commands`: read the file, change it,
//! write it back, so two windows cannot serve different lists.
//!
//! Running follows the audit's streaming path rather than `Session::exec`,
//! which caps at thirty seconds - a task is arbitrary length by definition,
//! and a backup that takes four minutes must not be cut off at thirty seconds
//! with its output discarded.
//!
//! Two commands exist where one would do: `plan_task` builds the exact command
//! and its danger assessment without running anything, and `start_task` runs
//! it. The UI always calls the first and shows what it returns before it is
//! allowed to call the second.

use tauri::{AppHandle, State};
use zeroize::Zeroizing;

use super::catalog::{self, BuiltinTaskView};
use super::model::{self, TaskDraft, TaskPlan, TaskRecord};
use super::store::TaskStore;
use crate::app_paths::config_dir;
use crate::hosts::store::now_iso8601;
use crate::logging;
use crate::remote::registry::SessionRegistry;
use crate::remote::secrets::SecretVault;
use crate::remote::{stream, OsFamily};
use crate::ssh::{SshError, SshResult};

/// Everything one host can run: the built-ins its OS supports, and the saved
/// tasks scoped to it.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostTasks {
    pub os: OsFamily,
    pub builtin: Vec<BuiltinTaskView>,
    pub saved: Vec<TaskRecord>,
}

/// What a host is offered. Takes the OS from the live session when there is
/// one; a disconnected host is asked for by OS so the list can still be
/// managed before connecting.
#[tauri::command]
pub fn list_host_tasks(
    app: AppHandle,
    registry: State<'_, SessionRegistry>,
    host_id: String,
) -> SshResult<HostTasks> {
    // A connected host knows its own OS; an unconnected one is unknown, and
    // `Unknown` is offered no built-ins rather than a guess.
    let os = registry
        .get(&host_id)
        .map(|live| live.os)
        .unwrap_or(OsFamily::Unknown);

    let store = TaskStore::read(&config_dir(&app)?);
    let saved = store
        .for_host(&host_id, os)
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();

    Ok(HostTasks {
        os,
        builtin: catalog::for_os(os),
        saved,
    })
}

/// Every saved task, for the settings-style list that manages them all.
#[tauri::command]
pub fn list_all_tasks(app: AppHandle) -> SshResult<Vec<TaskRecord>> {
    Ok(TaskStore::read(&config_dir(&app)?).tasks)
}

/// Add a task, or update one when the draft carries an id.
#[tauri::command]
pub fn save_task(app: AppHandle, draft: TaskDraft) -> SshResult<TaskRecord> {
    let config_dir = config_dir(&app)?;
    let mut store = TaskStore::read(&config_dir);

    let record = store.upsert(draft.validate()?, now_iso8601())?;
    store.write(&config_dir)?;

    Ok(record)
}

#[tauri::command]
pub fn delete_task(app: AppHandle, id: String) -> SshResult<TaskRecord> {
    let config_dir = config_dir(&app)?;
    let mut store = TaskStore::read(&config_dir);

    let removed = store.remove(&id)?;
    store.write(&config_dir)?;

    Ok(removed)
}

/// Drop every task pinned to a host. Called when the host record is deleted,
/// so the file cannot accumulate tasks aimed at machines that are gone.
#[tauri::command]
pub fn forget_host_tasks(app: AppHandle, host_id: String) -> SshResult<usize> {
    let config_dir = config_dir(&app)?;
    let mut store = TaskStore::read(&config_dir);

    let removed = store.forget_host(&host_id);
    if removed > 0 {
        store.write(&config_dir)?;
    }
    Ok(removed)
}

/// What pressing run would execute, and what the app makes of it. Runs nothing.
///
/// `taskId` names either a built-in or a saved task; `elevated` overrides the
/// task's own setting for this one press, which is how the pane offers "run
/// this without root" on a task that normally elevates.
#[tauri::command]
pub fn plan_task(
    app: AppHandle,
    registry: State<'_, SessionRegistry>,
    host_id: String,
    task_id: String,
    elevated: Option<bool>,
) -> SshResult<TaskPlan> {
    let live = registry.require(&host_id)?;
    let (command, default_elevated) = resolve(&app, &host_id, &task_id, live.os)?;

    model::plan(
        live.os,
        &live.elevation,
        &command,
        elevated.unwrap_or(default_elevated),
    )
}

/// Assess an arbitrary command without saving or running it - what the editor
/// calls as the operator types, so the warning appears before the task exists.
#[tauri::command]
pub fn assess_task_command(
    registry: State<'_, SessionRegistry>,
    host_id: Option<String>,
    command: String,
) -> super::danger::DangerAssessment {
    // With no host in context the rules for every platform apply, which is the
    // cautious reading and the honest one: the task may end up anywhere.
    let os = host_id
        .and_then(|id| registry.get(&id).map(|live| live.os))
        .unwrap_or(OsFamily::Unknown);

    super::danger::assess(os, &command)
}

/// Run a task, streaming its output. Returns the stream id; output arrives as
/// `stream://output` events and `close_stream` stops watching.
///
/// The plan is rebuilt here rather than accepted from the caller. A frontend
/// that showed one command and sent another would be the single worst bug this
/// module could have, and passing the command back across the boundary is what
/// would make it possible.
// Tauri injects the first four; the rest are the call's own. Splitting them
// into a struct would only move the same arguments behind a name.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn start_task(
    app: AppHandle,
    webview: tauri::Webview,
    registry: State<'_, SessionRegistry>,
    vault: State<'_, SecretVault>,
    host_id: String,
    task_id: String,
    elevated: Option<bool>,
    password: Option<String>,
) -> SshResult<u64> {
    let live = registry.require(&host_id)?;
    let (command, default_elevated) = resolve(&app, &host_id, &task_id, live.os)?;
    let plan = model::plan(
        live.os,
        &live.elevation,
        &command,
        elevated.unwrap_or(default_elevated),
    )?;

    // `sudo -S` reads a line from stdin either way; a NOPASSWD rule ignores
    // it, so the empty line keeps one code path.
    let stdin: Option<Zeroizing<Vec<u8>>> = if plan.elevated && live.os.is_unix() {
        let secret = password
            .map(Zeroizing::new)
            .or_else(|| live.login_password())
            .or_else(|| vault.recall(&host_id))
            .unwrap_or_else(|| Zeroizing::new(String::new()));
        Some(Zeroizing::new(format!("{}\n", secret.as_str()).into_bytes()))
    } else {
        None
    };

    // Checked before the channel opens, for the same reason a followed log is.
    if live.stream_count() >= crate::remote::registry::MAX_STREAMS_PER_HOST {
        return Err(SshError::invalid(
            "This host already has too many streams open. Close one before starting \
             a task.",
        ));
    }

    // The task's name and whether it elevated - never its command, and never
    // its output. A task's output describes the machine in detail and belongs
    // on screen, not in a file on this one.
    logging::info(
        "tasks",
        format!(
            "Task {task_id} started on host {host_id} ({})",
            if plan.elevated { "elevated" } else { "unprivileged" }
        ),
    );

    let label = webview.label().to_string();
    let handle = stream::open(
        &live.session,
        app.clone(),
        label,
        host_id,
        &plan.command,
        stdin.as_deref().map(Vec::as_slice),
    )
    .await?;

    let stream_id = handle.id;
    live.add_stream(handle);

    // Stamped on start rather than on completion: a stream that is still going
    // has already run, and a run that never finishes is the one most worth
    // seeing a timestamp for.
    if let Ok(config_dir) = config_dir(&app) {
        let mut store = TaskStore::read(&config_dir);
        store.touch(&task_id, now_iso8601());
        let _ = store.write(&config_dir);
    }

    Ok(stream_id)
}

/// Find a task by id - built-in or saved - and return its command and whether
/// it elevates by default.
///
/// Built-ins are looked up first: their ids are `kebab-case` words and saved
/// ids are `t-<hex>`, so the two namespaces cannot collide.
fn resolve(
    app: &AppHandle,
    host_id: &str,
    task_id: &str,
    os: OsFamily,
) -> SshResult<(String, bool)> {
    if let Some(builtin) = catalog::find(task_id, os) {
        return Ok((builtin.command.to_string(), builtin.elevated));
    }

    let store = TaskStore::read(&config_dir(app)?);
    let task = store.get(task_id).ok_or_else(|| {
        SshError::invalid("That task no longer exists - it may have been deleted.")
    })?;

    // Scope is enforced here, not only in the list: a stale window holding a
    // task id must not be able to run it against a host it was never for.
    if !task.scope.applies_to(host_id) {
        return Err(SshError::invalid(
            "That task belongs to a different host.",
        ));
    }
    if !task.supports(os) {
        return Err(SshError::invalid(format!(
            "That task is not set up to run on {}.",
            os.label()
        )));
    }

    Ok((task.command.clone(), task.elevated))
}

#[cfg(test)]
mod tests {
    use crate::tasks::catalog;
    use crate::remote::OsFamily;

    #[test]
    fn builtin_and_saved_ids_cannot_collide() {
        // `resolve` checks the catalog first, so a saved task can never
        // shadow a built-in. That is only safe while the two id shapes stay
        // distinct - saved ids are minted as `t-<hex>`.
        for task in catalog::BUILTIN_TASKS {
            assert!(
                !task.id.starts_with("t-"),
                "built-in `{}` collides with the saved-task id namespace",
                task.id
            );
        }
    }

    #[test]
    fn every_builtin_resolves_on_a_family_that_offers_it() {
        for task in catalog::BUILTIN_TASKS {
            let families = [OsFamily::Linux, OsFamily::Macos, OsFamily::Windows];
            assert!(
                families
                    .iter()
                    .any(|os| catalog::find(task.id, *os).is_some()),
                "built-in `{}` is offered on no OS at all",
                task.id
            );
        }
    }
}
