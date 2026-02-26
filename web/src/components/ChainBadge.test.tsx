import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import ChainBadge from "./ChainBadge";

describe("ChainBadge", () => {
  it("renders Ethereum badge", () => {
    render(<ChainBadge chain="ethereum" />);
    expect(screen.getByText(/ethereum/i)).toBeInTheDocument();
  });

  it("renders Tempo badge", () => {
    render(<ChainBadge chain="tempo" />);
    expect(screen.getByText(/tempo/i)).toBeInTheDocument();
  });
});
