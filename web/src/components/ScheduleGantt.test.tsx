import { describe, it, expect } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ScheduleGantt from "./ScheduleGantt";
import { mockSchedule, mockTxNodes } from "../test/fixtures";

describe("ScheduleGantt", () => {
  it("renders correct number of bars", () => {
    const { container } = render(
      <ScheduleGantt schedule={mockSchedule} txNodes={mockTxNodes} />,
    );
    const bars = container.querySelectorAll("[data-testid^='gantt-bar-']");
    expect(bars).toHaveLength(mockSchedule.length);
  });

  it("bar widths are proportional to duration", () => {
    const { container } = render(
      <ScheduleGantt schedule={mockSchedule} txNodes={mockTxNodes} width={800} />,
    );
    const bars = container.querySelectorAll("[data-testid^='gantt-bar-']");
    // All mock items have same duration (100000), so all bars should have equal width
    const widths = Array.from(bars).map((bar) => {
      const rect = bar.querySelector("rect");
      return rect?.getAttribute("width");
    });
    const uniqueWidths = new Set(widths);
    expect(uniqueWidths.size).toBe(1);
  });

  it("critical path bars have highlight class", () => {
    const { container } = render(
      <ScheduleGantt schedule={mockSchedule} txNodes={mockTxNodes} />,
    );
    // txIndex 0 and 1 are critical
    const criticalBar0 = container.querySelector("[data-testid='gantt-bar-0']");
    const criticalBar1 = container.querySelector("[data-testid='gantt-bar-1']");
    const normalBar2 = container.querySelector("[data-testid='gantt-bar-2']");

    expect(criticalBar0?.getAttribute("data-critical")).toBe("true");
    expect(criticalBar1?.getAttribute("data-critical")).toBe("true");
    expect(normalBar2?.getAttribute("data-critical")).toBe("false");
  });

  it("hover shows tooltip", async () => {
    render(
      <ScheduleGantt schedule={mockSchedule} txNodes={mockTxNodes} />,
    );
    const bar = screen.getByTestId("gantt-bar-0");
    fireEvent.mouseOver(bar);

    // Tooltip should show tx details (bar label + tooltip both show "Tx 0")
    const matches = screen.getAllByText(/Tx 0/);
    expect(matches.length).toBeGreaterThanOrEqual(2); // bar label + tooltip
    // Tooltip shows duration and gas (both 100,000)
    const valueMatches = screen.getAllByText(/100,000/);
    expect(valueMatches.length).toBeGreaterThanOrEqual(1);
  });
});
