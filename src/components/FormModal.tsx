import React from 'react';
import { useTranslation } from '../i18n';
import './FormModal.css';

interface FormModalProps {
  isOpen: boolean;
  title: string;
  children: React.ReactNode;
  onSubmit: () => void | Promise<void>;
  onClose: () => void;
  isLoading?: boolean;
  submitText?: string;
  extraFooterContent?: React.ReactNode;
}

export const FormModal: React.FC<FormModalProps> = ({
  isOpen,
  title,
  children,
  onSubmit,
  onClose,
  isLoading = false,
  submitText,
  extraFooterContent,
}) => {
  const { t } = useTranslation();
  const [loading, setLoading] = React.useState(false);

  const handleSubmit = async () => {
    setLoading(true);
    try {
      await onSubmit();
    } finally {
      setLoading(false);
    }
  };

  if (!isOpen) {
    return null;
  }

  const effectiveSubmitText = submitText || t.common.save;

  return (
    <div className="form-modal-overlay">
      <div className="form-modal-container">
        {/* Header */}
        <div className="form-modal-header">
          <h2>{title}</h2>
        </div>

        {/* Content */}
        <div className="form-modal-content">
          {children}
        </div>

        {/* Footer */}
        <div className="form-modal-footer">
          {extraFooterContent}
          <div className="flex gap-3">
            <button
              type="button"
              onClick={onClose}
              disabled={loading || isLoading}
              className="form-modal-cancel-btn"
            >
              {t.common.cancel}
            </button>
            <button
              type="button"
              onClick={handleSubmit}
              disabled={loading || isLoading}
              className="form-modal-submit-btn"
            >
              {(loading || isLoading) && <span className="inline-block animate-spin">⟳</span>}
              {effectiveSubmitText}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
};
