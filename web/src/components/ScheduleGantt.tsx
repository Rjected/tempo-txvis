import { useState } from "react";
import type { ScheduleItem, TxNode } from "../types";

interface ScheduleGanttProps {
  schedule: ScheduleItem[];
  txNodes: TxNode[];
  width?: number;
  height?: number;
}

const LANE_HEIGHT = 40;
const LANE_PADDING = 4;
const LEFT_MARGIN = 60;
const TOP_MARGIN = 30;

export default function ScheduleGantt({
  schedule,
  txNodes,
  width = 800,
  height: heightProp,
}: ScheduleGanttProps) {
  const [tooltip, setTooltip] = useState<{
    x: number;
    y: number;
    item: ScheduleItem;
    tx: TxNode | undefined;
  } | null>(null);

  const maxLane = Math.max(...schedule.map((s) => s.lane), 0);
  const maxTime = Math.max(...schedule.map((s) => s.start + s.duration), 1);
  const laneCount = maxLane + 1;
  const chartHeight = heightProp ?? TOP_MARGIN + laneCount * LANE_HEIGHT + 20;
  const chartWidth = width - LEFT_MARGIN - 20;

  const xScale = (time: number) => LEFT_MARGIN + (time / maxTime) * chartWidth;
  const barWidth = (duration: number) => (duration / maxTime) * chartWidth;

  return (
    <div className="relative">
      <svg
        viewBox={`0 0 ${width} ${chartHeight}`}
        className="w-full rounded-lg border bg-white"
      >
        {/* Y-axis lane labels */}
        {Array.from({ length: laneCount }, (_, i) => (
          <text
            key={`lane-${i}`}
            x={LEFT_MARGIN - 8}
            y={TOP_MARGIN + i * LANE_HEIGHT + LANE_HEIGHT / 2 + 4}
            textAnchor="end"
            fontSize={11}
            fill="#6b7280"
          >
            Lane {i}
          </text>
        ))}

        {/* X-axis label */}
        <text
          x={width / 2}
          y={chartHeight - 2}
          textAnchor="middle"
          fontSize={10}
          fill="#9ca3af"
        >
          Gas Units
        </text>

        {/* Lane background stripes */}
        {Array.from({ length: laneCount }, (_, i) => (
          <rect
            key={`bg-${i}`}
            x={LEFT_MARGIN}
            y={TOP_MARGIN + i * LANE_HEIGHT}
            width={chartWidth}
            height={LANE_HEIGHT}
            fill={i % 2 === 0 ? "#f9fafb" : "#f3f4f6"}
          />
        ))}

        {/* Bars */}
        {schedule.map((item) => {
          const tx = txNodes.find((t) => t.txIndex === item.txIndex);
          const x = xScale(item.start);
          const y = TOP_MARGIN + item.lane * LANE_HEIGHT + LANE_PADDING;
          const w = barWidth(item.duration);
          const h = LANE_HEIGHT - LANE_PADDING * 2;

          return (
            <g
              key={item.txIndex}
              data-testid={`gantt-bar-${item.txIndex}`}
              data-critical={String(item.isCritical)}
              onMouseOver={(e) =>
                setTooltip({ x: e.clientX, y: e.clientY, item, tx })
              }
              onMouseOut={() => setTooltip(null)}
              className="cursor-pointer"
            >
              <rect
                x={x}
                y={y}
                width={w}
                height={h}
                rx={3}
                fill={item.isCritical ? "#ef4444" : "#4a9eff"}
                opacity={0.85}
              />
              <text
                x={x + w / 2}
                y={y + h / 2 + 4}
                textAnchor="middle"
                fontSize={10}
                fill="#fff"
                fontWeight="bold"
              >
                Tx {item.txIndex}
              </text>
            </g>
          );
        })}
      </svg>

      {/* Tooltip */}
      {tooltip && (
        <div
          className="pointer-events-none fixed z-50 rounded bg-gray-900 px-3 py-2 text-xs text-white shadow"
          style={{ left: tooltip.x + 10, top: tooltip.y + 10 }}
        >
          <div className="font-medium">Tx {tooltip.item.txIndex}</div>
          <div>Start: {tooltip.item.start.toLocaleString()}</div>
          <div>Duration: {tooltip.item.duration.toLocaleString()}</div>
          <div>Lane: {tooltip.item.lane}</div>
          {tooltip.item.isCritical && (
            <div className="mt-1 text-red-300">Critical Path</div>
          )}
          {tooltip.tx && (
            <>
              <div className="mt-1">Gas: {tooltip.tx.gasUsed.toLocaleString()}</div>
              <div>Hash: {tooltip.tx.txHash.slice(0, 10)}…</div>
            </>
          )}
        </div>
      )}
    </div>
  );
}
