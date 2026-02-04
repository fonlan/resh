import { useState, useImperativeHandle, forwardRef, useRef } from 'react';
import { Authentication } from '../../types/config';
import { validateRequired, validateUniqueName } from '../../utils/validation';
import { useTranslation } from '../../i18n';
import { Upload } from 'lucide-react';

interface AuthFormProps {
  auth?: Authentication;
  existingNames: string[];
  onSave: (auth: Authentication) => void;
}

export interface AuthFormHandle {
  submit: () => void;
  synced: boolean;
  setSynced: (synced: boolean) => void;
}

export const AuthForm = forwardRef<AuthFormHandle, AuthFormProps>(
  ({ auth, existingNames, onSave }, ref) => {
  const { t } = useTranslation();
  const [formData, setFormData] = useState<Authentication>(() => {
    if (auth) {
      return {
        ...auth,
        synced: auth.synced !== undefined ? auth.synced : true,
        updatedAt: auth.updatedAt || new Date().toISOString(),
      };
    }
    return {
      id: '',
      name: '',
      type: 'password',
      password: '',
      synced: true,
      updatedAt: new Date().toISOString(),
    };
  });

  const [errors, setErrors] = useState<Record<string, string>>({});
  const fileInputRef = useRef<HTMLInputElement>(null);

  const validateForm = (): boolean => {
    const newErrors: Record<string, string> = {};

    const nameError = validateRequired(formData.name, t.authForm.nameLabel);
    if (nameError) newErrors.name = nameError;

    const uniqueError = validateUniqueName(formData.name, existingNames, auth?.name);
    if (uniqueError) newErrors.name = uniqueError;

    if (formData.type === 'password') {
      const passwordError = validateRequired(formData.password, t.authForm.passwordLabel);
      if (passwordError) newErrors.password = passwordError;
    } else {
      const keyContentError = validateRequired(formData.keyContent || '', t.authForm.keyContentLabel);
      if (keyContentError) newErrors.keyContent = keyContentError;
    }

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

  // Expose submit method to parent via ref
  useImperativeHandle(ref, () => ({
    submit: handleSave,
    synced: formData.synced,
    setSynced: (synced: boolean) => handleChange('synced', synced),
  }));

  const handleChange = (field: keyof Authentication, value: any) => {
    setFormData((prev) => ({
      ...prev,
      [field]: value,
    }));
    // Clear error for this field when user starts typing
    if (errors[field]) {
      setErrors((prev) => {
        const newErrors = { ...prev };
        delete newErrors[field];
        return newErrors;
      });
    }
  };

  const handleTypeChange = (newType: 'password' | 'key') => {
    handleChange('type', newType);
    // Clear validation errors for the switched-out fields
    setErrors((prev) => {
      const newErrors = { ...prev };
      if (newType === 'password') {
        delete newErrors.keyContent;
        delete newErrors.passphrase;
      } else {
        delete newErrors.password;
      }
      return newErrors;
    });
  };

  const handleFileUpload = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    const reader = new FileReader();
    reader.onload = (event) => {
      const content = event.target?.result as string;
      if (content) {
        handleChange('keyContent', content);
      }
    };
    reader.readAsText(file);
    // Reset input so the same file can be selected again
    e.target.value = '';
  };

  return (
    <div className="flex flex-col gap-4">
      {/* Name */}
      <div>
        <label className="block text-sm font-medium text-[var(--text-secondary)] mb-1">
          {t.authForm.nameLabel}
        </label>
        <input
          type="text"
          value={formData.name}
          onChange={(e) => handleChange('name', e.target.value)}
          placeholder={t.authForm.namePlaceholder}
          className={`w-full px-3 py-2 rounded-[var(--radius-sm)] bg-[var(--bg-primary)] border ${
            errors.name ? 'border-[var(--color-danger)]' : 'border-[var(--glass-border)]'
          } text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:border-[var(--accent-primary)] focus:shadow-[var(--glow-primary)] transition-all duration-[150ms]`}
        />
        {errors.name && <p className="text-[var(--color-danger)] text-xs mt-1">{errors.name}</p>}
      </div>

      {/* Type Toggle */}
      <div>
        <label className="block text-sm font-medium text-[var(--text-secondary)] mb-2">
          {t.authForm.typeLabel}
        </label>
        <div className="flex gap-4">
          {(['password', 'key'] as const).map((type) => (
            <label key={type} className="flex items-center gap-2 cursor-pointer">
              <input
                type="radio"
                name="authType"
                value={type}
                checked={formData.type === type}
                onChange={() => handleTypeChange(type)}
                className="appearance-none -webkit-appearance-none w-[18px] h-[18px] border border-[1.5px] border-[var(--glass-border)] rounded-full bg-[var(--bg-primary)] cursor-pointer relative transition-all duration-[150ms] flex-shrink-0 inline-flex items-center justify-center vertical-align-middle checked:border-[var(--accent-primary)] checked:shadow-[var(--glow-primary)] hover:border-[var(--accent-primary)] hover:bg-[var(--bg-tertiary)] focus:outline-none focus:shadow-[0_0_0_3px_rgba(59,130,246,0.2)] after:content-[''] after:checked:w-2 after:checked:h-2 after:checked:bg-[var(--accent-primary)] after:checked:rounded-full"
              />
              <span className="text-[var(--text-secondary)] capitalize">
                {type === 'key' ? t.authTab.keyType : t.authTab.passwordType}
              </span>
            </label>
          ))}
        </div>
      </div>

      {/* Password Authentication Fields */}
      {formData.type === 'password' && (
        <>
          <div>
            <label className="block text-sm font-medium text-[var(--text-secondary)] mb-1">
              {t.authForm.passwordLabel}
            </label>
            <input
              type="password"
              value={formData.password || ''}
              onChange={(e) => handleChange('password', e.target.value)}
              placeholder={t.authForm.passwordPlaceholder}
              className={`w-full px-3 py-2 rounded-[var(--radius-sm)] bg-[var(--bg-primary)] border ${
                errors.password ? 'border-[var(--color-danger)]' : 'border-[var(--glass-border)]'
              } text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:border-[var(--accent-primary)] focus:shadow-[var(--glow-primary)] transition-all duration-[150ms]`}
            />
            {errors.password && <p className="text-[var(--color-danger)] text-xs mt-1">{errors.password}</p>}
          </div>
        </>
      )}

      {/* SSH Key Authentication Fields */}
      {formData.type === 'key' && (
        <>
          <div>
            <div className="flex justify-between items-center mb-1">
              <label className="block text-sm font-medium text-[var(--text-secondary)]">
                {t.authForm.keyContentLabel}
              </label>
              <button
                type="button"
                onClick={() => fileInputRef.current?.click()}
                className="inline-flex items-center justify-center gap-2 px-4 py-2 text-sm font-medium bg-[var(--bg-tertiary)] text-[var(--text-primary)] border border-[1.5px] border-[var(--glass-border)] rounded-[var(--radius-sm)] cursor-pointer transition-all duration-[150ms] whitespace-nowrap font-[var(--font-ui)] hover:bg-[var(--bg-elevated)] hover:border-[var(--accent-primary)] py-1 px-2 text-xs flex gap-1.5"
                title={t.authForm.uploadKey}
              >
                <Upload size={14} />
                <span>{t.authForm.uploadKey}</span>
              </button>
              <input
                type="file"
                ref={fileInputRef}
                onChange={handleFileUpload}
                className="hidden"
              />
            </div>
            <textarea
              value={formData.keyContent || ''}
              onChange={(e) => handleChange('keyContent', e.target.value)}
              placeholder={t.authForm.keyContentPlaceholder}
              rows={6}
              className={`w-full px-3 py-2 rounded-[var(--radius-sm)] bg-[var(--bg-primary)] border ${
                errors.keyContent ? 'border-[var(--color-danger)]' : 'border-[var(--glass-border)]'
              } text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:border-[var(--accent-primary)] focus:shadow-[var(--glow-primary)] transition-all duration-[150ms] font-[var(--font-mono)] text-xs`}
            />
            {errors.keyContent && <p className="text-[var(--color-danger)] text-xs mt-1">{errors.keyContent}</p>}
          </div>

          <div>
            <label className="block text-sm font-medium text-[var(--text-secondary)] mb-1">
              {t.authForm.passphraseLabel}
            </label>
            <input
              type="password"
              value={formData.passphrase || ''}
              onChange={(e) => handleChange('passphrase', e.target.value)}
              placeholder={t.authForm.passphrasePlaceholder}
              className="w-full px-3 py-2 rounded-[var(--radius-sm)] bg-[var(--bg-primary)] border border-[var(--glass-border)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] focus:outline-none focus:border-[var(--accent-primary)] focus:shadow-[var(--glow-primary)] transition-all duration-[150ms]"
            />
          </div>
        </>
      )}
    </div>
  );
});
