// SPDX-License-Identifier: Apache-2.0

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ConfirmProvider, ToastProvider } from "./components";
import { LangProvider, initLang } from "./i18n";
import { initTheme } from "./theme";
import "./styles.css";

const root = document.getElementById("root");
if (root) {
  initTheme();
  initLang();
  createRoot(root).render(
    <StrictMode>
      <LangProvider>
        <ToastProvider>
          <ConfirmProvider>
            <App />
          </ConfirmProvider>
        </ToastProvider>
      </LangProvider>
    </StrictMode>,
  );
}
