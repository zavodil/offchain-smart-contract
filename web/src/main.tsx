import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { Report } from './Report';
import './styles.css';
import './report.css';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Report />
  </StrictMode>,
);
