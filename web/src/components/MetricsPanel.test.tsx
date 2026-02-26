import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import MetricsPanel from "./MetricsPanel";
import { mockMetrics } from "../test/fixtures";

describe("MetricsPanel", () => {
  it("renders tx count", () => {
    render(<MetricsPanel metrics={mockMetrics} />);
    expect(screen.getByText("4")).toBeInTheDocument();
    expect(screen.getByText(/transactions/i)).toBeInTheDocument();
  });

  it("renders speedup", () => {
    render(<MetricsPanel metrics={mockMetrics} />);
    expect(screen.getByText("2.00x")).toBeInTheDocument();
  });

  it("renders critical path weight", () => {
    render(<MetricsPanel metrics={mockMetrics} />);
    expect(screen.getByText(/200,?000/)).toBeInTheDocument();
  });

  it("renders max parallelism", () => {
    render(<MetricsPanel metrics={mockMetrics} />);
    expect(screen.getByText(/max parallelism/i)).toBeInTheDocument();
  });
});
