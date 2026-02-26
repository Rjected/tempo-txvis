import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import BlockNav from "./BlockNav";

describe("BlockNav", () => {
  it("renders current block number", () => {
    render(
      <BlockNav blockNumber={19000000} onNavigate={vi.fn()} />,
    );
    const input = screen.getByRole("spinbutton") as HTMLInputElement;
    expect(input.value).toBe("19000000");
  });

  it("prev button navigates to previous block", () => {
    const onNavigate = vi.fn();
    render(
      <BlockNav blockNumber={19000000} onNavigate={onNavigate} />,
    );
    fireEvent.click(screen.getByLabelText(/prev/i));
    expect(onNavigate).toHaveBeenCalledWith(18999999);
  });

  it("next button navigates to next block", () => {
    const onNavigate = vi.fn();
    render(
      <BlockNav blockNumber={19000000} onNavigate={onNavigate} />,
    );
    fireEvent.click(screen.getByLabelText(/next/i));
    expect(onNavigate).toHaveBeenCalledWith(19000001);
  });

  it("input submission navigates to entered block", () => {
    const onNavigate = vi.fn();
    render(
      <BlockNav blockNumber={19000000} onNavigate={onNavigate} />,
    );
    const input = screen.getByRole("spinbutton");
    fireEvent.change(input, { target: { value: "12345678" } });
    fireEvent.submit(input.closest("form")!);
    expect(onNavigate).toHaveBeenCalledWith(12345678);
  });
});
