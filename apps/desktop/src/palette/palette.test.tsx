import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import type { PaletteContext } from "../bindings/PaletteContext";
import { buildActions, filterActions } from "./actions";
import { Palette } from "./Palette";

const ctx = (intent: PaletteContext["intent"], clipboard = false): PaletteContext => ({
  appName: "App",
  intent,
  subtype: null,
  language: "english",
  languageName: "English",
  selection: null,
  field: { text: "hello", chars: 5 },
  fieldTruncated: false,
  clipboard: clipboard ? { text: "copied", chars: 6, sourceApp: "Mail", kind: "email" } : null,
  contextActions: [],
  hasApiKey: true,
  cloudEnabled: true,
  canInsert: true,
});

describe("palette actions", () => {
  it("puts prompt enhancement first for prompts and writing first for conversations", () => {
    expect(buildActions(ctx("prompt"))[0]?.group).toBe("Prompt");
    expect(buildActions(ctx("conversation"))[0]?.group).toBe("Writing");
  });

  it("offers copied content first when the clipboard is relevant", () => {
    const actions = buildActions(ctx("conversation", true));
    expect(actions[0]?.group).toBe("Copied content");
    expect(actions[0]?.title).toBe("Draft a response");
  });

  it("covers the spec's command set", () => {
    const titles = buildActions(ctx("unknown", true)).map((a) => a.title);
    for (const t of ["Improve text", "Create prompt", "Rewrite", "Summarize", "Explain", "Translate…", "Continue writing", "Open Usage", "Open Settings"]) {
      expect(titles).toContain(t);
    }
  });

  it("filters by title, hint and keywords", () => {
    const actions = buildActions(ctx("prompt"));
    expect(filterActions(actions, "hindi").map((a) => a.id)).toEqual(["translate"]);
    expect(filterActions(actions, "").length).toBe(actions.length);
  });
});

describe("Palette", () => {
  it("runs the highlighted action with Enter and shows an editable result", async () => {
    const user = userEvent.setup();
    render(<Palette />);
    const input = await screen.findByRole("textbox", { name: "Search actions" });
    expect(await screen.findByText("Debug this error")).toBeInTheDocument();
    await user.type(input, "improve prompt");
    await user.keyboard("{Enter}");
    const result = await screen.findByRole("textbox", { name: "Result (editable)" });
    expect((result as HTMLTextAreaElement).value).toMatch(/^Fix the issue in the following code/);
    expect(screen.getByRole("button", { name: /Replace/ })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(await screen.findByRole("textbox", { name: "Search actions" })).toBeInTheDocument();
  });

  it("shows the source of the text it will use", async () => {
    render(<Palette />);
    expect(await screen.findByRole("tab", { name: "Text field" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText(/fix this code it is giving error/)).toBeInTheDocument();
  });
});
