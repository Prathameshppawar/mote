import { act, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { mockEmit, mockHasListener, sampleOverlay } from "../lib/mock";
import { Overlay } from "./Overlay";

describe("Overlay", () => {
  it("renders nothing until a view arrives, then hides on request", async () => {
    render(<Overlay />);
    // The live region stays mounted (so suggestions are announced) but is empty.
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
    await waitFor(() => expect(mockHasListener("overlay-view") && mockHasListener("overlay-hide")).toBe(true));
    await act(async () => mockEmit("overlay-view", sampleOverlay("correction")));
    expect(screen.getByText("completd")).toBeInTheDocument();
    expect(screen.getByText("completed")).toBeInTheDocument();
    expect(screen.getByText("Tab")).toBeInTheDocument();
    await act(async () => mockEmit("overlay-hide", null));
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
  });

  it("shows completion ghost text with the candidate counter", async () => {
    render(<Overlay />);
    await waitFor(() => expect(mockHasListener("overlay-view")).toBe(true));
    await act(async () => mockEmit("overlay-view", sampleOverlay("completion")));
    expect(screen.getByText(/the Redis container was unavailable/)).toBeInTheDocument();
    expect(screen.getByText("1/2")).toBeInTheDocument();
  });
});
