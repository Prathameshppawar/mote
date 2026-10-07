import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import { mockInvoke } from "../lib/mock";
import { App } from "./App";

describe("App", () => {
  it("shows General by default and navigates between sections", async () => {
    const user = userEvent.setup();
    render(<App />);
    expect(await screen.findByRole("heading", { name: "General" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Privacy" }));
    expect(await screen.findByRole("heading", { name: "Privacy" })).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "Clipboard" })).toHaveAttribute("aria-checked", "true");
  });

  it("labels browser preview data as sample data", async () => {
    render(<App />);
    expect(await screen.findByText(/sample data/i)).toBeInTheDocument();
  });

  it("starts with onboarding until it is completed", async () => {
    await mockInvoke("reset_local_data");
    const user = userEvent.setup();
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Welcome to Mote" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Continue" }));
    expect(await screen.findByRole("heading", { name: "Your data stays yours" })).toBeInTheDocument();
  });

  it("toggling a setting saves it", async () => {
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: "Completion" }));
    const toggle = await screen.findByRole("switch", { name: "Inline completion" });
    expect(toggle).toHaveAttribute("aria-checked", "true");
    await user.click(toggle);
    await waitFor(() => expect(toggle).toHaveAttribute("aria-checked", "false"));
    const saved = await mockInvoke<{ completion: { enabled: boolean } }>("get_settings");
    expect(saved.completion.enabled).toBe(false);
  });
});

describe("Usage dashboard", () => {
  it("shows KPI tiles, charts with table twins, and scopes breakdowns by range", async () => {
    window.location.hash = "#/usage";
    const user = userEvent.setup();
    render(<App />);
    expect(await screen.findByText("Tokens today")).toBeInTheDocument();
    expect(screen.getByText("This month")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Tokens\. Last 30 days/ })).toBeInTheDocument();

    // Every chart has a table view.
    const tokensCard = screen.getByText("Tokens", { selector: "figcaption" }).closest("figure");
    expect(tokensCard).not.toBeNull();
    await user.click(within(tokensCard as HTMLElement).getByRole("button", { name: "Table" }));
    expect(within(tokensCard as HTMLElement).getAllByRole("row").length).toBe(31);

    // The range filter scopes the breakdowns below it.
    await user.click(screen.getByRole("button", { name: "Today" }));
    expect(screen.getByText("Today, per model.")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /Requests\. Today, by hour/ })).toBeInTheDocument();
  });

  it("explains Mote usage vs provider usage", async () => {
    window.location.hash = "#/usage";
    render(<App />);
    expect(await screen.findByText(/source of truth for cost/)).toBeInTheDocument();
    expect(screen.getByText(/Raw prompt content is not required for usage analytics/)).toBeInTheDocument();
  });
});
