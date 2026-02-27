/**
 * ChannelDragContext - Drag-and-drop state management for channels and categories
 *
 * Provides reactive state for drag operations including:
 * - Tracking what is being dragged (channel or category)
 * - Tracking drop targets and positions
 * - Helper functions for drag operations
 *
 * Supports:
 * - Dragging channels within a category (reorder)
 * - Dragging channels between categories (move)
 * - Dragging categories to reorder them
 * - Dragging a category into another to make it a subcategory (2-level max)
 */

import { createStore } from "solid-js/store";

export type DraggableType = "channel" | "category";
export type DropPosition = "before" | "after" | "inside";

interface DragState {
  /** Whether a drag operation is in progress */
  isDragging: boolean;
  /** ID of the item being dragged */
  draggingId: string | null;
  /** Type of the item being dragged */
  draggingType: DraggableType | null;
  /** ID of the current drop target */
  dropTargetId: string | null;
  /** Type of the drop target */
  dropTargetType: DraggableType | null;
  /** Position relative to drop target (before, after, or inside for categories) */
  dropPosition: DropPosition | null;
}

export interface DragResult {
  sourceId: string | null;
  sourceType: DraggableType | null;
  targetId: string | null;
  targetType: DraggableType | null;
  position: DropPosition | null;
}

const [dragState, setDragState] = createStore<DragState>({
  isDragging: false,
  draggingId: null,
  draggingType: null,
  dropTargetId: null,
  dropTargetType: null,
  dropPosition: null,
});

/**
 * Start a drag operation
 */
export function startDrag(id: string, type: DraggableType): void {
  setDragState({
    isDragging: true,
    draggingId: id,
    draggingType: type,
    dropTargetId: null,
    dropTargetType: null,
    dropPosition: null,
  });
}

/**
 * Update the current drop target
 */
export function setDropTarget(
  id: string | null,
  type: DraggableType | null,
  position: DropPosition | null,
): void {
  // Don't set drop target to the item being dragged
  if (id === dragState.draggingId) {
    setDragState({
      dropTargetId: null,
      dropTargetType: null,
      dropPosition: null,
    });
    return;
  }

  setDragState({
    dropTargetId: id,
    dropTargetType: type,
    dropPosition: position,
  });
}

/**
 * End the drag operation and reset state
 */
export function endDrag(): void {
  setDragState({
    isDragging: false,
    draggingId: null,
    draggingType: null,
    dropTargetId: null,
    dropTargetType: null,
    dropPosition: null,
  });
}

/**
 * Get the current drag result for handling the drop
 */
export function getDragResult(): DragResult {
  return {
    sourceId: dragState.draggingId,
    sourceType: dragState.draggingType,
    targetId: dragState.dropTargetId,
    targetType: dragState.dropTargetType,
    position: dragState.dropPosition,
  };
}

/**
 * Check if a specific item is the drop target with a specific position
 */
export function isDropTarget(id: string, position?: DropPosition): boolean {
  if (dragState.dropTargetId !== id) return false;
  if (position && dragState.dropPosition !== position) return false;
  return true;
}

/**
 * Check if a specific item is being dragged
 */
export function isDraggingItem(id: string): boolean {
  return dragState.draggingId === id;
}

/**
 * Calculate drop position based on mouse position relative to element
 * For categories, the middle zone indicates "inside" (nesting)
 * For channels, only "before" and "after" are used
 */
export function calculateDropPosition(
  e: DragEvent,
  element: HTMLElement,
  targetType: DraggableType,
): DropPosition {
  const rect = element.getBoundingClientRect();
  const y = e.clientY - rect.top;
  const height = rect.height;

  // Categories can accept "inside" drops (for nesting)
  if (targetType === "category") {
    // Top 25% = before, middle 50% = inside, bottom 25% = after
    if (y < height * 0.25) {
      return "before";
    } else if (y > height * 0.75) {
      return "after";
    } else {
      return "inside";
    }
  }

  // Channels only support before/after
  return y < height / 2 ? "before" : "after";
}

/**
 * Validate if a drop operation is allowed
 * Returns false if the operation would violate constraints
 */
export function isValidDrop(
  sourceId: string,
  sourceType: DraggableType,
  targetId: string,
  targetType: DraggableType,
  position: DropPosition,
  isSubcategory: boolean,
): boolean {
  // Can't drop on self
  if (sourceId === targetId) return false;

  // Category nesting validation
  if (
    sourceType === "category" &&
    targetType === "category" &&
    position === "inside"
  ) {
    // Can't nest into a subcategory (2-level max)
    if (isSubcategory) return false;
  }

  return true;
}

export { dragState };
