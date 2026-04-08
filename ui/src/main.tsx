import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import "./globals.css";

// detect system theme
const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
document.documentElement.classList.toggle("dark", prefersDark);

// listen for theme changes from main process
window.api?.onThemeChanged((theme: string) => {
  document.documentElement.classList.toggle("dark", theme === "dark");
});

// also watch OS media query
window.matchMedia("(prefers-color-scheme: dark)").addEventListener("change", (e) => {
  document.documentElement.classList.toggle("dark", e.matches);
});

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <App />
  </StrictMode>
);
