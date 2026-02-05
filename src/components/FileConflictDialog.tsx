import { FileConflict, ConflictResolution } from '../types/sftp'

interface FileConflictDialogProps {
  conflict: FileConflict
  onResolve: (resolution: ConflictResolution) => void
}

function FileConflictDialog({ conflict, onResolve }: FileConflictDialogProps) {
  const formatSize = (bytes?: number) => {
    if (!bytes) return '未知'
    const kb = bytes / 1024
    const mb = kb / 1024
    const gb = mb / 1024
    if (gb >= 1) return `${gb.toFixed(2)} GB`
    if (mb >= 1) return `${mb.toFixed(2)} MB`
    return `${kb.toFixed(2)} KB`
  }

  const formatDate = (timestamp?: number) => {
    if (!timestamp) return '未知'
    return new Date(timestamp * 1000).toLocaleString('zh-CN', {
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit'
    })
  }

  const fileName = conflict.file_path.split('/').pop() || conflict.file_path

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="bg-[var(--bg-secondary)] border border-[var(--border)] rounded-lg shadow-lg w-[500px] max-w-[90vw]">
        <div className="p-4 border-b border-[var(--border)]">
          <h3 className="text-lg font-semibold text-[var(--text-primary)]">
            文件已存在
          </h3>
          <p className="text-sm text-[var(--text-secondary)] mt-1 break-all">
            {fileName}
          </p>
        </div>

        <div className="p-4 space-y-4">
          <div className="grid grid-cols-2 gap-4">
            <div className="border border-[var(--border)] rounded p-3 bg-[var(--bg-primary)]">
              <h4 className="text-sm font-semibold text-[var(--text-primary)] mb-2">
                本地文件
              </h4>
              <div className="text-sm space-y-1">
                <div className="text-[var(--text-secondary)]">
                  大小: <span className="text-[var(--text-primary)]">{formatSize(conflict.local_size)}</span>
                </div>
                <div className="text-[var(--text-secondary)]">
                  修改时间:
                </div>
                <div className="text-xs text-[var(--text-primary)]">
                  {formatDate(conflict.local_modified)}
                </div>
              </div>
            </div>

            <div className="border border-[var(--border)] rounded p-3 bg-[var(--bg-primary)]">
              <h4 className="text-sm font-semibold text-[var(--text-primary)] mb-2">
                远程文件
              </h4>
              <div className="text-sm space-y-1">
                <div className="text-[var(--text-secondary)]">
                  大小: <span className="text-[var(--text-primary)]">{formatSize(conflict.remote_size)}</span>
                </div>
                <div className="text-[var(--text-secondary)]">
                  修改时间:
                </div>
                <div className="text-xs text-[var(--text-primary)]">
                  {formatDate(conflict.remote_modified)}
                </div>
              </div>
            </div>
          </div>

          <div className="text-sm text-[var(--text-secondary)]">
            远程路径: <span className="text-[var(--text-primary)] break-all">{conflict.file_path}</span>
          </div>
        </div>

        <div className="p-4 border-t border-[var(--border)] flex gap-2 justify-end">
          <button
            type="button"
            onClick={() => onResolve('skip')}
            className="px-4 py-2 text-sm rounded border border-[var(--border)] hover:bg-[var(--bg-hover)] text-[var(--text-primary)] transition-colors"
          >
            跳过
          </button>
          <button
            type="button"
            onClick={() => onResolve('cancel')}
            className="px-4 py-2 text-sm rounded border border-[var(--border)] hover:bg-[var(--bg-hover)] text-[var(--text-primary)] transition-colors"
          >
            取消所有
          </button>
          <button
            type="button"
            onClick={() => onResolve('overwrite')}
            className="px-4 py-2 text-sm rounded bg-blue-600 hover:bg-blue-700 text-white transition-colors"
          >
            覆盖
          </button>
        </div>
      </div>
    </div>
  )
}

export default FileConflictDialog
