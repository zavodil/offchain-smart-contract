import { useEffect, useState } from 'react';
import { Landing } from './pages/Landing';
import { BuildPage } from './pages/Build';
import { VerifyPage } from './pages/Verify';
import { ShowcasePage, type ShowcaseKey } from './pages/Showcase';

const PAGES: ShowcaseKey[] = ['grants', 'markets', 'attacks'];

/** `#/grants`, `#/markets/M2`: a page, and a step on it. Anything else is the overview. */
function route(hash: string): { page: ShowcaseKey | null; focus?: string } {
  const [, page, focus] = hash.replace(/^#/, '').split('/');
  return PAGES.includes(page as ShowcaseKey) ? { page: page as ShowcaseKey, focus } : { page: null };
}

export function App() {
  const [hash, setHash] = useState(window.location.hash);
  useEffect(() => {
    const onChange = () => setHash(window.location.hash);
    window.addEventListener('hashchange', onChange);
    return () => window.removeEventListener('hashchange', onChange);
  }, []);
  if (hash.startsWith('#/build')) return <BuildPage />;
  if (hash.startsWith('#/verify')) return <VerifyPage />;
  const { page, focus } = route(hash);
  return page ? <ShowcasePage which={page} focus={focus} /> : <Landing />;
}
