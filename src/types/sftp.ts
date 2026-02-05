export type TransferStatus = "pending" | "transferring" | "completed" | "failed" | "cancelled";

export type ConflictResolution = "overwrite" | "skip" | "cancel";

export interface TransferTask {
    task_id: string;
    type_: "download" | "upload";
    session_id: string;
    file_name: string;
    source: string;
    destination: string;
    total_bytes: number;
    transferred_bytes: number;
    speed: number; // bytes per second
    eta?: number; // seconds remaining
    status: TransferStatus;
    error?: string;
}

export interface FileConflict {
    task_id: string;
    session_id: string;
    file_path: string;
    local_size?: number;
    remote_size?: number;
    local_modified?: number;
    remote_modified?: number;
}
