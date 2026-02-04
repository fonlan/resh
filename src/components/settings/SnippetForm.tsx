import { useState, useImperativeHandle, forwardRef, useRef, useEffect, useMemo, useCallback } from 'react';
import { Snippet } from '../../types/config';
import { validateRequired } from '../../utils/validation';
import { useTranslation } from '../../i18n';
import { ChevronDown } from 'lucide-react';

interface SnippetFormProps {
  snippet?: Snippet;
  existingGroups: string[];
  onSave: (snippet: Snippet) => void;
}

export interface SnippetFormHandle {
  submit: () => void;
  synced: boolean;
  setSynced: (synced: boolean) => void;
}

export const SnippetForm = forwardRef<SnippetFormHandle, SnippetFormProps>(
  ({ snippet, existingGroups, onSave }, ref) => {
  const { t } = useTranslation();
  const [formData, setFormData] = useState<Snippet>(() => {
    if (snippet) {
      return {
        ...snippet,
        group: snippet.group || t.snippetForm.defaultGroup,
        synced: snippet.synced !== undefined ? snippet.synced : true,
        updatedAt: snippet.updatedAt || new Date().toISOString(),
      };
    }
    return {
      id: '',
      name: '',
      content: '',
      description: '',
      group: t.snippetForm.defaultGroup,
      synced: true,
      updatedAt: new Date().toISOString(),
    };
  });

  const [errors, setErrors] = useState<Record<string, string>>({});
  const [showSuggestions, setShowSuggestions] = useState(false);
  const suggestionsRef = useRef<HTMLDivElement>(null);
  
  const allGroups = useMemo(() => {
    const groups = new Set(existingGroups);
    groups.add(t.snippetForm.defaultGroup);
    return Array.from(groups).sort();
  }, [existingGroups, t.snippetForm.defaultGroup]);

  useEffect(() => {
    const handleClickOutside = (event: MouseEvent) => {
      if (suggestionsRef.current && !suggestionsRef.current.contains(event.target as Node)) {
        setShowSuggestions(false);
      }
    };

    if (showSuggestions) {
      document.addEventListener('mousedown', handleClickOutside);
    }
    return () => {
      document.removeEventListener('mousedown', handleClickOutside);
    };
  }, [showSuggestions]);

  const validateForm = useCallback((): boolean => {
    const newErrors: Record<string, string> = {};

    const nameError = validateRequired(formData.name, t.snippetForm.nameLabel);
    if (nameError) newErrors.name = nameError;

    const contentError = validateRequired(formData.content, t.snippetForm.contentLabel);
    if (contentError) newErrors.content = contentError;

    setErrors(newErrors);
    return Object.keys(newErrors).length === 0;
  }, [formData.name, formData.content, t.snippetForm]);

  const handleSave = useCallback(() => {
    if (validateForm()) {
      onSave({
        ...formData,
        group: formData.group || t.snippetForm.defaultGroup,
        updatedAt: new Date().toISOString()
      });
    }
  }, [formData, t.snippetForm.defaultGroup, onSave, validateForm]);

  const handleChange = useCallback((field: keyof Snippet, value: any) => {
    setFormData((prev) => ({
      ...prev,
      [field]: value,
    }));
    setErrors((prev) => {
      if (prev[field]) {
        const newErrors = { ...prev };
        delete newErrors[field];
        return newErrors;
      }
      return prev;
    });
  }, []);

  useImperativeHandle(ref, () => ({
    submit: handleSave,
    synced: formData.synced,
    setSynced: (synced: boolean) => handleChange('synced', synced),
  }), [handleSave, formData.synced, handleChange]);

   return (
     <div className="space-y-4">
       <div className="flex flex-col gap-1.5 mb-4">
         <label htmlFor="snippet-name" className="block text-sm font-medium text-zinc-400 mb-1.5 ">
           {t.snippetForm.nameLabel}
         </label>
         <input
           id="snippet-name"
           type="text"
           value={formData.name}
           onChange={(e) => handleChange('name', e.target.value)}
           placeholder={t.snippetForm.namePlaceholder}
           className={`w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] ${errors.name ? 'border-red-500' : ''}`}
         />
         {errors.name && <p className="text-red-500 text-xs mt-1">{errors.name}</p>}
       </div>

       <div className="flex flex-col gap-1.5 mb-4 relative" ref={suggestionsRef}>
         <label htmlFor="snippet-group" className="block text-sm font-medium text-zinc-400 mb-1.5 ">
           {t.snippetForm.groupLabel}
         </label>
         <div className="relative w-full">
             <input
             id="snippet-group"
             type="text"
             autoComplete="off"
             value={formData.group || ''}
             onChange={(e) => handleChange('group', e.target.value)}
             onFocus={() => setShowSuggestions(true)}
             onClick={() => setShowSuggestions(true)}
             placeholder={t.snippetForm.groupPlaceholder}
             className="w-full px-3 py-2 pr-10 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
             />
             <div
                 className="absolute text-zinc-400 pointer-events-none"
                 style={{
                   right: '12px',
                   top: '50%',
                   transform: 'translateY(-50%)',
                   display: 'flex',
                   alignItems: 'center'
                 }}
             >
                 <ChevronDown size={14} />
             </div>
         </div>

{showSuggestions && (
              <div className="flex flex-wrap gap-1.5 p-2 bg-[var(--bg-primary)] border-[1.5px] border-zinc-700/50 rounded mt-1 max-w-[300px] z-[1000]">
                 {allGroups.map((group) => (
                     <button
                         key={group}
                         type="button"
                         className="p-1 px-2.5 text-xs bg-[var(--bg-primary)] text-[var(--text-muted)] border border-zinc-700/50 rounded cursor-pointer transition-all hover:bg-[var(--bg-elevated)] hover:text-[var(--text-primary)] hover:border-blue-500"
                         onClick={() => {
                             handleChange('group', group);
                             setShowSuggestions(false);
                         }}
                     >
                         {group}
                     </button>
                 ))}
             </div>
         )}
       </div>

       <div className="flex flex-col gap-1.5 mb-4">
         <label htmlFor="snippet-content" className="block text-sm font-medium text-zinc-400 mb-1.5 ">
           {t.snippetForm.contentLabel}
         </label>
         <textarea
           id="snippet-content"
           value={formData.content}
           onChange={(e) => handleChange('content', e.target.value)}
           placeholder={t.snippetForm.contentPlaceholder}
           rows={6}
           className={`w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)] font-mono text-xs ${errors.content ? 'border-red-500' : ''}`}
         />
         {errors.content && <p className="text-red-500 text-xs mt-1">{errors.content}</p>}
       </div>

       <div className="flex flex-col gap-1.5 mb-4">
         <label htmlFor="snippet-description" className="block text-sm font-medium text-zinc-400 mb-1.5 ">
           {t.snippetForm.descriptionLabel}
         </label>
         <input
           id="snippet-description"
           type="text"
           value={formData.description || ''}
           onChange={(e) => handleChange('description', e.target.value)}
           placeholder={t.snippetForm.descriptionPlaceholder}
           className="w-full px-3 py-2 text-sm border border-zinc-700/50 rounded-md outline-none transition-all focus:border-blue-500 focus:shadow-[0_0_20px_rgba(59,130,246,0.2)] disabled:opacity-50 disabled:cursor-not-allowed bg-[var(--bg-primary)] text-[var(--text-primary)] placeholder:text-[var(--text-muted)]"
         />
       </div>
     </div>
   );
});
