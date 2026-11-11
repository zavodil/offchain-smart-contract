/** The same navigation on every page: the overview, the three showcases, the guide, the check. */
const LINKS: [string, string, string][] = [
  ['overview', '#/', 'Overview'],
  ['grants', '#/grants', 'Grant fund'],
  ['markets', '#/markets', 'Market factory'],
  ['attacks', '#/attacks', 'Getting around it'],
  ['build', '#/build', 'Build one'],
  ['check', '#/verify', 'Verify it'],
];

export function PageNav({ current }: { current: string }) {
  return (
    <nav className="page-nav">
      <a href="#/" className="brand">
        <span className="brand-mark" />
        <span>Off-chain smart contracts</span>
      </a>
      <span>
        {LINKS.map(([key, href, label]) => (
          <a key={key} href={href} className={current === key ? 'on' : ''}>
            {label}
          </a>
        ))}
      </span>
    </nav>
  );
}

const NEXT: [string, string, string, string][] = [
  ['grants', '#/grants', 'A digital-art grant fund', 'USDC grants paid, refused or sent to a council by the fund’s own rulebook. 8 live transactions.'],
  ['markets', '#/markets', 'A prediction-market factory', 'Markets that never open on a person’s death or harm; political questions go through moderation. 6 live transactions.'],
  ['attacks', '#/attacks', 'Getting around it', 'A withdrawn rulebook, the judge without the gate, a forged callback, a forged verdict. Each one fails.'],
  ['build', '#/build', 'Build one', 'How a rulebook is written, calibrated and published, what the judge does in the enclave, and how your contract calls the gate.'],
  ['calibration', '#calibration', 'Calibration', 'Every rule’s probability on every test case, and the two findings that shaped the published rulebooks.'],
  ['check', '#/verify', 'Verify it yourself', 'Every verdict the contracts hold, checked in your browser: signature, the gate on chain, the rulebook\'s hash, Intel\'s attestation.'],
];

/** The bottom of every page: where to go next. */
export function SeeNext({ current }: { current: string }) {
  const onOverview = current === 'overview';
  return (
    <section className="see-next">
      <h2>See next</h2>
      <div className="see-next-tiles">
        {NEXT.filter(([key]) => key !== current && !(onOverview && (key === 'calibration' || key === 'check'))).map(([key, href, title, text]) => {
          const anchor = href.startsWith('#/') ? null : href.slice(1);
          return (
            <a
              key={key}
              href={anchor && !onOverview ? '#/' : href}
              className="see-next-tile"
              onClick={anchor && !onOverview ? () => setTimeout(() => document.getElementById(anchor)?.scrollIntoView(), 50) : undefined}
            >
              <b>{title} →</b>
              <span>{text}</span>
            </a>
          );
        })}
      </div>
    </section>
  );
}
