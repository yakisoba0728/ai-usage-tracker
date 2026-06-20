import i18n from "i18next";
import LanguageDetector from "i18next-browser-languagedetector";
import { initReactI18next } from "react-i18next";

import en from "@/locales/en.json";
import ko from "@/locales/ko.json";

/**
 * App localization. English + Korean catalogs are bundled inline (small static
 * set), the language is detected from a persisted choice then the webview
 * locale, and the user's explicit pick is cached to localStorage. `changeLanguage`
 * persists automatically via the detector's localStorage cache.
 */
void i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      ko: { translation: ko },
    },
    fallbackLng: "en",
    supportedLngs: ["en", "ko"],
    load: "languageOnly", // "ko-KR" -> "ko"
    nonExplicitSupportedLngs: true,
    interpolation: { escapeValue: false }, // React already escapes
    detection: {
      order: ["localStorage", "navigator"],
      caches: ["localStorage"],
      lookupLocalStorage: "ait-lang",
    },
  });

export default i18n;
