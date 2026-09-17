import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { clearMocks } from "@tauri-apps/api/mocks";
import { afterEach } from "vitest";

afterEach(async () => {
  cleanup();
  // Unmount unlistens asynchronously; clearing the IPC mock first makes them throw.
  await new Promise((resolve) => setTimeout(resolve, 0));
  clearMocks();
  localStorage.clear();
});
