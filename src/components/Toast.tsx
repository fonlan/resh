import React, { useEffect, useState } from "react"
import { X, AlertCircle, CheckCircle, Info } from "lucide-react"

export interface ToastItem {
  id: string
  type: "success" | "error" | "info" | "warning"
  message: string
  duration?: number
}

interface ToastContainerProps {
  toasts: ToastItem[]
  onRemove: (id: string) => void
}

export const ToastContainer: React.FC<ToastContainerProps> = ({
  toasts,
  onRemove,
}) => {
  if (toasts.length === 0) return null

  return (
    <div className="toast-container">
      {toasts.map((toast) => (
        <ToastItem key={toast.id} toast={toast} onRemove={onRemove} />
      ))}
    </div>
  )
}

const ToastItem: React.FC<{
  toast: ToastItem
  onRemove: (id: string) => void
}> = ({ toast, onRemove }) => {
  const [isExiting, setIsExiting] = useState(false)

  useEffect(() => {
    const duration = toast.duration || 4000
    const exitTimeout = setTimeout(() => {
      setIsExiting(true)
      setTimeout(() => onRemove(toast.id), 200)
    }, duration - 200)

    return () => clearTimeout(exitTimeout)
  }, [toast.id, toast.duration, onRemove])

  const icons = {
    success: <CheckCircle size={20} className="text-green-500" />,
    error: <AlertCircle size={20} className="text-red-500" />,
    info: <Info size={20} className="text-blue-500" />,
    warning: <AlertCircle size={20} className="text-yellow-500" />,
  }

  const bgColors = {
    success:
      "bg-green-50 dark:bg-green-900/20 border-green-200 dark:border-green-800",
    error: "bg-red-50 dark:bg-red-900/20 border-red-200 dark:border-red-800",
    info: "bg-blue-50 dark:bg-blue-900/20 border-blue-200 dark:border-blue-800",
    warning:
      "bg-yellow-50 dark:bg-yellow-900/20 border-yellow-200 dark:border-yellow-800",
  }

  return (
    <div
      className={`toast-item ${bgColors[toast.type]} ${isExiting ? "exiting" : ""}`}
    >
      <div className="toast-icon">{icons[toast.type]}</div>
      <div className="toast-message">{toast.message}</div>
      <button
        type="button"
        className="toast-close"
        onClick={() => onRemove(toast.id)}
      >
        <X size={16} />
      </button>
    </div>
  )
}

// Simple toast context for global access
interface ToastContextType {
  showToast: (message: string, type?: ToastItem["type"]) => void
}

export const ToastContext = React.createContext<ToastContextType | null>(null)

export const useToast = () => {
  const context = React.useContext(ToastContext)
  if (!context) {
    throw new Error("useToast must be used within a ToastProvider")
  }
  return context
}
