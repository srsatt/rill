export type Theme = "light" | "dark";

const storageKey = "rill-theme";

export function currentTheme(): Theme {
  return document.documentElement.classList.contains("dark") ? "dark" : "light";
}

export function setTheme(theme: Theme): void {
  document.documentElement.classList.toggle("dark", theme === "dark");
  document.documentElement.style.colorScheme = theme;
  try {
    localStorage.setItem(storageKey, theme);
  } catch {
    // Theme still applies when storage is unavailable.
  }
}

export function initializeTheme(): void {
  let theme: Theme = "light";
  try {
    if (localStorage.getItem(storageKey) === "dark") theme = "dark";
  } catch {
    // Keep the light default when storage is unavailable.
  }
  setTheme(theme);
}
