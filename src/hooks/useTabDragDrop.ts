import { useState, useCallback } from 'react';

export function useTabDragDrop<T>(
  items: T[],
  setItems: (items: T[]) => void
) {
  const [draggedIndex, setDraggedIndex] = useState<number | null>(null);
  const [dropTargetIndex, setDropTargetIndex] = useState<number | null>(null);

  const handleDragStart = useCallback((index: number) => {
    if (items.length <= 1) return;
    setDraggedIndex(index);
  }, [items.length]);

  const handleDragOver = useCallback((e: React.DragEvent, index: number) => {
    e.preventDefault();
    if (draggedIndex !== null && draggedIndex !== index && dropTargetIndex !== index) {
      setDropTargetIndex(index);
    }
  }, [draggedIndex, dropTargetIndex]);

  const handleDrop = useCallback((e: React.DragEvent, dropIndex: number) => {
    e.preventDefault();
    if (
      draggedIndex !== null &&
      draggedIndex !== dropIndex &&
      draggedIndex >= 0 &&
      draggedIndex < items.length &&
      dropIndex >= 0 &&
      dropIndex < items.length
    ) {
      const newItems = [...items];
      const [draggedItem] = newItems.splice(draggedIndex, 1);
      newItems.splice(dropIndex, 0, draggedItem);
      setItems(newItems);
    }
    setDraggedIndex(null);
    setDropTargetIndex(null);
  }, [draggedIndex, items, setItems]);

  const handleDragEnd = useCallback(() => {
    setDraggedIndex(null);
    setDropTargetIndex(null);
  }, []);

  return {
    draggedIndex,
    dropTargetIndex,
    handleDragStart,
    handleDragOver,
    handleDrop,
    handleDragEnd
  };
}
