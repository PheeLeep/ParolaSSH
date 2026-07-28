import { useCallback, useEffect, useRef, useState } from "react";
import { Alert, Badge, Button, Card, Form, Modal, Spinner } from "react-bootstrap";
import {
  AlertTriangle,
  ListChecks,
  Pencil,
  Play,
  Plus,
  ShieldAlert,
  Square,
  Trash2,
  X,
} from "lucide-react";
import * as api from "../api";
import { errorMessage } from "../api";
import { useElevation } from "../ElevationProvider";
import { useHosts } from "../HostsProvider";
import * as taskStore from "../taskStore";
import { useStoreSubscription } from "../../../lib/useStoreSubscription";
import { useTheme } from "../../../theme/ThemeProvider";
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
  const run = taskStore.get(hostId);

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
    try {
      await taskStore.start(hostId, target.id, target.name, plan, theme, password);
    } catch (caught) {
      setError(errorMessage(caught));
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

      {run && <RunCard hostId={hostId} run={run} />}

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
              <h2 className="h6 mb-0">Built in</h2>
              <span className="text-body-secondary small">
                written for {osLabel(os)} — none of them install anything
              </span>
            </div>

            {catalog && catalog.builtin.length > 0 ? (
              <div className="task-grid">
                {catalog.builtin.map((task) => (
                  <BuiltinCard
                    key={task.id}
                    task={task}
                    busy={run?.state === "running"}
                    onRun={() => void preview(task.id, task.name)}
                  />
                ))}
              </div>
            ) : (
              <Alert variant="secondary" className="mb-0">
                {disconnected
                  ? "Connect to this host to see the tasks written for its operating system."
                  : `No built-in task is written for ${osLabel(os)} yet. Your own tasks still run here.`}
              </Alert>
            )}
          </section>

          <section>
            <div className="d-flex align-items-center gap-2 mb-2">
              <Pencil className="icon-sm" aria-hidden="true" />
              <h2 className="h6 mb-0">Yours</h2>
              <span className="text-body-secondary small">
                run exactly as written — the app makes no claim about them
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

            {catalog && catalog.saved.length > 0 ? (
              <div className="task-grid">
                {catalog.saved.map((task) => (
                  <SavedCard
                    key={task.id}
                    task={task}
                    busy={run?.state === "running"}
                    onRun={() => void preview(task.id, task.name)}
                    onEdit={() => setEditing(task)}
                    onDelete={() => void remove(task)}
                  />
                ))}
              </div>
            ) : (
              <Alert variant="secondary" className="mb-0">
                No saved tasks for this host yet. A task is a command you keep — set it
                global to get it on every host, or pin it to this one.
              </Alert>
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

function BuiltinCard({
  task,
  busy,
  onRun,
}: {
  task: BuiltinTask;
  busy: boolean;
  onRun: () => void;
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
      </div>
      <p className="task-card__detail">{task.description}</p>
      <Button size="sm" variant="outline-primary" disabled={busy} onClick={onRun}>
        <Play className="icon-sm" aria-hidden="true" />
        Run
      </Button>
    </article>
  );
}

function SavedCard({
  task,
  busy,
  onRun,
  onEdit,
  onDelete,
}: {
  task: TaskRecord;
  busy: boolean;
  onRun: () => void;
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
        <Button size="sm" variant="outline-primary" disabled={busy} onClick={onRun}>
          <Play className="icon-sm" aria-hidden="true" />
          Run
        </Button>
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
 *  confirmation — not to prevent the command, which is the operator's to make,
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
  const destructive = plan.danger.level === "destructive";
  const armed = !destructive || typed.trim().toUpperCase() === CONFIRM_WORD;

  return (
    <Modal show onHide={onCancel} centered backdrop="static" size="lg">
      <Modal.Header closeButton>
        <Modal.Title className="h6">Run “{name}”?</Modal.Title>
      </Modal.Header>
      <Modal.Body className="d-flex flex-column gap-3">
        <div>
          <div className="text-body-secondary small mb-1">
            This exact command runs on the host:
          </div>
          <pre className="task-plan__command mb-0">{plan.command}</pre>
        </div>

        {plan.elevated && plan.command !== plan.innerCommand && (
          <div className="text-body-secondary small">
            The <code>sudo</code> wrapper is the app's; the command you saved is{" "}
            <code>{plan.innerCommand}</code>.
          </div>
        )}

        <DangerNotice danger={plan.danger} />

        {destructive && (
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

/** The assessment, in the operator's terms. Absent when nothing matched —
 *  and deliberately *not* replaced with "looks safe", which the check has no
 *  basis to say. */
function DangerNotice({ danger }: { danger: DangerAssessment }) {
  if (danger.level === "none") return null;

  const destructive = danger.level === "destructive";

  return (
    <Alert variant={destructive ? "danger" : "warning"} className="mb-0">
      <div className="d-flex align-items-center gap-2 mb-2">
        {destructive ? (
          <ShieldAlert className="icon-sm" aria-hidden="true" />
        ) : (
          <AlertTriangle className="icon-sm" aria-hidden="true" />
        )}
        <strong>{destructive ? "This destroys data or the machine" : "Worth a second look"}</strong>
      </div>

      <ul className="task-danger__list">
        {danger.reasons.map((reason) => (
          <li key={reason.label}>
            <strong>{reason.label}.</strong> {reason.detail}
          </li>
        ))}
      </ul>

      <div className="small text-body-secondary mt-2 mb-0">
        This is a check on the text of the command — it catches common mistakes, not a
        command written to hide what it does. Read the command above; it is the one that
        runs.
      </div>
    </Alert>
  );
}

/* ── The run ───────────────────────────────────────────────────────────── */

function RunCard({ hostId, run }: { hostId: string; run: taskStore.TaskRun }) {
  const mount = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!mount.current) return;
    return taskStore.attach(hostId, mount.current);
  }, [hostId, run.startedAt]);

  const running = run.state === "running";

  return (
    <Card>
      <Card.Body className="d-flex flex-column gap-2">
        <div className="d-flex flex-wrap align-items-center gap-2">
          <h2 className="h6 mb-0">{run.taskName}</h2>
          <RunBadge run={run} />
          <div className="ms-auto d-flex gap-2">
            {running ? (
              <Button size="sm" variant="outline-danger" onClick={() => void taskStore.stop(hostId)}>
                <Square className="icon-sm" aria-hidden="true" />
                Stop watching
              </Button>
            ) : (
              <Button size="sm" variant="outline-secondary" onClick={() => taskStore.clear(hostId)}>
                <X className="icon-sm" aria-hidden="true" />
                Clear
              </Button>
            )}
          </div>
        </div>

        <div className="task-feed" ref={mount} />
      </Card.Body>
    </Card>
  );
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
      await api.saveTask(draft);
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
            Run exactly as written, in a non-interactive shell — nothing can answer a
            prompt, so pass the flag that skips it.
          </Form.Text>
        </Form.Group>

        {danger && <DangerNotice danger={danger} />}

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
            ? "Offered on every host. Pressing it still runs on one machine — the one you are looking at."
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
        <Button variant="primary" disabled={saving || !name.trim() || !command.trim()} onClick={() => void save()}>
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
