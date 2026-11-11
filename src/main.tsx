import { createRoot } from "react-dom/client";
import "@wterm/react/css";
import "./styles.css";
import { App } from "./App.tsx";
import { I18nProvider } from "./i18n/index.tsx";
import { guardFileDrops } from "./terminal/image-input.ts";

guardFileDrops(window);

// No StrictMode: each pane owns a live PTY, and a double-mounted effect would
// spawn and kill an extra process on every open.
createRoot(document.getElementById("root")!).render(
  <I18nProvider>
    <App />
  </I18nProvider>,
);
