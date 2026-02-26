import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import DependencyGraph from "./DependencyGraph";
import { mockBlockGraph } from "../test/fixtures";

describe("DependencyGraph", () => {
  it("renders correct number of nodes", () => {
    const { container } = render(
      <DependencyGraph graph={mockBlockGraph} onSelectTx={vi.fn()} />,
    );
    const nodes = container.querySelectorAll("[data-testid^='graph-node-']");
    expect(nodes).toHaveLength(mockBlockGraph.txNodes.length);
  });

  it("renders correct number of edges", () => {
    const { container } = render(
      <DependencyGraph graph={mockBlockGraph} onSelectTx={vi.fn()} />,
    );
    const edges = container.querySelectorAll("[data-testid^='graph-edge-']");
    expect(edges).toHaveLength(mockBlockGraph.depEdges.length);
  });

  it("node click fires onSelectTx", () => {
    const onSelectTx = vi.fn();
    const { container } = render(
      <DependencyGraph graph={mockBlockGraph} onSelectTx={onSelectTx} />,
    );
    const node = container.querySelector("[data-testid='graph-node-0']");
    expect(node).toBeTruthy();
    fireEvent.click(node!);
    expect(onSelectTx).toHaveBeenCalledWith(0);
  });

  it("filter toggle hides edge types", () => {
    const { container } = render(
      <DependencyGraph graph={mockBlockGraph} onSelectTx={vi.fn()} />,
    );

    // Initially all edges are visible
    const rawEdges = container.querySelectorAll("[data-edge-kind='raw']");
    expect(rawEdges.length).toBeGreaterThan(0);

    // Toggle off RAW edges
    const rawToggle = screen.getByRole("checkbox", { name: /raw/i });
    fireEvent.click(rawToggle);

    // RAW edges should be hidden
    const hiddenRawEdges = container.querySelectorAll("[data-edge-kind='raw']:not(.hidden)");
    expect(hiddenRawEdges).toHaveLength(0);
  });

  it("filter toggle shows edge types when re-enabled", () => {
    const { container } = render(
      <DependencyGraph graph={mockBlockGraph} onSelectTx={vi.fn()} />,
    );

    const rawToggle = screen.getByRole("checkbox", { name: /raw/i });
    // Toggle off then on
    fireEvent.click(rawToggle);
    fireEvent.click(rawToggle);

    const rawEdges = container.querySelectorAll("[data-edge-kind='raw']:not(.hidden)");
    expect(rawEdges.length).toBeGreaterThan(0);
  });

  it("displays node labels with tx index and short hash", () => {
    const { container } = render(
      <DependencyGraph graph={mockBlockGraph} onSelectTx={vi.fn()} />,
    );
    // Node 0 has a text element with the short hash inside
    const node0 = container.querySelector("[data-testid='graph-node-0']");
    expect(node0?.textContent).toContain("0");
    expect(node0?.textContent).toContain("aaaa");
  });

  it("highlights selected node", () => {
    const { container } = render(
      <DependencyGraph graph={mockBlockGraph} onSelectTx={vi.fn()} selectedTx={0} />,
    );
    const node = container.querySelector("[data-testid='graph-node-0']");
    expect(node).toBeTruthy();
    expect(node!.getAttribute("class")).toContain("selected");
  });
});
