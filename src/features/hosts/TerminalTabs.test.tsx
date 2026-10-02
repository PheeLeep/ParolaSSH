import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

/** Just enough of xterm for the store: it writes, disposes, and reports size. */
vi.mock("@xterm/xterm", () => ({
  Terminal: class {
    cols = 80;
    rows = 24;
    options: Record<string, unknown> = {};
    written: string[] = [];
    loadAddon() {}
    open(node: HTMLElement) {
      node.dataset.xterm = "open";
    }
    write(chunk: string) {
      this.written.push(chunk);
    }
    pasted: string[] = [];
    paste(text: string) {
      this.pasted.push(text);
    }
    screen = "";
    buffer = {
      active: {
        type: "normal",
        baseY: 0,
        cursorY: 0,
        getLine: () => ({ isWrapped: false, translateToString: () => this.screen }),
      },
    };
    handler: ((data: string) => void) | null = null;
    onData(handler: (data: string) => void) {
      this.handler = handler;
    }
    attachCustomKeyEventHandler() {}
    focus() {}
    clear() {}
    dispose() {}
  },
}));
vi.mock("@xterm/addon-fit", () => ({ FitAddon: class { fit() {} } }));
vi.mock("@xterm/xterm/css/xterm.css", () => ({}));
vi.mock("../../lib/appWindow", () => ({ isMacOS: false }));
vi.mock("../../theme/ThemeProvider", () => ({ useTheme: () => ({ resolved: "dark" }) }));

globalThis.ResizeObserver ??= class {
  observe() {}
  disconnect() {}
} as unknown as typeof ResizeObserver;

let nextShell: number;
let openFails: string | null;
let closed: number[];
let sent: string[];

beforeEach(() => {
  nextShell = 1;
  openFails = null;
  closed = [];
  sent = [];
  mockIPC(
    (cmd, args) => {
      const a = args as Record<string, number>;
      if (cmd === "open_shell") {
        if (openFails) throw openFails;
        return nextShell++;
      }
      if (cmd === "close_shell") closed.push(a.shellId);
      if (cmd === "write_shell") sent.push(String((args as Record<string, unknown>).data));
      if (cmd === "assess_task_command") {
        const command = String((args as Record<string, unknown>).command);
        return command.includes("rm -rf /")
          ? { level: "destructive", reasons: [{ label: "Recursive delete", detail: "x", level: "destructive" }] }
          : { level: "none", reasons: [] };
      }
    },
    { shouldMockEvents: true },
  );
});

/** A fresh store per test: open shells would otherwise carry over. */
async function load() {
  vi.resetModules();
  const { TerminalTabs } = await import("./TerminalTabs");
  const store = await import("./terminalStore");
  return { TerminalTabs, store };
}

describe("TerminalTabs", () => {
  it("opens a first shell on mount and more on demand", async () => {
    const { TerminalTabs } = await load();
    const user = userEvent.setup();
    render(<TerminalTabs hostId="h" />);

    expect(await screen.findByText("shell 1")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "New terminal" }));
    expect(await screen.findByText("shell 2")).toBeInTheDocument();
  });

  it("shows why a shell could not open and offers to try again", async () => {
    openFails = "Not connected to that host. Connect first, then try again.";
    const { TerminalTabs } = await load();
    render(<TerminalTabs hostId="h" />);

    expect(await screen.findByText(/Not connected to that host/)).toBeInTheDocument();
    expect(screen.getByText("No terminal open on this host.")).toBeInTheDocument();
  });

  it("closes a tab and tells the backend", async () => {
    const { TerminalTabs } = await load();
    const user = userEvent.setup();
    render(<TerminalTabs hostId="h" />);
    await screen.findByText("shell 1");

    await user.click(screen.getByRole("button", { name: "Close shell 1" }));
    await waitFor(() => expect(closed).toEqual([1]));
    expect(screen.getByText("No terminal open on this host.")).toBeInTheDocument();
  });

  describe("renaming", () => {
    it("commits on Enter, trimmed", async () => {
      const { TerminalTabs } = await load();
      const user = userEvent.setup();
      render(<TerminalTabs hostId="h" />);
      await user.dblClick(await screen.findByText("shell 1"));

      const field = screen.getByLabelText("Tab name");
      await user.clear(field);
      await user.type(field, "  logs  {Enter}");
      expect(await screen.findByText("logs")).toBeInTheDocument();
    });

    it("reverts on Escape", async () => {
      const { TerminalTabs } = await load();
      const user = userEvent.setup();
      render(<TerminalTabs hostId="h" />);
      await user.click(await screen.findByRole("button", { name: "Rename this tab" }));

      const field = screen.getByLabelText("Tab name");
      await user.clear(field);
      await user.type(field, "nope{Escape}");
      expect(await screen.findByText("shell 1")).toBeInTheDocument();
      expect(screen.queryByText("nope")).not.toBeInTheDocument();
    });

    it("restores the original name when cleared", async () => {
      const { TerminalTabs, store } = await load();
      const user = userEvent.setup();
      render(<TerminalTabs hostId="h" />);
      await screen.findByText("shell 1");

      act(() => store.rename(1, "build"));
      await user.dblClick(await screen.findByText("build"));
      const field = screen.getByLabelText("Tab name");
      await user.clear(field);
      fireEvent.blur(field);
      expect(await screen.findByText("shell 1")).toBeInTheDocument();
    });

    it("caps a very long name", async () => {
      const { store } = await load();
      const { TerminalTabs } = await import("./TerminalTabs");
      render(<TerminalTabs hostId="h" />);
      await screen.findByText("shell 1");

      act(() => store.rename(1, "x".repeat(200)));
      expect(store.get(1)?.title).toHaveLength(store.MAX_TERMINAL_TITLE);
    });
  });

  it("marks a tab whose session ended with its exit code", async () => {
    const { TerminalTabs, store } = await load();
    render(<TerminalTabs hostId="h" />);
    await screen.findByText("shell 1");

    await act(() => emit("terminal://closed", { hostId: "h", shellId: 1, exitCode: 130 }));
    expect(await screen.findByText("130")).toBeInTheDocument();
    expect(store.liveCount()).toBe(0);
  });

  it("routes output only to its own shell", async () => {
    const { TerminalTabs, store } = await load();
    render(<TerminalTabs hostId="h" />);
    await screen.findByText("shell 1");

    await emit("terminal://output", { hostId: "h", shellId: 1, chunk: "mine" });
    await emit("terminal://output", { hostId: "h", shellId: 99, chunk: "stale" });
    await emit("terminal://output", { hostId: "other", shellId: 1, chunk: "elsewhere" });

    const written = (store.get(1)?.terminal as unknown as { written: string[] }).written;
    await waitFor(() => expect(written).toEqual(["mine"]));
  });

  it("steps a per-tab font size within bounds", async () => {
    const { TerminalTabs, store } = await load();
    const user = userEvent.setup();
    render(<TerminalTabs hostId="h" />);
    await screen.findByText("shell 1");

    await user.click(screen.getByRole("button", { name: "Font for this terminal" }));
    const size = store.fontFor(1).size;
    await user.click(await screen.findByRole("button", { name: "Larger" }));
    expect(store.fontFor(1).size).toBe(size + 1);
    expect(store.hasFontOverride(1)).toBe(true);

    await user.click(screen.getByRole("button", { name: "Use the default" }));
    expect(store.hasFontOverride(1)).toBe(false);
  });

  describe("paste guard", () => {
    const pasteInto = async (text: string) => {
      const { TerminalTabs, store } = await load();
      render(<TerminalTabs hostId="h" />);
      await screen.findByText("shell 1");
      const entry = store.get(1)!;
      const event = new Event("paste", { bubbles: true, cancelable: true });
      Object.assign(event, { clipboardData: { getData: () => text } });
      act(() => {
        entry.node.dispatchEvent(event);
      });
      return { entry, event };
    };

    it("holds back a flagged multi-line paste until confirmed", async () => {
      const user = userEvent.setup();
      const { entry, event } = await pasteInto("rm -rf /\n");
      expect(event.defaultPrevented).toBe(true);
      await user.click(await screen.findByRole("button", { name: "Paste anyway" }));
      expect((entry.terminal as unknown as { pasted: string[] }).pasted).toEqual(["rm -rf /\n"]);
    });

    it("drops the paste on cancel", async () => {
      const user = userEvent.setup();
      const { entry } = await pasteInto("rm -rf /\n");
      await user.click(await screen.findByRole("button", { name: "Cancel" }));
      expect((entry.terminal as unknown as { pasted: string[] }).pasted).toEqual([]);
    });

    it("pastes a harmless multi-line paste without asking", async () => {
      const { entry } = await pasteInto("ls -la\n");
      await waitFor(() =>
        expect((entry.terminal as unknown as { pasted: string[] }).pasted).toEqual(["ls -la\n"]),
      );
      expect(screen.queryByRole("button", { name: "Paste anyway" })).toBeNull();
    });

    it("leaves a single pasted line to xterm", async () => {
      const { event } = await pasteInto("rm -rf /");
      expect(event.defaultPrevented).toBe(false);
    });

    describe("typed commands", () => {
      const typeLine = async (text: string) => {
        const { TerminalTabs, store } = await load();
        render(<TerminalTabs hostId="h" />);
        await screen.findByText("shell 1");
        const terminal = store.get(1)!.terminal as unknown as {
          handler: (data: string) => void;
          screen: string;
        };
        // What the shell echoed, however the line got there (typed, history, Tab).
        terminal.screen = `me@host:~$ ${text}`;
        act(() => terminal.handler("\r"));
      };

      it("holds Enter on a flagged line, then sends it once allowed", async () => {
        const user = userEvent.setup();
        await typeLine("rm -rf /etc");
        await user.click(await screen.findByRole("button", { name: "Run anyway" }));
        await waitFor(() => expect(sent[sent.length - 1]).toBe("\r"));
      });

      it("clears the line instead of running it on cancel", async () => {
        const user = userEvent.setup();
        await typeLine("rm -rf /etc");
        await user.click(await screen.findByRole("button", { name: "Cancel" }));
        await waitFor(() => expect(sent[sent.length - 1]).toBe("\x15"));
        expect(sent).not.toContain("\r");
      });

      it("sends a harmless line straight away", async () => {
        await typeLine("ls");
        await waitFor(() => expect(sent[sent.length - 1]).toBe("\r"));
        expect(screen.queryByRole("button", { name: "Run anyway" })).toBeNull();
      });
    });
  });
});
