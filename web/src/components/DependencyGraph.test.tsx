import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import DependencyGraph from "./DependencyGraph";
import { mockBlockGraph } from "../test/fixtures";

// Cytoscape renders to a canvas, so we test the controls and callback wiring
// rather than inspecting individual SVG elements.

describe("DependencyGraph", () => {
  it("renders the graph container", () => {
    const { container } = render(
      <DependencyGraph graph={mockBlockGraph} onSelectTx={vi.fn()} />,
    );
    // Cytoscape mounts a div container
    expect(container.querySelector(".rounded-lg.border")).toBeTruthy();
  });

  it("renders filter checkboxes for all edge kinds", () => {
    render(<DependencyGraph graph={mockBlockGraph} onSelectTx={vi.fn()} />);
    expect(screen.getByRole("checkbox", { name: /raw/i })).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: /waw/i })).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: /nonce 1d/i })).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: /nonce 2d/i })).toBeTruthy();
    expect(screen.getByRole("checkbox", { name: /fee sponsorship/i })).toBeTruthy();
  });

  it("filter checkboxes are checked by default", () => {
    render(<DependencyGraph graph={mockBlockGraph} onSelectTx={vi.fn()} />);
    const rawToggle = screen.getByRole("checkbox", { name: /raw/i });
    expect(rawToggle).toBeChecked();
  });

  it("filter toggle unchecks when clicked", () => {
    render(<DependencyGraph graph={mockBlockGraph} onSelectTx={vi.fn()} />);
    const rawToggle = screen.getByRole("checkbox", { name: /raw/i });
    fireEvent.click(rawToggle);
    expect(rawToggle).not.toBeChecked();
  });

  it("filter toggle re-checks when clicked twice", () => {
    render(<DependencyGraph graph={mockBlockGraph} onSelectTx={vi.fn()} />);
    const rawToggle = screen.getByRole("checkbox", { name: /raw/i });
    fireEvent.click(rawToggle);
    fireEvent.click(rawToggle);
    expect(rawToggle).toBeChecked();
  });

  it("renders fit-to-view button", () => {
    render(<DependencyGraph graph={mockBlockGraph} onSelectTx={vi.fn()} />);
    expect(screen.getByText("Fit to View")).toBeTruthy();
  });
});
