import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

// Opened in a plain browser (no Tauri backend)? Run on sample data instead.
const ready = "__TAURI_INTERNALS__" in window ? Promise.resolve() : import("./demo").then(() => {});

ready.then(() =>
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <App />
    </React.StrictMode>,
  ),
);
