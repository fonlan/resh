import React, { useEffect } from 'react';
import { useTransferStore } from '../stores/transferStore';
import { invoke } from '@tauri-apps/api/core';
import { X, ArrowDown, ArrowUp } from 'lucide-react';
import { useTranslation } from '../i18n';

export const TransferStatusPanel: React.FC = () => {
    const { t } = useTranslation();
    const { tasks, initListener } = useTransferStore();

    useEffect(() => {
        initListener();
    }, [initListener]);

    if (tasks.length === 0) return null;

    const handleCancel = (taskId: string) => {
        invoke('sftp_cancel_transfer', { taskId });
    };

    const formatBytes = (bytes: number) => {
        if (bytes === 0) return '0 B';
        const k = 1024;
        const sizes = ['B', 'KB', 'MB', 'GB'];
        const i = Math.floor(Math.log(bytes) / Math.log(k));
        return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
    };

    const formatSpeed = (speed: number) => {
        return `${formatBytes(speed)}/s`;
    };

    const formatETA = (seconds?: number) => {
        if (seconds === undefined || seconds === null) return '--:--';
        if (seconds < 60) return `${seconds}s`;
        const mins = Math.floor(seconds / 60);
        if (mins < 60) return `${mins}m ${seconds % 60}s`;
        const hours = Math.floor(mins / 60);
        return `${hours}h ${mins % 60}m`;
    };

    const getProgressBarClass = (status: string) => {
        if (status === 'failed') return 'bg-[#ff4d4f]';
        if (status === 'completed') return 'bg-[#52c41a]';
        return 'bg-[var(--accent-color,#3c8ce7)]';
    };

    return (
        <div
            className="bg-[var(--sidebar-bg,#1e1e1e)] border-t border-[var(--border-color,#333)] max-h-[200px] overflow-y-auto text-[12px]"
        >
            <div className="px-3 py-2 font-bold bg-[rgba(0,0,0,0.1)] text-[var(--text-color,#ccc)]">
                <span>{t.sftp.transfers} ({tasks.length})</span>
            </div>
            <div className="flex flex-col">
                {tasks.map(task => (
                    <div key={task.task_id} className="flex items-center px-3 py-2 border-b border-[var(--border-color,#333)] gap-2">
                        <div className="text-[var(--accent-color,#3c8ce7)] flex items-center">
                            {task.type_ === 'download' ? <ArrowDown size={16} /> : <ArrowUp size={16} />}
                        </div>
                        <div className="flex-1 overflow-hidden min-w-0">
                            <div className="whitespace-nowrap overflow-hidden text-ellipsis mb-1 text-[var(--text-color,#fff)]" title={task.file_name}>
                                {task.file_name}
                            </div>
                            <div className="flex justify-between text-[10px] text-[var(--text-muted,#888)] mb-1">
                                <span className="transfer-size">{formatBytes(task.transferred_bytes)} / {formatBytes(task.total_bytes)}</span>
                                <span className="transfer-speed">
                                    {formatSpeed(task.speed)} • {formatETA(task.eta)}
                                </span>
                            </div>
                            <div className="h-1 bg-[var(--bg-color-dim,#333)] rounded overflow-hidden">
                                <div
                                    className={`h-full transition-all duration-300 ${getProgressBarClass(task.status)}`}
                                    style={{ width: `${(task.transferred_bytes / task.total_bytes) * 100}%` }}
                                />
                            </div>
                        </div>
                        <button
                            type="button"
                            className="bg-none border-none cursor-pointer text-[var(--text-muted,#888)] p-1 flex items-center justify-center transition-colors duration-200 hover:text-[var(--text-color,#fff)] hover:bg-[rgba(255,255,255,0.1)] rounded"
                            onClick={() => handleCancel(task.task_id)}
                        >
                            <X size={14} />
                        </button>
                    </div>
                ))}
            </div>
        </div>
    );
};
