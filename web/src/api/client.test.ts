import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { getBlock, getBlockByHash, listBlocks, getChain, subscribeLive } from "./client";
import { mockBlockGraph, mockChainIdentity, mockBlockSummaries } from "../test/fixtures";

describe("API client", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe("URL construction", () => {
    it("getBlock constructs correct URL for block number", async () => {
      const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response(JSON.stringify(mockBlockGraph), { status: 200 }),
      );
      await getBlock(19000000);
      expect(fetchSpy).toHaveBeenCalledWith("/api/block/19000000");
    });

    it("getBlock constructs correct URL for 'latest'", async () => {
      const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response(JSON.stringify(mockBlockGraph), { status: 200 }),
      );
      await getBlock("latest");
      expect(fetchSpy).toHaveBeenCalledWith("/api/block/latest");
    });

    it("getBlockByHash constructs correct URL", async () => {
      const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response(JSON.stringify(mockBlockGraph), { status: 200 }),
      );
      await getBlockByHash("0xabc");
      expect(fetchSpy).toHaveBeenCalledWith("/api/block/hash/0xabc");
    });

    it("getChain constructs correct URL", async () => {
      const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response(JSON.stringify(mockChainIdentity), { status: 200 }),
      );
      await getChain();
      expect(fetchSpy).toHaveBeenCalledWith("/api/chain");
    });

    it("listBlocks constructs URL with query params", async () => {
      const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response(JSON.stringify({ blocks: mockBlockSummaries }), { status: 200 }),
      );
      await listBlocks({ from: 100, to: 200, limit: 10 });
      const url = fetchSpy.mock.calls[0][0] as string;
      expect(url).toContain("/api/blocks?");
      expect(url).toContain("from=100");
      expect(url).toContain("to=200");
      expect(url).toContain("limit=10");
    });

    it("listBlocks constructs URL without query params when none given", async () => {
      const fetchSpy = vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response(JSON.stringify({ blocks: [] }), { status: 200 }),
      );
      await listBlocks();
      expect(fetchSpy).toHaveBeenCalledWith("/api/blocks");
    });
  });

  describe("404 handling", () => {
    it("getBlock returns null on 404", async () => {
      vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response(null, { status: 404 }),
      );
      const result = await getBlock(99999999);
      expect(result).toBeNull();
    });

    it("getBlockByHash returns null on 404", async () => {
      vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response(null, { status: 404 }),
      );
      const result = await getBlockByHash("0xnonexistent");
      expect(result).toBeNull();
    });
  });

  describe("error handling", () => {
    it("getBlock throws on 500", async () => {
      vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response(null, { status: 500 }),
      );
      await expect(getBlock(19000000)).rejects.toThrow("Failed to fetch block: 500");
    });

    it("listBlocks throws on 500", async () => {
      vi.spyOn(globalThis, "fetch").mockResolvedValue(
        new Response(null, { status: 500 }),
      );
      await expect(listBlocks()).rejects.toThrow("Failed to list blocks: 500");
    });
  });

  describe("SSE subscription", () => {
    it("creates EventSource with correct URL", () => {
      const mockEventSource = vi.fn().mockImplementation(() => ({
        addEventListener: vi.fn(),
        close: vi.fn(),
      }));
      vi.stubGlobal("EventSource", mockEventSource);

      const callback = vi.fn();
      subscribeLive(callback);

      expect(mockEventSource).toHaveBeenCalledWith("/api/live");
    });

    it("registers block event listener", () => {
      const addEventListenerSpy = vi.fn();
      const mockEventSource = vi.fn().mockImplementation(() => ({
        addEventListener: addEventListenerSpy,
        close: vi.fn(),
      }));
      vi.stubGlobal("EventSource", mockEventSource);

      const callback = vi.fn();
      subscribeLive(callback);

      expect(addEventListenerSpy).toHaveBeenCalledWith("block", expect.any(Function));
    });
  });
});
