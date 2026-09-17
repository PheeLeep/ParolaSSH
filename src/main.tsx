import React from "react";
import ReactDOM from "react-dom/client";
import { isTauri } from "@tauri-apps/api/core";

import "bootstrap/dist/css/bootstrap.min.css";
import "./styles/theme.css";
import "./styles/app.css";

const root = ReactDOM.createRoot(document.getElementById("root") as HTMLElement);

// Outside the Tauri webview (e.g. a browser on the dev server) never load the app.
if (isTauri()) {
  // Dynamic import so App's modules and their side effects never run in a browser.
  void import("./App").then(({ default: App }) =>
    root.render(
      <React.StrictMode>
        <App />
      </React.StrictMode>,
    ),
  );
} else {
  root.render(
    <main className="d-flex vh-100 align-items-center justify-content-center p-4 text-center">
      <div>
        <p className="text-body-secondary mb-0">
          This site is not available.
        </p>
      </div>
    </main>,
  );
}
