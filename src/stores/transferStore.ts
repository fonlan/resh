import { create } from "zustand"
import { TransferTask, FileConflict } from "../types/sftp"
import { listen } from "@tauri-apps/api/event"

interface TransferState {
  tasks: TransferTask[]
  conflicts: FileConflict[]
  addTask: (task: TransferTask) => void
  updateTask: (task: TransferTask) => void
  removeTask: (taskId: string) => void
  addConflict: (conflict: FileConflict) => void
  removeConflict: (taskId: string) => void
  initListener: () => Promise<() => void>
  listening: boolean
}

export const useTransferStore = create<TransferState>((set, get) => ({
  tasks: [],
  conflicts: [],
  listening: false,
  addTask: (task) =>
    set((state) => {
      if (state.tasks.some((t) => t.task_id === task.task_id)) return state
      return { tasks: [...state.tasks, task] }
    }),
  updateTask: (updatedTask) =>
    set((state) => ({
      tasks: state.tasks.map((t) =>
        t.task_id === updatedTask.task_id ? updatedTask : t,
      ),
    })),
  removeTask: (taskId) =>
    set((state) => ({
      tasks: state.tasks.filter((t) => t.task_id !== taskId),
    })),
  addConflict: (conflict) =>
    set((state) => ({
      conflicts: [...state.conflicts, conflict],
    })),
  removeConflict: (taskId) =>
    set((state) => ({
      conflicts: state.conflicts.filter((c) => c.task_id !== taskId),
    })),
  initListener: async () => {
    if (get().listening) return () => {}

    set({ listening: true })

    const unlistenProgress = await listen<TransferTask>(
      "transfer-progress",
      (event) => {
        const task = event.payload
        const state = get()
        const exists = state.tasks.find((t) => t.task_id === task.task_id)

        if (
          task.status === "completed" ||
          task.status === "cancelled" ||
          task.status === "failed"
        ) {
          if (exists) {
            state.updateTask(task)
          } else {
            state.addTask(task)
          }

          setTimeout(() => {
            get().removeTask(task.task_id)
          }, 3000)
        } else {
          if (exists) {
            state.updateTask(task)
          } else {
            state.addTask(task)
          }
        }
      },
    )

    const unlistenConflict = await listen<FileConflict>(
      "sftp-file-conflict",
      (event) => {
        const conflict = event.payload
        get().addConflict(conflict)
      },
    )

    return () => {
      unlistenProgress()
      unlistenConflict()
    }
  },
}))
