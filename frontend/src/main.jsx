import React from 'react';
import { createRoot } from 'react-dom/client';
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
