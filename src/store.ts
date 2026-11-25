import { create } from "zustand";
export const pages = [
  "Обзор",
  "Устройства",
  "Конвейер",
  "Кнопки",
  "Подключения",
  "Диагностика",
  "Настройки",
] as const;
export type Page = (typeof pages)[number];
export const useUi = create<{ page: Page; setPage: (page: Page) => void }>(
  (set) => ({ page: "Обзор", setPage: (page) => set({ page }) }),
);
