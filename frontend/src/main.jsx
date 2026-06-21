import React from 'react';
import { createRoot } from 'react-dom/client';
// Self-hosted fonts (bundled + served same-origin) — replaces the per-view
// Google Fonts @import so the UI loads no third-party assets (privacy +
// air-gapped use) and lets the CSP drop fonts.googleapis.com / fonts.gstatic.com.
import '@fontsource/inter/400.css';
import '@fontsource/inter/500.css';
import '@fontsource/inter/600.css';
import '@fontsource/inter/700.css';
import '@fontsource/jetbrains-mono/400.css';
import '@fontsource/jetbrains-mono/500.css';
import './index.css';
import App from './App.jsx';
import { loadTheme } from './lib/theme.js';
import { loadResponsive } from './lib/responsive.js';

loadTheme();
loadResponsive();

createRoot(document.getElementById('root')).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
);
