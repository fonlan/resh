import React from 'react';
import { useTranslation } from '../i18n';

interface FormModalProps {
  isOpen: boolean;
  title: string;
  children: React.ReactNode;
  onSubmit: () => void | Promise<void>;
  onClose: () => void;
  isLoading?: boolean;
  submitText?: string;
}

export const FormModal: React.FC<FormModalProps> = ({
  isOpen,
  title,
  children,
  onSubmit,
  onClose,
  isLoading = false,
  submitText,
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
    <div className="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div className="bg-gray-900 rounded-lg shadow-lg max-w-2xl w-full mx-4">
        {/* Header */}
        <div className="border-b border-gray-700 p-6">
          <h2 className="text-xl font-semibold text-white">{title}</h2>
        </div>

        {/* Content */}
        <div className="p-6 max-h-96 overflow-y-auto">
          {children}
        </div>

        {/* Footer */}
        <div className="border-t border-gray-700 p-6 flex justify-end gap-3">
          <button
            type="button"
            onClick={onClose}
            disabled={loading || isLoading}
            className="px-4 py-2 rounded-md border border-gray-600 text-gray-300 hover:bg-gray-800 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            {t.common.cancel}
          </button>
          <button
            type="button"
            onClick={handleSubmit}
            disabled={loading || isLoading}
            className="px-4 py-2 rounded-md bg-blue-600 text-white hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center gap-2"
          >
            {(loading || isLoading) && <span className="inline-block animate-spin">⟳</span>}
            {effectiveSubmitText}
          </button>
        </div>
      </div>
    </div>
  );
};
