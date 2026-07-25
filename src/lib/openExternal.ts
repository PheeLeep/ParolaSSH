import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";

/**
 * Hand a URL to the system browser.
 *
 * A plain `<a href>` would navigate the webview itself, replacing the app
 * with the web page and leaving no way back — so every outbound link has to
 * go through the opener plugin instead.
 */
export async function openExternal(url: string): Promise<void> {
  try {
    await openUrl(url);
  } catch (error) {
    console.error("Could not open", url, error);
  }
}

/** Show a path in the platform's file manager. */
export async function revealInFileManager(path: string): Promise<void> {
  try {
    await revealItemInDir(path);
  } catch (error) {
    console.error("Could not reveal", path, error);
  }
}
