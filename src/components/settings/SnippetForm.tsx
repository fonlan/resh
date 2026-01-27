import { useState, useImperativeHandle, forwardRef } from 'react';
import { Snippet } from '../../types/config';
import { validateRequired } from '../../utils/validation';
import { useTranslation } from '../../i18n';

interface SnippetFormProps {
  snippet?: Snippet;
  onSave: (snippet: Snippet) => void;
}

export interface SnippetFormHandle {
  submit: () => void;
  synced: boolean;
  setSynced: (synced: boolean) => void;
}

export const SnippetForm = forwardRef<SnippetFormHandle, SnippetFormProps>(
  ({ snippet, onSave }, ref) => {
  const { t } = useTranslation();
  const [formData, setFormData] = useState<Snippet>(() => {
    if (snippet) {
      return {
        ...snippet,
        synced: snippet.synced !== undefined ? snippet.synced : true,
        updatedAt: snippet.updatedAt || new Date().toISOString(),
      };
    }
    return {
      id: '',
      name: '',
      content: '',
      description: '',
      synced: true,
      updatedAt: new Date().toISOString(),
    };
  });

  const [errors, setErrors] = useState<Record<string, string>>({});

  const validateForm = (): boolean => {
    const newErrors: Record<string, string> = {};

    const nameError = validateRequired(formData.name, t.snippetForm.nameLabel);
    if (nameError) newErrors.name = nameError;

    const contentError = validateRequired(formData.content, t.snippetForm.contentLabel);
    if (contentError) newErrors.content = contentError;

    setErrors(newErrors);
    return Object.keys(newErrors).length === 0;
  };

  const handleSave = () => {
    if (validateForm()) {
      onSave({
        ...formData,
        updatedAt: new Date().toISOString()
      });
    }
  };

  const handleChange = (field: keyof Snippet, value: any) => {
    setFormData((prev) => ({
      ...prev,
      [field]: value,
    }));
    if (errors[field]) {
      setErrors((prev) => {
        const newErrors = { ...prev };
        delete newErrors[field];
        return newErrors;
      });
    }
  };

  useImperativeHandle(ref, () => ({
    submit: handleSave,
    synced: formData.synced,
    setSynced: (synced: boolean) => handleChange('synced', synced),
  }));

  return (
    <div className="space-y-4">
      <div>
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {t.snippetForm.nameLabel}
        </label>
        <input
          type="text"
          value={formData.name}
          onChange={(e) => handleChange('name', e.target.value)}
          placeholder={t.snippetForm.namePlaceholder}
          className={`w-full px-3 py-2 rounded-md bg-gray-800 border ${
            errors.name ? 'border-red-500' : 'border-gray-600'
          } text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500`}
        />
        {errors.name && <p className="text-red-400 text-xs mt-1">{errors.name}</p>}
      </div>

      <div>
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {t.snippetForm.contentLabel}
        </label>
        <textarea
          value={formData.content}
          onChange={(e) => handleChange('content', e.target.value)}
          placeholder={t.snippetForm.contentPlaceholder}
          rows={6}
          className={`w-full px-3 py-2 rounded-md bg-gray-800 border ${
            errors.content ? 'border-red-500' : 'border-gray-600'
          } text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500 font-mono text-xs`}
        />
        {errors.content && <p className="text-red-400 text-xs mt-1">{errors.content}</p>}
      </div>

      <div>
        <label className="block text-sm font-medium text-gray-300 mb-1">
          {t.snippetForm.descriptionLabel}
        </label>
        <input
          type="text"
          value={formData.description || ''}
          onChange={(e) => handleChange('description', e.target.value)}
          placeholder={t.snippetForm.descriptionPlaceholder}
          className="w-full px-3 py-2 rounded-md bg-gray-800 border border-gray-600 text-white placeholder-gray-500 focus:outline-none focus:ring-2 focus:ring-blue-500"
        />
      </div>
    </div>
  );
});
