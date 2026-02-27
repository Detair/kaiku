/**
 * TableRowSkeleton - Loading skeleton for admin table rows
 *
 * Displays animated placeholder content while data is loading.
 */

import { Component, For } from "solid-js";
import Skeleton from "@/components/ui/Skeleton";

interface TableRowSkeletonProps {
  /** Number of columns in the table */
  columns: number;
  /** Number of skeleton rows to display */
  rows?: number;
  /** Whether to show an avatar skeleton in the first column */
  showAvatar?: boolean;
}

const TableRowSkeleton: Component<TableRowSkeletonProps> = (props) => {
  const rows = () => props.rows ?? 5;

  return (
    <For each={Array(rows()).fill(0)}>
      {() => (
        <div
          class="grid gap-4 px-4 py-3 border-b border-white/5"
          style={{
            "grid-template-columns": `repeat(${props.columns}, minmax(0, 1fr))`,
          }}
        >
          <For each={Array(props.columns).fill(0)}>
            {(_, colIndex) => (
              <div class="flex items-center gap-3">
                {colIndex() === 0 && props.showAvatar && (
                  <Skeleton width="32px" height="32px" circle />
                )}
                <Skeleton
                  width={
                    colIndex() === 0
                      ? "60%"
                      : colIndex() === props.columns - 1
                        ? "50px"
                        : "70%"
                  }
                  height="16px"
                />
              </div>
            )}
          </For>
        </div>
      )}
    </For>
  );
};

export default TableRowSkeleton;
