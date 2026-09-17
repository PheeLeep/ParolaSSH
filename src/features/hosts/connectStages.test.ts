import { describe, expect, it } from "vitest";

import { describeStage } from "./connectStages";

describe("describeStage", () => {
  it("names a direct dial and a tunnelled one differently", () => {
    expect(describeStage({ stage: "dialing", host: "10.0.0.5", port: 22, viaJump: false })).toBe(
      "Connecting to 10.0.0.5:22",
    );
    expect(describeStage({ stage: "dialing", host: "db", port: 2222, viaJump: true })).toBe(
      "Opening a tunnel to db:2222",
    );
  });

  it("says whether a host key was already known or just trusted", () => {
    const key = { stage: "hostKey", algorithm: "ssh-ed25519", fingerprint: "SHA256:abc" } as const;
    expect(describeStage({ ...key, known: true })).toBe("Host key verified - ssh-ed25519 SHA256:abc");
    expect(describeStage({ ...key, known: false })).toMatch(/^Host key trusted and saved/);
  });

  it("describes every sign-in method without naming a secret", () => {
    for (const method of ["password", "key", "agent", "none"] as const) {
      expect(describeStage({ stage: "authenticating", method })).toMatch(/^(Signing in|Offering)/);
    }
  });

  it("covers the remaining steps", () => {
    expect(describeStage({ stage: "jump", label: "bastion" })).toBe("Reaching jump host “bastion”");
    expect(describeStage({ stage: "encrypted", kex: "curve25519-sha256", cipher: "chacha20-poly1305@openssh.com" })).toBe(
      "Encrypted with chacha20-poly1305@openssh.com (curve25519-sha256)",
    );
    expect(describeStage({ stage: "authenticated" })).toBe("Signed in");
    expect(describeStage({ stage: "checkingAccount" })).toBe("Checking how this account elevates");
  });
});
