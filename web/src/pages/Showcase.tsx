import { useEffect } from 'react';
import { PageNav, SeeNext } from '../components/PageNav';
import { Step } from '../components/StepView';
import { RuleTable } from '../components/Tables';
import { accountLink, useBuild, usePack, useRun } from '../data';
import { DEMO } from '../lib/network';

export type ShowcaseKey = 'grants' | 'markets' | 'attacks';

interface Showcase {
  prefix: string;
  tag: string;
  title: string;
  intro: string;
  pack?: string;
  facts: [string, string, string?][];
}

const SHOWCASES: Record<ShowcaseKey, Showcase> = {
  grants: {
    prefix: 'G',
    tag: 'Contract 1 · USDC',
    title: 'A digital-art grant fund',
    intro:
      'The fund holds USDC and pays grants for digital art. Every application goes to the policy gate under the fund\'s own rulebook. A pass under $250 is paid in the same callback that reads the verdict; a larger one, or a rule that escalates, goes to the council; a block refuses it with the rule that fired.',
    pack: 'grants-digital-art@2',
    facts: [
      ['Contract', DEMO.fund, accountLink(DEMO.fund)],
      ['Pays in', `USDC (${DEMO.token}), up to $250 on its own`],
      ['Council', DEMO.council, accountLink(DEMO.council)],
      ['Rulebook', 'grants/digital-art@2, published by the fund'],
    ],
  },
  markets: {
    prefix: 'M',
    tag: 'Contract 2 · NEAR bond',
    title: 'A prediction-market factory',
    intro:
      'The factory creates prediction markets for a 1 NEAR bond. It sends every listing to the policy gate under its rulebook. A pass opens the market; a block means the market never exists and the bond goes back; a political question, such as an election, goes through moderation.',
    pack: 'markets-listing@1',
    facts: [
      ['Contract', DEMO.markets, accountLink(DEMO.markets)],
      ['Bond', '1 NEAR, returned when a market is refused'],
      ['Moderator', DEMO.moderator, accountLink(DEMO.moderator)],
      ['Rulebook', 'markets/listing@1, published by the factory'],
    ],
  },
  attacks: {
    prefix: 'E',
    tag: 'Adversarial',
    title: 'Trying to get around it',
    intro: 'Each step tries to get a decision without the rules, or to change one after the fact. Each one fails the way it should.',
    facts: [['Gate', DEMO.gate, accountLink(DEMO.gate)]],
  },
};

export function ShowcasePage({ which, focus }: { which: ShowcaseKey; focus?: string }) {
  const run = useRun();
  const build = useBuild();
  const s = SHOWCASES[which];
  const pack = usePack(s.pack ?? '');
  const steps = (run?.steps ?? []).filter((x) => x.id.startsWith(s.prefix));
  const b = run?.deployment.build ?? build;

  useEffect(() => {
    if (focus && steps.length) document.getElementById(`step-${focus}`)?.scrollIntoView({ behavior: 'smooth' });
    else if (!focus) window.scrollTo(0, 0);
  }, [focus, steps.length]);

  return (
    <div className="page report">
      <PageNav current={which} />

      <header className="showcase-head">
        <span className="showcase-tag">{s.tag}</span>
        <h1>{s.title}</h1>
        <p className="lede">{s.intro}</p>
        <dl className="facts-row">
          {s.facts.map(([label, value, href]) => (
            <div key={label}>
              <dt>{label}</dt>
              <dd>
                {href ? (
                  <a href={href} target="_blank" rel="noreferrer">
                    {value}
                  </a>
                ) : (
                  value
                )}
              </dd>
            </div>
          ))}
        </dl>
      </header>

      {s.pack && (
        <section className="report-section">
          <h2>Its rulebook</h2>
          <p>Each rule is one yes/no question to Sage. It fires when the probability reaches its threshold, and the strictest fired action decides.</p>
          <RuleTable pack={pack} />
        </section>
      )}

      <section className="report-section">
        <h2>{which === 'attacks' ? 'The attempts' : 'The transactions'}</h2>
        <div className="step-index">
          {steps.map((x) => (
            <a key={x.id} href={`#/${which}/${x.id}`}>
              <b>{x.id}</b> {x.title}
            </a>
          ))}
        </div>
      </section>

      {steps.map((x) => (
        <Step key={x.id} step={x} build={b} />
      ))}

      <SeeNext current={which} />
      <footer className="footer">
        Built on <a href="https://outlayer.ai">OutLayer</a>, <a href="https://cloud.near.ai">NEAR AI Cloud</a> and NEAR.
      </footer>
    </div>
  );
}
