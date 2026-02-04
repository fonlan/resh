import React from 'react';
import { useTranslation } from '../i18n';

interface ConfirmationModalProps {
  isOpen: boolean;
  title: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  onConfirm: () => void;
  onCancel: () => void;
  type?: 'danger' | 'info' | 'warning';
}

export const ConfirmationModal: React.FC<ConfirmationModalProps> = ({
  isOpen,
  title,
  message,
  confirmText,
  cancelText,
  onConfirm,
  onCancel,
  type = 'info'
}) => {
  const { t } = useTranslation();

  if (!isOpen) return null;

  const getConfirmButtonStyle = () => {
    switch (type) {
      case 'danger':
        return {
          background: '#ef4444',
          boxShadow: '0 4px 12px rgba(239, 68, 68, 0.2)'
        };
      case 'warning':
        return {
          background: '#f59e0b',
          boxShadow: '0 4px 12px rgba(245, 158, 11, 0.2)'
        };
      default:
        return {
          background: 'var(--accent-primary)',
          boxShadow: '0 4px 12px rgba(59, 130, 246, 0.2)'
        };
    }
  };

  return (
    <div
      className="fixed inset-0 flex items-center justify-center z-[2000] animate-in fade-in duration-200"
      style={{
        background: 'rgba(2, 6, 23, 0.6)',
        backdropFilter: 'blur(8px) saturate(150%)'
      }}
    >
      <div
        className="relative bg-[var(--bg-secondary)] rounded-lg max-w-[400px] w-[calc(100%-32px)] overflow-hidden animate-in slide-in-from-bottom-2 duration-300"
        style={{
          boxShadow: '0 25px 50px -12px rgba(0, 0, 0, 0.6), 0 0 0 1px var(--glass-border)'
        }}
      >
        <div
          className="px-5 py-4 border-b border-[var(--glass-border)]"
          style={{ background: 'rgba(255, 255, 255, 0.02)' }}
        >
          <h3 className="text-[16px] font-bold text-[var(--text-primary)] tracking-[-0.01em] m-0">{title}</h3>
        </div>
        <div className="p-5 text-[var(--text-secondary)] text-[14px] leading-relaxed">
          <p>{message}</p>
        </div>
        <div
          className="px-5 py-3 border-t border-[var(--glass-border)] flex justify-end gap-3"
          style={{ background: 'rgba(255, 255, 255, 0.02)' }}
        >
          <button
            type="button"
            className="px-4 py-2 rounded text-[13px] font-semibold cursor-pointer transition-all duration-200 border-none bg-transparent border border-[var(--glass-border)] text-[var(--text-secondary)] hover:bg-[var(--bg-tertiary)] hover:text-[var(--text-primary)]"
            onClick={onCancel}
          >
            {cancelText || t.common.cancel}
          </button>
          <button
            type="button"
            className="px-4 py-2 rounded text-[13px] font-semibold cursor-pointer transition-all duration-200 border-none text-white hover:brightness-110 hover:-translate-y-0.5"
            style={getConfirmButtonStyle()}
            onClick={onConfirm}
          >
            {confirmText || t.common.delete}
          </button>
        </div>
      </div>
    </div>
  );
};
