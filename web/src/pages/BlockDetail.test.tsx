import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import BlockDetail from "./BlockDetail";
import { mockBlockGraph } from "../test/fixtures";

vi.mock("../api/client", () => ({
  getBlock: vi.fn(),
}));

import { getBlock } from "../api/client";

const mockedGetBlock = vi.mocked(getBlock);

function renderBlockDetail(blockNumber = "19000000") {
  return render(
    <MemoryRouter initialEntries={[`/block/${blockNumber}`]}>
      <Routes>
        <Route path="/block/:number" element={<BlockDetail />} />
      </Routes>
    </MemoryRouter>,
  );
}

describe("BlockDetail", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedGetBlock.mockResolvedValue(mockBlockGraph);
  });

  it("loads graph data", async () => {
    renderBlockDetail();

    await waitFor(() => {
      expect(mockedGetBlock).toHaveBeenCalledWith("19000000");
    });

    await waitFor(() => {
      expect(screen.getByText(/19,?000,?000/)).toBeInTheDocument();
    });
  });

  it("shows Graph tab by default", async () => {
    renderBlockDetail();

    await waitFor(() => {
      const graphTab = screen.getByRole("tab", { name: /graph/i });
      expect(graphTab).toHaveAttribute("aria-selected", "true");
    });
  });

  it("switches between Graph and Gantt tabs", async () => {
    renderBlockDetail();

    await waitFor(() => {
      expect(screen.getByRole("tab", { name: /graph/i })).toBeInTheDocument();
    });

    const ganttTab = screen.getByRole("tab", { name: /gantt/i });
    fireEvent.click(ganttTab);

    expect(ganttTab).toHaveAttribute("aria-selected", "true");
    const graphTab = screen.getByRole("tab", { name: /graph/i });
    expect(graphTab).toHaveAttribute("aria-selected", "false");
  });

  it("shows metrics panel", async () => {
    renderBlockDetail();

    await waitFor(() => {
      expect(screen.getByText(/transactions/i)).toBeInTheDocument();
      expect(screen.getByText("2.00x")).toBeInTheDocument();
    });
  });

  it("shows TxInspector when node is clicked", async () => {
    renderBlockDetail();

    await waitFor(() => {
      expect(screen.getByText(/19,?000,?000/)).toBeInTheDocument();
    });

    // Click on a graph node
    const node = screen.getByTestId("graph-node-0");
    fireEvent.click(node);

    await waitFor(() => {
      expect(screen.getByTestId("tx-inspector")).toBeInTheDocument();
    });
  });
});
