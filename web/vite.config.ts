import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// Relative asset paths: the report works from any folder it is served under
// (GitHub Pages serves it under /<repo>/).
export default defineConfig({
  base: './',
  plugins: [react()],
  build: { target: 'es2022' },
});
