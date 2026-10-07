import "@testing-library/jest-dom/vitest";

import { cleanup } from "@testing-library/react";
import { afterEach, beforeEach } from "vitest";

import { resetMock } from "../lib/mock";

beforeEach(() => {
  resetMock();
  window.location.hash = "";
});

afterEach(() => cleanup());
