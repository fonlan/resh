import React from 'react';
import { useTranslation } from '../i18n';
import './ConfirmationModal.css';

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

  return (
    <div className="confirmation-modal-overlay">
      <div className="confirmation-modal-container">
        <div className="confirmation-modal-header">
          <h3>{title}</h3>
        </div>
        <div className="confirmation-modal-body">
          <p>{message}</p>
        </div>
        <div className="confirmation-modal-footer">
          <button 
            type="button" 
            className="confirmation-modal-btn cancel" 
            onClick={onCancel}
          >
            {cancelText || t.common.cancel}
          </button>
          <button 
            type="button" 
            className={`confirmation-modal-btn confirm ${type}`} 
            onClick={onConfirm}
          >
            {confirmText || t.common.delete}
          </button>
        </div>
      </div>
    </div>
  );
};
