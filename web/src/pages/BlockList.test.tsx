import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import BlockList from "./BlockList";
import { mockBlockSummaries } from "../test/fixtures";

vi.mock("../api/client", () => ({
  listBlocks: vi.fn(),
}));

import { listBlocks } from "../api/client";

const mockedListBlocks = vi.mocked(listBlocks);

describe("BlockList", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockedListBlocks.mockResolvedValue({ blocks: mockBlockSummaries });
  });

  it("renders rows for each block", async () => {
    render(
      <MemoryRouter>
        <BlockList />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText(/19,?000,?002/)).toBeInTheDocument();
      expect(screen.getByText(/19,?000,?001/)).toBeInTheDocument();
      expect(screen.getByText(/19,?000,?000/)).toBeInTheDocument();
    });
  });

  it("shows tx count for each block", async () => {
    render(
      <MemoryRouter>
        <BlockList />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText("10")).toBeInTheDocument();
      expect(screen.getByText("8")).toBeInTheDocument();
      expect(screen.getByText("4")).toBeInTheDocument();
    });
  });

  it("shows speedup for each block", async () => {
    render(
      <MemoryRouter>
        <BlockList />
      </MemoryRouter>,
    );

    await waitFor(() => {
      // All blocks have speedup 2.0
      const speedups = screen.getAllByText("2.00x");
      expect(speedups.length).toBe(3);
    });
  });

  it("rows link to block detail", async () => {
    render(
      <MemoryRouter>
        <BlockList />
      </MemoryRouter>,
    );

    await waitFor(() => {
      const links = screen.getAllByRole("link");
      const blockLink = links.find((l) => l.getAttribute("href") === "/block/19000002");
      expect(blockLink).toBeTruthy();
    });
  });

  it("pagination loads next page", async () => {
    mockedListBlocks
      .mockResolvedValueOnce({ blocks: mockBlockSummaries })
      .mockResolvedValueOnce({ blocks: [] });

    render(
      <MemoryRouter>
        <BlockList />
      </MemoryRouter>,
    );

    await waitFor(() => {
      expect(screen.getByText(/19,?000,?000/)).toBeInTheDocument();
    });

    const nextBtn = screen.getByRole("button", { name: /next/i });
    fireEvent.click(nextBtn);

    await waitFor(() => {
      expect(mockedListBlocks).toHaveBeenCalledTimes(2);
    });
  });
});
