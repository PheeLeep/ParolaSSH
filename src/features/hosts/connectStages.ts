import type { ConnectStage } from "./types";

const METHODS: Record<Extract<ConnectStage, { stage: "authenticating" }>["method"], string> = {
  password: "Signing in with a password",
  key: "Signing in with your key",
  agent: "Offering the keys held by your SSH agent",
  none: "Signing in without a credential",
};

/** One status line per step. */
export function describeStage(stage: ConnectStage): string {
  switch (stage.stage) {
    case "jump":
      return `Reaching jump host “${stage.label}”`;
    case "dialing":
      return stage.viaJump
        ? `Opening a tunnel to ${stage.host}:${stage.port}`
        : `Connecting to ${stage.host}:${stage.port}`;
    case "hostKey":
      return stage.known
        ? `Host key verified - ${stage.algorithm} ${stage.fingerprint}`
        : `Host key trusted and saved - ${stage.algorithm} ${stage.fingerprint}`;
    case "encrypted":
      return `Encrypted with ${stage.cipher} (${stage.kex})`;
    case "authenticating":
      return METHODS[stage.method];
    case "authenticated":
      return "Signed in";
    case "checkingAccount":
      return "Checking how this account elevates";
  }
}
