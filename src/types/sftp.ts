export type TransferStatus = "pending" | "transferring" | "completed" | "failed" | "cancelled";

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
