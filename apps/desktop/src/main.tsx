// SPDX-License-Identifier: Apache-2.0

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { LangProvider, initLang } from "./i18n";
import "./styles.css";

const root = document.getElementById("root");
if (root) {
  initLang();
  createRoot(root).render(
    <StrictMode>
      <LangProvider>
        <App />
      </LangProvider>
    </StrictMode>,
  );
}
