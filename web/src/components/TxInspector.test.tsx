import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import TxInspector from "./TxInspector";
import { mockBlockGraph } from "../test/fixtures";

describe("TxInspector", () => {
  it("shows details for selected tx", () => {
    render(
      <TxInspector
        txNodes={mockBlockGraph.txNodes}
        depEdges={mockBlockGraph.depEdges}
        selectedTx={0}
      />,
    );
    // Should show tx hash (short form)
    expect(screen.getByText(/0xaaaa/i)).toBeInTheDocument();
    // Should show from address (may appear multiple times due to dep reasons)
    expect(screen.getAllByText(/0x1111/i).length).toBeGreaterThan(0);
    // Should show gas used
    expect(screen.getByText(/100,?000/)).toBeInTheDocument();
  });

  it("shows dependency reasons for selected tx", () => {
    render(
      <TxInspector
        txNodes={mockBlockGraph.txNodes}
        depEdges={mockBlockGraph.depEdges}
        selectedTx={0}
      />,
    );
    // Tx 0 has outgoing edges to tx 1 (RAW) and tx 2 (Nonce1D)
    expect(screen.getByText(/raw/i)).toBeInTheDocument();
  });

  it("shows nothing when no tx is selected", () => {
    const { container } = render(
      <TxInspector
        txNodes={mockBlockGraph.txNodes}
        depEdges={mockBlockGraph.depEdges}
        selectedTx={null}
      />,
    );
    expect(container.querySelector("[data-testid='tx-inspector']")).toBeNull();
  });

  it("shows Tempo metadata when present", () => {
    render(
      <TxInspector
        txNodes={mockBlockGraph.txNodes}
        depEdges={mockBlockGraph.depEdges}
        selectedTx={2}
      />,
    );
    expect(screen.getByText(/payment/i)).toBeInTheDocument();
  });
});
