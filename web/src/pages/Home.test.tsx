import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import Home from "./Home";
import { mockBlockGraph, mockBlockUpdateEvent } from "../test/fixtures";

vi.mock("../api/client", () => ({
  getBlock: vi.fn(),
  subscribeLive: vi.fn(),
}));

import { getBlock, subscribeLive } from "../api/client";

const mockedGetBlock = vi.mocked(getBlock);
const mockedSubscribeLive = vi.mocked(subscribeLive);

describe("Home", () => {
  let blockCallback: ((e: typeof mockBlockUpdateEvent) => void) | null = null;

  beforeEach(() => {
    vi.clearAllMocks();
    blockCallback = null;

    mockedGetBlock.mockResolvedValue(mockBlockGraph);
    mockedSubscribeLive.mockImplementation((cb) => {
      blockCallback = cb;
      return {
        addEventListener: vi.fn(),
        close: vi.fn(),
        readyState: 1,
      } as unknown as EventSource;
    });
  });

  it("shows latest block", async () => {
    render(
      <MemoryRouter>
        <Home />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText(/19,?000,?000/)).toBeInTheDocument();
    });
    expect(mockedGetBlock).toHaveBeenCalledWith("latest");
  });

  it("shows live indicator", async () => {
    render(
      <MemoryRouter>
        <Home />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByTestId("live-indicator")).toBeInTheDocument();
    });
  });

  it("updates on SSE event", async () => {
    render(
      <MemoryRouter>
        <Home />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText(/19,?000,?000/)).toBeInTheDocument();
    });

    // Simulate SSE event for new block
    const newBlock = {
      ...mockBlockGraph,
      blockNumber: 19000003,
      metrics: { ...mockBlockGraph.metrics, txCount: 12 },
    };
    mockedGetBlock.mockResolvedValue(newBlock);

    await act(async () => {
      blockCallback?.(mockBlockUpdateEvent);
    });

    await waitFor(() => {
      expect(screen.getByText(/19,?000,?003/)).toBeInTheDocument();
    });
  });

  it("has link to block detail", async () => {
    render(
      <MemoryRouter>
        <Home />
      </MemoryRouter>,
    );

    await waitFor(() => {
      const link = screen.getByRole("link", { name: /view/i });
      expect(link).toHaveAttribute("href", "/block/19000000");
    });
  });
});
