import "./palette.css";

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { Palette } from "./Palette";

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <Palette />
    </StrictMode>,
  );
}
