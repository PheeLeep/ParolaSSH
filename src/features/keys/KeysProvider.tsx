import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import * as api from "./api";
import { errorMessage } from "./api";
import type {
  AuditReport,
  Finding,
  GenerateOutcome,
  GenerateRequest,
  KeyScan,
  SshKey,
  SshLocation,
} from "./types";

type KeysContextValue = {
  location: SshLocation | null;
  scan: KeyScan | null;
  report: AuditReport | null;
  loading: boolean;
  error: string | null;

  keys: SshKey[];
  getKey: (id: string) => SshKey | undefined;
  /** Findings that point at a particular key, worst first. */
  findingsForKey: (key: SshKey) => Finding[];

  refresh: () => Promise<void>;
  generate: (request: GenerateRequest) => Promise<GenerateOutcome>;
  remove: (key: SshKey, includePublic: boolean) => Promise<string[]>;
  applyPermissionFix: (path: string, isDir: boolean) => Promise<void>;
  setSuppressed: (findingId: string, suppressed: boolean) => Promise<void>;
};

const KeysContext = createContext<KeysContextValue | null>(null);

export function KeysProvider({ children }: { children: ReactNode }) {
  const [location, setLocation] = useState<SshLocation | null>(null);
  const [scan, setScan] = useState<KeyScan | null>(null);
  const [report, setReport] = useState<AuditReport | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // Guards against a slow scan overwriting a newer one.
  const requestId = useRef(0);

  const refresh = useCallback(async () => {
    const id = ++requestId.current;
    setLoading(true);
    setError(null);

    try {
      const [nextLocation, nextScan, nextReport] = await Promise.all([
        api.sshLocation(),
        api.listSshKeys(),
        api.auditSshDir(),
      ]);

      if (id !== requestId.current) return;
      setLocation(nextLocation);
      setScan(nextScan);
      setReport(nextReport);
    } catch (caught) {
      if (id !== requestId.current) return;
      setError(errorMessage(caught));
    } finally {
      if (id === requestId.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const generate = useCallback(
    async (request: GenerateRequest) => {
      const outcome = await api.generateSshKey(request);
      // A new key changes both the list and the audit, so re-read everything.
      await refresh();
      return outcome;
    },
    [refresh],
  );

  const remove = useCallback(
    async (key: SshKey, includePublic: boolean) => {
      const removed = await api.deleteSshKey(key.path, includePublic);
      await refresh();
      return removed;
    },
    [refresh],
  );

  const applyPermissionFix = useCallback(
    async (path: string, isDir: boolean) => {
      await api.restrictPermissions(path, isDir);
      await refresh();
    },
    [refresh],
  );

  const setSuppressed = useCallback(
    async (findingId: string, suppressed: boolean) => {
      // The command returns the refreshed report, so no second round trip.
      setReport(await api.setFindingSuppressed(findingId, suppressed));
    },
    [],
  );

  const value = useMemo<KeysContextValue>(() => {
    const keys = scan?.keys ?? [];
    const findings = report?.findings ?? [];

    return {
      location,
      scan,
      report,
      loading,
      error,
      keys,
      getKey: (id) => keys.find((key) => key.id === id),
      findingsForKey: (key) =>
        findings.filter(
          (finding) => finding.path === key.path || finding.id.endsWith(`|${key.id}`),
        ),
      refresh,
      generate,
      remove,
      applyPermissionFix,
      setSuppressed,
    };
  }, [
    location,
    scan,
    report,
    loading,
    error,
    refresh,
    generate,
    remove,
    applyPermissionFix,
    setSuppressed,
  ]);

  return <KeysContext.Provider value={value}>{children}</KeysContext.Provider>;
}

export function useKeys(): KeysContextValue {
  const context = useContext(KeysContext);
  if (!context) {
    throw new Error("useKeys must be used inside a <KeysProvider>");
  }
  return context;
}
