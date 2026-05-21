import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App.jsx";
import { createTauriHostAdapter } from "./hostAdapters.js";
import "./styles.css";

createRoot(document.getElementById("root")).render(
  <App adapter={createTauriHostAdapter()} lane="tauri" />
);
