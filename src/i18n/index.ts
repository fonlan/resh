import { locales, LocaleType, TranslationKeys } from './locales';
import { useConfig } from '../hooks/useConfig';

export const useTranslation = () => {
  const { config } = useConfig();
  const language = (config?.general.language || 'en') as LocaleType;
  
  const t = locales[language] || locales.en;

  return { t, language };
};
