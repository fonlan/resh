import React, { useEffect } from 'react';
import { useTransferStore } from '../stores/transferStore';
import { invoke } from '@tauri-apps/api/core';
import { X, ArrowDown, ArrowUp } from 'lucide-react';
import './TransferStatusPanel.css';

export const TransferStatusPanel: React.FC = () => {
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

    return (
        <div className="transfer-panel">
            <div className="transfer-header">
                <span>Transfers ({tasks.length})</span>
            </div>
            <div className="transfer-list">
                {tasks.map(task => (
                    <div key={task.task_id} className="transfer-item">
                        <div className="transfer-icon">
                            {task.type_ === 'download' ? <ArrowDown size={16} /> : <ArrowUp size={16} />}
                        </div>
                        <div className="transfer-info">
                            <div className="transfer-name" title={task.file_name}>{task.file_name}</div>
                            <div className="transfer-details">
                                <span className="transfer-size">{formatBytes(task.transferred_bytes)} / {formatBytes(task.total_bytes)}</span>
                                <span className="transfer-speed">
                                    {formatSpeed(task.speed)} • {formatETA(task.eta)}
                                </span>
                            </div>
                            <div className="progress-bar-container">
                                <div 
                                    className={`progress-bar ${task.status === 'failed' ? 'failed' : ''} ${task.status === 'completed' ? 'completed' : ''}`} 
                                    style={{ width: `${(task.transferred_bytes / task.total_bytes) * 100}%` }}
                                />
                            </div>
                        </div>
                        <button type="button" className="cancel-btn" onClick={() => handleCancel(task.task_id)}>
                            <X size={14} />
                        </button>
                    </div>
                ))}
            </div>
        </div>
    );
};
