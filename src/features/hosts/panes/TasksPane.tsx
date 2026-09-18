import { useCallback, useEffect, useState } from "react";
import { Alert, Badge, Button, Form, Modal, Spinner } from "react-bootstrap";
import {
  AlertTriangle,
  Ban,
  Eye,
  ListChecks,
  Play,
  Plus,
  ShieldAlert,
  Square,
  Trash2,
} from "lucide-react";
import * as api from "../api";
import { errorMessage } from "../api";
import { useElevation } from "../ElevationProvider";
import { useHosts } from "../HostsProvider";
import * as taskStore from "../taskStore";
import { useStoreSubscription } from "../../../lib/useStoreSubscription";
import { useTheme } from "../../../theme/ThemeProvider";
import { isBlocked, readTaskBlocking } from "../../settings/preferences";
import type {
  BuiltinTask,
  DangerAssessment,
  HostTasks,
  OsFamily,
  TaskDraft,
  TaskPlan,
  TaskRecord,
} from "../types";

/** What has to be typed to arm a destructive task. Short enough to type, long
 *  enough that muscle memory does not do it for you. */
const CONFIRM_WORD = "RUN";

export function TasksPane({ hostId }: { hostId: string }) {
  const { getConnection } = useHosts();
  const requestElevation = useElevation();
  const { resolved: theme } = useTheme();
  const connection = getConnection(hostId);

  const [catalog, setCatalog] = useState<HostTasks | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  /** The task waiting on its plan being approved. */
  const [pending, setPending] = useState<{ id: string; name: string; plan: TaskPlan } | null>(
    null,
  );
  const [editing, setEditing] = useState<TaskRecord | "new" | null>(null);

  useStoreSubscription(taskStore.subscribe);
  /** The task whose output dialog is open. */
  const [viewing, setViewing] = useState<string | null>(null);

  useEffect(() => {
    taskStore.applyTheme(theme);
  }, [theme]);

  const refresh = useCallback(async () => {
    try {
      setCatalog(await api.listHostTasks(hostId));
      setError(null);
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setLoading(false);
    }
  }, [hostId]);

  useEffect(() => {
    setLoading(true);
    void refresh();
  }, [refresh]);

  // Planning is a separate round trip from running, always: what the operator
  // approves has to be what executes, and the only way to promise that is to
  // show the real command before the run exists.
  const preview = async (id: string, name: string) => {
    setError(null);
    // Already running: show it rather than start a second copy.
    if (taskStore.find(hostId, id)?.state === "running") {
      setViewing(id);
      return;
    }
    try {
      setPending({ id, name, plan: await api.planTask(hostId, id) });
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const execute = async (plan: TaskPlan) => {
    if (!pending) return;

    let password: string | null = null;
    if (plan.needsPassword) {
      const grant = await requestElevation({
        hostId,
        summary: `Run “${pending.name}” as root`,
        command: plan.command,
        destructive: plan.danger.level !== "none",
      });
      if (grant.outcome !== "granted") return;
      password = grant.password;
    }

    const target = pending;
    setPending(null);
    // The run is registered before `start` first awaits, so the dialog opens
    // on it and shows output from the first byte.
    const started = taskStore.start(hostId, target.id, target.name, plan, theme, password);
    setViewing(target.id);
    try {
      await started;
    } catch (caught) {
      // The dialog already shows the failure in the output; this covers a
      // refusal before any run existed.
      if (!taskStore.find(hostId, target.id)) setError(errorMessage(caught));
    }
  };

  const remove = async (task: TaskRecord) => {
    try {
      await api.deleteTask(task.id);
      await refresh();
    } catch (caught) {
      setError(errorMessage(caught));
    }
  };

  const os = catalog?.os ?? "unknown";
  const disconnected = connection === undefined;

  return (
    <div className="d-flex flex-column gap-3">
      {error && (
        <Alert variant="danger" className="text-prewrap mb-0" dismissible onClose={() => setError(null)}>
          {error}
        </Alert>
      )}


      {loading ? (
        <div className="d-flex align-items-center gap-2 text-body-secondary">
          <Spinner animation="border" size="sm" aria-hidden="true" />
          Loading tasks…
        </div>
      ) : (
        <>
          <section>
            <div className="d-flex align-items-center gap-2 mb-2">
              <ListChecks className="icon-sm" aria-hidden="true" />
              <h2 className="h6 mb-0">Tasks</h2>
              <span className="text-body-secondary small">
                built-ins are written for {osLabel(os)} and install nothing; yours run
                exactly as written
              </span>
              <Button
                size="sm"
                variant="outline-secondary"
                className="ms-auto"
                onClick={() => setEditing("new")}
              >
                <Plus className="icon-sm" aria-hidden="true" />
                New task
              </Button>
            </div>

            {disconnected && (
              <Alert variant="secondary" className="mb-2">
                Connect to this host to see the built-in tasks written for its operating system.
              </Alert>
            )}

            {catalog && catalog.builtin.length + catalog.saved.length > 0 ? (
              <div className="task-grid">
                {catalog.builtin.map((task) => (
                  <BuiltinCard
                    key={task.id}
                    task={task}
                    run={taskStore.find(hostId, task.id)}
                    onRun={() => void preview(task.id, task.name)}
                    onShow={() => setViewing(task.id)}
                  />
                ))}
                {catalog.saved.map((task) => (
                  <SavedCard
                    key={task.id}
                    task={task}
                    run={taskStore.find(hostId, task.id)}
                    onRun={() => void preview(task.id, task.name)}
                    onShow={() => setViewing(task.id)}
                    onEdit={() => setEditing(task)}
                    onDelete={() => void remove(task)}
                  />
                ))}
              </div>
            ) : (
              !disconnected && (
                <Alert variant="secondary" className="mb-0">
                  No tasks for {osLabel(os)} yet. A task is a command you keep - set it global
                  to get it on every host, or pin it to this one.
                </Alert>
              )
            )}
          </section>
        </>
      )}

      {pending && (
        <PlanDialog
          name={pending.name}
          plan={pending.plan}
          onCancel={() => setPending(null)}
          onConfirm={() => void execute(pending.plan)}
        />
      )}

      {viewing && taskStore.find(hostId, viewing) && (
        <RunDialog run={taskStore.find(hostId, viewing)!} onClose={() => setViewing(null)} />
      )}

      {editing && (
        <TaskEditor
          hostId={hostId}
          os={os}
          task={editing === "new" ? null : editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            void refresh();
          }}
        />
      )}
    </div>
  );
}

/* ── The list ──────────────────────────────────────────────────────────── */

/** Run, or Show while this task is still running; once it has finished, a
 *  link reopens its last result. */
function RunButton({
  run,
  onRun,
  onShow,
}: {
  run: taskStore.TaskRun | undefined;
  onRun: () => void;
  onShow: () => void;
}) {
  if (run?.state === "running") {
    return (
      <Button size="sm" variant="outline-primary" onClick={onShow}>
        <Eye className="icon-sm" aria-hidden="true" />
        Show
      </Button>
    );
  }
  return (
    <span className="d-inline-flex align-items-center gap-3">
      <Button size="sm" variant="outline-primary" onClick={onRun}>
        <Play className="icon-sm" aria-hidden="true" />
        Run
      </Button>
      {run && (
        <Button size="sm" variant="link" className="p-0 text-decoration-none d-inline-flex align-items-center gap-1" onClick={onShow}>
          <RunDot run={run} />
          Last result
        </Button>
      )}
    </span>
  );
}

function BuiltinCard({
  task,
  run,
  onRun,
  onShow,
}: {
  task: BuiltinTask;
  run: taskStore.TaskRun | undefined;
  onRun: () => void;
  onShow: () => void;
}) {
  return (
    <article className="task-card">
      <div className="task-card__head">
        <h3 className="task-card__title">{task.name}</h3>
        {task.elevated && (
          <Badge bg="secondary" className="task-card__badge">
            root
          </Badge>
        )}
        <Badge bg="primary" className="task-card__badge">
          built-in
        </Badge>
      </div>
      <p className="task-card__detail">{task.description}</p>
      <RunButton run={run} onRun={onRun} onShow={onShow} />
    </article>
  );
}

function SavedCard({
  task,
  run,
  onRun,
  onShow,
  onEdit,
  onDelete,
}: {
  task: TaskRecord;
  run: taskStore.TaskRun | undefined;
  onRun: () => void;
  onShow: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const [confirming, setConfirming] = useState(false);

  return (
    <article className="task-card">
      <div className="task-card__head">
        <h3 className="task-card__title">{task.name}</h3>
        {task.elevated && (
          <Badge bg="secondary" className="task-card__badge">
            root
          </Badge>
        )}
        <Badge bg="light" text="dark" className="task-card__badge">
          {task.scope.kind === "global" ? "global" : "this host"}
        </Badge>
      </div>

      {task.description && <p className="task-card__detail">{task.description}</p>}
      <code className="task-card__command">{task.command}</code>

      <div className="task-card__actions">
        <RunButton run={run} onRun={onRun} onShow={onShow} />
        <Button size="sm" variant="link" className="p-0 text-decoration-none" onClick={onEdit}>
          Edit
        </Button>
        {confirming ? (
          <span className="d-inline-flex align-items-center gap-2 ms-auto small">
            <span className="text-body-secondary">Delete?</span>
            <Button size="sm" variant="danger" onClick={onDelete}>
              Yes
            </Button>
            <Button size="sm" variant="outline-secondary" onClick={() => setConfirming(false)}>
              No
            </Button>
          </span>
        ) : (
          <Button
            size="sm"
            variant="link"
            className="ms-auto p-0 text-decoration-none text-body-secondary"
            onClick={() => setConfirming(true)}
            aria-label={`Delete ${task.name}`}
          >
            <Trash2 className="icon-sm" aria-hidden="true" />
          </Button>
        )}
      </div>
    </article>
  );
}

/* ── The gate ──────────────────────────────────────────────────────────── */

/** What runs, shown before it runs. A destructive assessment adds a typed
 *  confirmation - not to prevent the command, which is the operator's to make,
 *  but to make it impossible to reach by reflex. */
function PlanDialog({
  name,
  plan,
  onCancel,
  onConfirm,
}: {
  name: string;
  plan: TaskPlan;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const [typed, setTyped] = useState("");
  const [blocked] = useState(() => isBlocked(plan.danger.level, readTaskBlocking()));
  const destructive = plan.danger.level === "destructive";
  const armed = !blocked && (!destructive || typed.trim().toUpperCase() === CONFIRM_WORD);

  return (
    <Modal show onHide={onCancel} centered backdrop="static" size="lg">
      <Modal.Header closeButton>
        <Modal.Title className="h6">Run “{name}”?</Modal.Title>
      </Modal.Header>
      <Modal.Body className="d-flex flex-column gap-3">
        {plan.wrapper === "powershell" ? (
          <div>
            <div className="text-body-secondary small mb-1">This runs on the host in PowerShell:</div>
            <pre className="task-plan__command mb-0">{plan.innerCommand}</pre>
            <details className="text-body-secondary small mt-1">
              <summary>Exact command sent</summary>
              Encoded, so the host's login shell cannot misread it:
              <pre className="task-plan__command mb-0 mt-1">{plan.command}</pre>
            </details>
          </div>
        ) : (
          <div>
            <div className="text-body-secondary small mb-1">
              This exact command runs on the host:
            </div>
            <pre className="task-plan__command mb-0">{plan.command}</pre>
          </div>
        )}

        {plan.wrapper === "sudo" && (
          <div className="text-body-secondary small">
            The <code>sudo</code> wrapper is the app's; the command you saved is{" "}
            <code>{plan.innerCommand}</code>.
          </div>
        )}

        <DangerNotice danger={plan.danger} blocked={blocked ? "run" : undefined} />

        {destructive && !blocked && (
          <Form.Group controlId="task-confirm">
            <Form.Label className="small mb-1">
              Type <strong>{CONFIRM_WORD}</strong> to enable the button.
            </Form.Label>
            <Form.Control
              value={typed}
              onChange={(event) => setTyped(event.target.value)}
              autoComplete="off"
              spellCheck={false}
            />
          </Form.Group>
        )}
      </Modal.Body>
      <Modal.Footer>
        <Button variant="outline-secondary" onClick={onCancel}>
          Cancel
        </Button>
        <Button variant={destructive ? "danger" : "primary"} disabled={!armed} onClick={onConfirm}>
          <Play className="icon-sm" aria-hidden="true" />
          Run
        </Button>
      </Modal.Footer>
    </Modal>
  );
}

/** The assessment, in the operator's terms. Absent when nothing matched -
 *  and deliberately *not* replaced with "looks safe", which the check has no
 *  basis to say. */
function DangerNotice({
  danger,
  blocked,
}: {
  danger: DangerAssessment;
  /** Set when the blocking setting forbids this command; the alert then says so. */
  blocked?: "run" | "saved";
}) {
  if (danger.level === "none") return null;

  const destructive = danger.level === "destructive";
  const title = blocked
    ? `Blocked: this command cannot be ${blocked}`
    : destructive
      ? "This destroys data or the machine"
      : "Worth a second look";

  return (
    <Alert variant={destructive || blocked ? "danger" : "warning"} className="mb-0">
      <div className="d-flex align-items-center gap-2 mb-2">
        {blocked ? (
          <Ban className="icon-sm" aria-hidden="true" />
        ) : destructive ? (
          <ShieldAlert className="icon-sm" aria-hidden="true" />
        ) : (
          <AlertTriangle className="icon-sm" aria-hidden="true" />
        )}
        <strong>{title}</strong>
      </div>

      <ul className="task-danger__list">
        {danger.reasons.map((reason) => (
          <li key={reason.label}>
            <strong>{reason.label}.</strong> {reason.detail}
          </li>
        ))}
      </ul>

      <div className="small text-body-secondary mt-2 mb-0">
        {blocked
          ? "Settings › Advanced › Block dangerous tasks does not allow it. Turn that off, or use a terminal, if you mean it."
          : "This is a check on the text of the command - it catches common mistakes, not a command written to hide what it does. Read the command above; it is the one that runs."}
      </div>
    </Alert>
  );
}

/* ── The run ───────────────────────────────────────────────────────────── */

/** A run's output. Closing it only hides the dialog: the task keeps running,
 *  and its card's Show or Last result brings it back. */
function RunDialog({ run, onClose }: { run: taskStore.TaskRun; onClose: () => void }) {
  const [mount, setMount] = useState<HTMLDivElement | null>(null);

  useEffect(() => {
    if (!mount) return;
    return taskStore.attach(run.hostId, run.taskId, mount);
  }, [mount, run]);

  return (
    <Modal show onHide={onClose} centered size="xl">
      <Modal.Header closeButton>
        <Modal.Title className="h6 d-flex align-items-center gap-2 me-3 flex-grow-1">
          {run.taskName}
          <RunBadge run={run} />
          {run.state === "running" && (
            <Button
              size="sm"
              variant="outline-danger"
              className="ms-auto"
              onClick={() => void taskStore.stop(run.hostId, run.taskId)}
            >
              <Square className="icon-sm" aria-hidden="true" />
              Stop watching
            </Button>
          )}
        </Modal.Title>
      </Modal.Header>
      <Modal.Body className="p-0">
        {/* Owned by the store, which appends the run's terminal here. */}
        <div className="task-feed" ref={setMount} />
      </Modal.Body>
      <Modal.Footer className="justify-content-between">
        <span className="text-body-secondary small">
          {run.state === "running"
            ? "Closing this keeps the task running; Show on its card brings it back."
            : `Finished ${new Date(run.finishedAt ?? run.startedAt).toLocaleTimeString()}`}
        </span>
        <Button variant="outline-secondary" onClick={onClose}>
          Close
        </Button>
      </Modal.Footer>
    </Modal>
  );
}

/** A run's state at a glance: pulsing while running, coloured once done. */
function RunDot({ run }: { run: taskStore.TaskRun }) {
  const tone =
    run.state === "running" ? "running" : run.state === "finished" ? "ok" : run.state === "failed" ? "bad" : "idle";
  return <span className={`task-dot task-dot--${tone}`} aria-hidden="true" />;
}

function RunBadge({ run }: { run: taskStore.TaskRun }) {
  if (run.state === "running") {
    return (
      <span className="task-progress" role="status">
        <span className="task-progress__dot" aria-hidden="true" />
        <span className="text-body-secondary small">Running</span>
      </span>
    );
  }
  if (run.state === "stopped") {
    return <Badge bg="secondary">Stopped watching</Badge>;
  }
  if (run.state === "failed") {
    return <Badge bg="danger">{run.exitCode === null ? "Failed" : `Exit ${run.exitCode}`}</Badge>;
  }
  return <Badge bg="success">Finished</Badge>;
}

/* ── The editor ────────────────────────────────────────────────────────── */

function TaskEditor({
  hostId,
  os,
  task,
  onClose,
  onSaved,
}: {
  hostId: string;
  os: OsFamily;
  task: TaskRecord | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [name, setName] = useState(task?.name ?? "");
  const [description, setDescription] = useState(task?.description ?? "");
  const [command, setCommand] = useState(task?.command ?? "");
  const [elevated, setElevated] = useState(task?.elevated ?? false);
  const [global, setGlobal] = useState(task ? task.scope.kind === "global" : true);
  const [thisOsOnly, setThisOsOnly] = useState((task?.osFamilies.length ?? 0) > 0);
  const [danger, setDanger] = useState<DangerAssessment | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [blocking] = useState(readTaskBlocking);
  const blocked = danger !== null && command.trim() !== "" && isBlocked(danger.level, blocking);

  // Assessed as it is typed, so the warning arrives while the command is still
  // being written rather than at the moment of pressing run.
  useEffect(() => {
    const text = command.trim();
    if (!text) {
      setDanger(null);
      return;
    }

    let cancelled = false;
    const timer = window.setTimeout(() => {
      void api
        .assessTaskCommand(text, hostId)
        .then((assessment) => {
          if (!cancelled) setDanger(assessment);
        })
        .catch(() => undefined);
    }, 300);

    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [command, hostId]);

  const save = async () => {
    setSaving(true);
    setError(null);

    const draft: TaskDraft = {
      id: task?.id ?? null,
      name,
      description: description.trim() || null,
      command,
      elevated,
      scope: global ? { kind: "global" } : { kind: "host", hostId },
      osFamilies: thisOsOnly && os !== "unknown" ? [os] : [],
    };

    try {
      await api.saveTask(draft, hostId, readTaskBlocking());
      onSaved();
    } catch (caught) {
      setError(errorMessage(caught));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Modal show onHide={onClose} centered backdrop="static" size="lg">
      <Modal.Header closeButton>
        <Modal.Title className="h6">{task ? "Edit task" : "New task"}</Modal.Title>
      </Modal.Header>
      <Modal.Body className="d-flex flex-column gap-3">
        {error && <Alert variant="danger" className="text-prewrap mb-0">{error}</Alert>}

        <Form.Group controlId="task-name">
          <Form.Label className="small mb-1">Name</Form.Label>
          <Form.Control
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="Restart the app server"
            autoFocus
          />
        </Form.Group>

        <Form.Group controlId="task-description">
          <Form.Label className="small mb-1">
            Description <span className="text-body-secondary">(optional)</span>
          </Form.Label>
          <Form.Control
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            placeholder="What this is for, and when to reach for it"
          />
        </Form.Group>

        <Form.Group controlId="task-command">
          <Form.Label className="small mb-1">Command</Form.Label>
          <Form.Control
            as="textarea"
            rows={4}
            className="font-monospace"
            value={command}
            onChange={(event) => setCommand(event.target.value)}
            placeholder="systemctl restart myapp && systemctl --no-pager status myapp"
            spellCheck={false}
          />
          <Form.Text className="text-body-secondary">
            Run exactly as written, in a non-interactive shell - nothing can answer a
            prompt, so pass the flag that skips it.
          </Form.Text>
        </Form.Group>

        {danger && <DangerNotice danger={danger} blocked={blocked ? "saved" : undefined} />}

        <Form.Check
          type="switch"
          id="task-elevated"
          checked={elevated}
          onChange={(event) => setElevated(event.target.checked)}
          label="Run with elevated privileges"
        />
        <div className="text-body-secondary small mt-n2">
          Wrapped in <code>sudo</code> using this session's own route to root. A host with
          no route refuses the task rather than running it as someone else.
        </div>

        <Form.Check
          type="switch"
          id="task-global"
          checked={global}
          onChange={(event) => setGlobal(event.target.checked)}
          label="Available on every host"
        />
        <div className="text-body-secondary small mt-n2">
          {global
            ? "Offered on every host. Pressing it still runs on one machine - the one you are looking at."
            : "Offered on this host only, and deleted with it."}
        </div>

        {os !== "unknown" && (
          <Form.Check
            type="switch"
            id="task-os"
            checked={thisOsOnly}
            onChange={(event) => setThisOsOnly(event.target.checked)}
            label={`Only offer this on ${osLabel(os)} hosts`}
          />
        )}
      </Modal.Body>
      <Modal.Footer>
        <Button variant="outline-secondary" onClick={onClose}>
          Cancel
        </Button>
        <Button variant="primary" disabled={saving || blocked || !name.trim() || !command.trim()} onClick={() => void save()}>
          {saving && <Spinner animation="border" size="sm" className="me-1" aria-hidden="true" />}
          Save
        </Button>
      </Modal.Footer>
    </Modal>
  );
}

const OS_LABELS: Record<OsFamily, string> = {
  linux: "Linux",
  macos: "macOS",
  bsd: "BSD",
  windows: "Windows",
  unknown: "this host",
};

function osLabel(os: OsFamily): string {
  return OS_LABELS[os];
}
