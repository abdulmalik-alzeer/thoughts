// Development only: a made-up MADO service for looking at the Plan view without the background
// service (see index.html). It answers the routes of docs/mado-spec.md from memory, keeps what the
// view changes, and says so over a made-up events socket — enough to click through every step.
// Nothing here is imported by the app; it is not part of the production build.
import type { BuildState, Comment, GraphDoc, MadoState, PlanDoc, Review, StepId, StepStatus } from '../../../lib/mado/types';
import { STEPS } from '../../../lib/mado/types';

export interface Fake { projectId: string; finishRuns: () => void; rewritePlan: () => void; /** Round 0.4.0: every task done, the build goes on to look at the screens and be reviewed. */ lastRound: () => void }

const PID = '7b0c8f0e-1111-4222-8333-444455556666';
const now = () => new Date().toISOString();

const BRIEF = `# Corner Bakery — order ahead

**For:** the regulars of a small neighbourhood bakery.

People want to order bread and pastries the evening before and pick them up without queueing.
The baker wants to know by 9 pm what to bake.

## What matters most

- Ordering takes under a minute on a phone.
- The baker sees one clear list each evening.
- No online payment in the first version: people pay at pick-up.

## Not now

Delivery, loyalty points, more than one shop.
`;
const SPEC = `# What it does

The first version covers ordering ahead and the baker's evening list.

### F1 — Browse today's bakes

A customer opens the shop page and sees what can be ordered for tomorrow, with a photo, a price
and how many are left. Sold-out items stay visible but cannot be added.

### F2 — Order for pick-up

A customer adds items to a basket, chooses a pick-up time and gives a name and a phone number.
The basket shows a total. No account is needed.

1. Add items to the basket.
2. Choose a pick-up time between 7:00 and 12:00.
3. Confirm with a name and a phone number.

### F3 — The baker's list

At 9 pm ordering closes. The baker sees **one list** of everything to bake, grouped by item,
and a second list of orders by pick-up time. See the [opening hours](https://example.com/hours).

\`\`\`text
Sourdough      x 14
Croissant      x 32
\`\`\`

### F4 — رسالة تأكيد

يتلقى العميل رسالة نصية تؤكد الطلب ووقت الاستلام. تُكتب الرسالة بلغة العميل.
`;
const TECH = `# How it's built

A small web app: one server, one database, pages rendered on the server.

## Parts

| Part | Does |
|------|------|
| Shop pages | What customers see |
| Orders service | Takes and stores orders |
| Baker's board | The evening lists |

## Risks

- Two people ordering the last loaf at the same moment.
`;
const DESIGN = `# Design direction

Warm and plain, like a paper bag: cream background, dark brown text, one accent the colour of crust.
Large tap targets; prices always visible.
`;

const FLOW: GraphDoc = { version: 1, kind: 'flow', title: 'Screens and paths', nodes: [
  { id: 'shop', label: 'Shop', kind: 'screen' }, { id: 'basket', label: 'Basket', kind: 'screen' },
  { id: 'open', label: 'Still open?', kind: 'decision' }, { id: 'confirm', label: 'Confirmation', kind: 'screen' }, { id: 'board', label: "Baker's board", kind: 'screen' },
], edges: [
  { id: 'e1', from: 'shop', to: 'basket', label: 'adds an item' }, { id: 'e2', from: 'basket', to: 'open' },
  { id: 'e3', from: 'open', to: 'confirm', label: 'yes' }, { id: 'e4', from: 'open', to: 'shop', label: 'closed' },
] };
const ARCH: GraphDoc = { version: 1, kind: 'architecture', title: 'Parts of the system', nodes: [
  { id: 'pages', label: 'Shop pages', kind: 'service' }, { id: 'orders', label: 'Orders service', kind: 'service' }, { id: 'db', label: 'Database', kind: 'store' },
], edges: [{ id: 'a1', from: 'pages', to: 'orders' }, { id: 'a2', from: 'orders', to: 'db' }] };
const SCHEMA: GraphDoc = { version: 1, kind: 'erd', title: 'Data', nodes: [
  { id: 'order', label: 'Order', kind: 'entity', props: { fields: [{ name: 'id', type: 'uuid', key: 'pk' }, { name: 'pickup_at', type: 'time' }] } },
  { id: 'item', label: 'Item', kind: 'entity', props: { fields: [{ name: 'id', type: 'uuid', key: 'pk' }, { name: 'price', type: 'money' }] } },
], edges: [{ id: 's1', from: 'order', to: 'item', kind: 'n-n' }] };

const CONVENTIONS = `# Conventions

## A screen

Every page is a server-rendered template in \`src/pages/<name>/\` with one \`load()\` and one template.

## Errors

Show the reason in plain words above the form. Never a bare status code.

## Shared helpers

| Helper | Use it for |
|--------|------------|
| \`money(n)\` | Every price on every screen |
| \`api(path)\` | Every call to the orders service |
`;
const README = `# Corner Bakery

Order ahead, pick up without queueing.

![A loaf](https://example.com/loaf.png)

## Run it

\`\`\`sh
npm install
npm run dev
\`\`\`

Then open the shop page. The baker's board is at \`/board\`.

<script>alert('never runs')</script>

## الخبّاز

تُغلق الطلبات عند الساعة التاسعة مساءً.
`;

const PLAN: PlanDoc = { version: 1, verify: ['npm test', 'npx tsc --noEmit', 'npx eslint .'],
  run: { start: 'npm run dev -- --port 4173', url: 'http://localhost:4173', screens: [{ id: 'shop', path: '/' }, { id: 'basket', path: '/basket' }, { id: 'confirm', path: '/orders/1001' }, { id: 'board', path: '/board' }] },
  tasks: [
  { id: 'contracts', title: 'Examples of every answer', description: 'One example file per answer of the orders service.', owns: ['contracts/**'], dependsOn: [], acceptance: ['npm test -- contracts'], feature: 'F2', kind: 'contracts' },
  { id: 'tests', title: 'Tests of ordering', description: 'The acceptance tests of ordering, written before the feature.', owns: ['tests/orders/**'], dependsOn: ['contracts'], acceptance: ['npm test -- --list'], feature: 'F2', kind: 'tests' },
  { id: 'data', title: 'Database and models', description: 'Tables for items, orders and order lines, with the stock rule.', owns: ['src/db/**', 'migrations/**'], dependsOn: [], acceptance: ['npm run migrate -- --check', 'npm test -- db'], feature: 'F2' },
  { id: 'shop', title: 'Shop page', description: 'Today\'s bakes with photos, prices and what is left.', owns: ['src/pages/shop/**'], dependsOn: ['data'], acceptance: ['npm test -- shop'], feature: 'F1', screens: ['shop'] },
  { id: 'basket', title: 'Basket and ordering', description: 'Basket, pick-up time, name and phone, the 9 pm cut-off.', owns: ['src/pages/basket/**', 'src/orders/**'], dependsOn: ['data'], acceptance: ['npm test -- basket', 'npm test -- orders'], feature: 'F2', screens: ['basket', 'confirm'] },
  { id: 'board', title: "Baker's board", description: 'The two evening lists.', owns: ['src/pages/board/**'], dependsOn: ['data'], acceptance: ['npm test -- board'], feature: 'F3', screens: ['board'] },
  { id: 'sms', title: 'Confirmation message', description: 'A text message in the customer\'s language.', owns: ['src/orders/notify.ts'], dependsOn: ['basket'], acceptance: ['npm test -- notify'], feature: 'F4' },
] };
const TASKS: GraphDoc = { version: 1, kind: 'tasks', title: 'Order of work', nodes: PLAN.tasks.map((t) => ({ id: t.id, label: t.title, kind: 'task' })),
  edges: PLAN.tasks.flatMap((t) => t.dependsOn.map((d) => ({ id: `${d}-${t.id}`, from: d, to: t.id }))) };

function review(blocked: boolean, planHash: string, extraCommand = '', advice = false): Review {
  return {
    features: [{ id: 'F1', name: "Browse today's bakes", tasks: ['shop'] }, { id: 'F2', name: 'Order for pick-up', tasks: ['data', 'basket'] }, { id: 'F3', name: "The baker's list", tasks: ['board'] }, { id: 'F4', name: 'رسالة تأكيد', tasks: blocked ? [] : ['sms'] }],
    screens: [{ id: 'shop', label: 'Shop', tasks: ['shop'] }, { id: 'basket', label: 'Basket', tasks: ['basket'] }, { id: 'confirm', label: 'Confirmation', tasks: ['basket'] }, { id: 'board', label: "Baker's board", tasks: ['board'] }],
    problems: [
      { kind: 'overlap', message: '“Basket and ordering” and “Confirmation message” both change src/orders. They will be built one after another.', tasks: ['basket', 'sms'] },
      ...(advice ? [
        { kind: 'no-contracts' as const, message: 'More than one task, and none writes the examples the parts must agree on.' },
        { kind: 'no-tests' as const, message: 'No task writes the tests before the feature.' },
        { kind: 'no-verify' as const, message: 'Nothing is run on the whole project after a round.' },
        { kind: 'no-run' as const, message: 'The plan does not say how to start the product.' },
        { kind: 'unchecked-screen' as const, message: 'Nobody will look at “Baker\'s board”: it has no address in the plan.' },
      ] : []),
      ...(blocked ? [
        { kind: 'cycle' as const, message: '“Shop page” waits for “Baker\'s board”, which waits for “Shop page”. Neither could ever start.', tasks: ['shop', 'board'] },
        { kind: 'untraced-feature' as const, message: 'Nothing in the plan builds F4.' },
      ] : []),
    ],
    waves: [{ lanes: [{ id: 'w0-a', tasks: ['contracts', 'tests'] }] }, { lanes: [{ id: 'w1-a', tasks: ['data'] }] }, { lanes: [{ id: 'w2-a', tasks: ['shop'] }, { id: 'w2-b', tasks: ['basket', 'sms'] }, { id: 'w2-c', tasks: ['board'] }] }],
    commands: [...new Set([...PLAN.tasks.flatMap((t) => t.acceptance), ...(PLAN.verify ?? []), PLAN.run!.start]), ...(extraCommand ? [extraCommand] : [])],
    planHash,
  };
}

function build(kind: 'idle' | 'building' | 'done'): BuildState {
  const s = (a: string, b: string, c: string, d: string, e: string) => ({ contracts: 'done', tests: 'done', data: a, shop: b, basket: c, sms: d, board: e }) as Record<string, any>;
  const st = kind === 'idle' ? null : kind === 'done' ? s('done', 'done', 'done', 'done', 'done') : s('done', 'running', 'failed', 'pending', 'checking');
  const task = (id: string) => ({ id, status: st![id], chatId: st![id] === 'pending' ? undefined : 'build-' + id, commit: st![id] === 'done' ? '4f9c2ab77d01' : undefined, note: st![id] === 'failed' ? 'npm test -- orders: 2 tests failed after the second try.' : undefined });
  return {
    version: 1, status: kind === 'idle' ? 'idle' : kind === 'done' ? 'done' : 'waiting', branch: kind === 'idle' ? '' : 'mado/build-1', base: kind === 'idle' ? '' : 'main',
    startedAt: kind === 'idle' ? undefined : new Date(Date.now() - 42 * 60000).toISOString(),
    number: 1,
    waves: kind === 'idle' ? [] : [
      { status: 'done', checked: true, verified: true, lanes: [{ id: 'w0-a', status: 'done', tasks: [task('contracts'), task('tests')] }] },
      { status: 'done', checked: true, verified: true, gateFixChatId: 'build-gatefix', lanes: [{ id: 'w1-a', status: 'done', tasks: [task('data')] }] },
      { status: kind === 'done' ? 'done' : 'running', ...(kind === 'done' ? { checked: true, verified: true } : {}), lanes: [{ id: 'w2-a', status: st!.shop, tasks: [task('shop')] }, { id: 'w2-b', status: st!.basket, tasks: [task('basket'), task('sms')] }, { id: 'w2-c', status: st!.board, tasks: [task('board')] }] },
    ],
    governor: { maxParallel: 3, running: kind === 'building' ? 2 : 0, holding: kind === 'building', reason: kind === 'building' ? 'Usage is at 84% of this five-hour window.' : undefined },
    plan_hash: 'h1',
  };
}

const FINDINGS = [
  { severity: 'must' as const, where: 'src/pages/basket/template.html:41', text: 'The pick-up time list starts at 7:00, but F2 says the shop opens at 6:30 on Fridays.\nThe test only covers a Tuesday.' },
  { severity: 'should' as const, where: 'src/pages/board/load.ts', text: 'Prices on the board are formatted by hand; the conventions say every price goes through money().' },
  { severity: 'should' as const, text: 'رسالة التأكيد لا تذكر وقت الاستلام. <b>not markup</b>' },
];
const PLANS_EXTRA = [
  { projectId: 'aaaa1111-2222-4333-8444-555566667777', name: 'مخبز الحي — الطلب المسبق', step: 'spec', stepStatus: 'review', updatedAt: new Date(Date.now() - 3600000).toISOString() },
  { projectId: 'bbbb1111-2222-4333-8444-555566667777', name: 'A project with a very long name that will not fit in the sidebar at all', step: 'build', stepStatus: 'ready', build: 'waiting', updatedAt: new Date(Date.now() - 7200000).toISOString() },
];

const text = (file: string, quote: string, heading?: string) => ({ kind: 'text' as const, file, quote, prefix: '', suffix: '', ...(heading ? { heading } : {}) });
const COMMENTS: Comment[] = [
  { id: 'c1', target: text('spec.md', 'Sold-out items stay visible but cannot be added.', 'F1 — Browse today\'s bakes'), body: 'Also show when they will be back.', createdAt: new Date(Date.now() - 5 * 60000).toISOString(), status: 'open' },
  { id: 'c2', target: text('spec.md', 'between 7:00 and 12:00', 'F2 — Order for pick-up'), body: 'The shop opens at 6:30 on Fridays.', createdAt: new Date(Date.now() - 50 * 60000).toISOString(), status: 'sent' },
  { id: 'c3', target: { kind: 'node', file: 'flow.graph.json', nodeId: 'confirm' }, body: 'Add a way back to the shop from here.', createdAt: new Date(Date.now() - 3 * 3600000).toISOString(), status: 'open' },
  { id: 'c4', target: text('spec.md', 'one list', 'F3 — The baker\'s list'), body: 'Group by oven, not by item.', createdAt: new Date(Date.now() - 26 * 3600000).toISOString(), status: 'resolved' },
  { id: 'c5', target: text('spec.md', 'pay by card in the app'), body: 'Not in the first version.', createdAt: new Date(Date.now() - 30 * 3600000).toISOString(), status: 'orphaned' },
  { id: 'c6', target: { kind: 'task', taskId: 'sms' }, body: 'Use the provider we already have.', createdAt: now(), status: 'open' },
];

function steps(scenario: string): MadoState['steps'] {
  const by: Record<string, StepStatus[]> = {
    early: ['ready', 'locked', 'locked', 'locked', 'locked', 'locked', 'locked'],
    mid: ['approved', 'review', 'drafting', 'ready', 'locked', 'locked', 'locked'],
    review: ['approved', 'approved', 'approved', 'stale', 'approved', 'ready', 'locked'],
    blocked: ['approved', 'approved', 'approved', 'approved', 'approved', 'ready', 'locked'],
    building: ['approved', 'approved', 'approved', 'approved', 'approved', 'approved', 'ready'],
    checking: ['approved', 'approved', 'approved', 'approved', 'approved', 'approved', 'ready'],
    reviewing: ['approved', 'approved', 'approved', 'approved', 'approved', 'approved', 'ready'],
    reviewed: ['approved', 'approved', 'approved', 'approved', 'approved', 'approved', 'ready'],
    landed: ['approved', 'approved', 'approved', 'approved', 'approved', 'approved', 'approved'],
    done: ['approved', 'approved', 'approved', 'approved', 'approved', 'approved', 'ready'],
  };
  const list = by[scenario] ?? by.mid;
  return Object.fromEntries(STEPS.map((id, i) => [id, { status: list[i], ...(list[i] !== 'locked' && !(scenario === 'early') && id !== 'review' && id !== 'build' ? { chatId: 'chat-' + id } : {}) }])) as MadoState['steps'];
}

export function install(scenario: string): Fake {
  const sockets: any[] = [];
  const emit = (e: any) => setTimeout(() => sockets.forEach((s) => s.onmessage?.({ data: JSON.stringify(e) })), 30);
  const mado = (what: string) => emit({ type: 'mado', projectId: PID, what });
  class FakeSocket {
    static OPEN = 1; static CONNECTING = 0;
    readyState = 1; onopen: any; onmessage: any; onclose: any;
    constructor() { sockets.push(this); setTimeout(() => this.onopen?.(), 0); }
    close() {}
  }
  (window as any).WebSocket = FakeSocket;

  const past = (step: StepId) => STEPS.indexOf(step);
  const db = {
    exists: scenario !== 'fresh',
    steps: steps(scenario),
    docs: {} as Record<string, string>,
    graphs: {} as Record<string, GraphDoc>,
    comments: scenario === 'fresh' || scenario === 'early' ? [] : structuredClone(COMMENTS),
    planHash: 'h1', extraCommand: '',
    build: build(scenario === 'building' ? 'building' : ['done', 'checking', 'reviewing', 'reviewed', 'landed'].includes(scenario) ? 'done' : 'idle'),
    chats: {} as Record<string, any>,
    options: { model: null as string | null, effort: null as string | null },
    nonce: '', posted: 0,
  };
  // Round 0.4.0: the phases after the last round.
  const toReview = (round: number) => {
    db.build.status = 'reviewing';
    db.build.review = { status: 'running', round, chatId: 'build-review', findings: [] };
    mado('build'); mado('state');
    later(() => {
      if (round === 1) { db.build.review = { status: 'changes', round: 1, chatId: 'build-review', fixChatId: 'build-reviewfix', findings: structuredClone(FINDINGS) }; mado('build'); later(() => toReview(2)); return; }
      db.build.review = { status: 'changes', round: 2, chatId: 'build-review', fixChatId: 'build-reviewfix', findings: structuredClone(FINDINGS.slice(1)) };
      db.build.status = 'done'; mado('build'); mado('state');
    });
  };
  const toChecking = () => {
    db.build.status = 'checking'; db.build.screens = { status: 'waiting' }; db.nonce = 'n' + Date.now();
    mado('build'); mado('state');
    // Nobody looked (a browser cannot): the service gives up by itself.
    later(() => { if (db.build.status !== 'checking') return; db.nonce = ''; db.build.screens = { status: 'skipped', reason: 'No window of the app looked within two minutes.' }; toReview(1); });
  };
  if (scenario === 'checking') { db.build.status = 'checking'; db.build.screens = { status: 'waiting' }; db.nonce = 'n1'; }
  if (scenario === 'reviewing') { db.build.status = 'reviewing'; db.build.screens = { status: 'done', count: 4, failed: 1 }; db.build.review = { status: 'changes', round: 1, chatId: 'build-review', fixChatId: 'build-reviewfix', findings: structuredClone(FINDINGS) }; }
  if (scenario === 'reviewed' || scenario === 'landed') { db.build.screens = { status: 'done', count: 4, failed: 0 }; db.build.review = { status: 'changes', round: 2, chatId: 'build-review', fixChatId: 'build-reviewfix', findings: structuredClone(FINDINGS.slice(1)) }; }
  if (scenario === 'landed') db.build.landedAt = new Date(Date.now() - 20 * 60000).toISOString();
  const currentStep = () => { let at: StepId = 'brief'; for (const st of STEPS) if (db.steps[st].status !== 'locked') at = st; for (const st of STEPS) if (db.steps[st].status !== 'approved' && db.steps[st].status !== 'locked') { at = st; break; } return at; };
  const reached = (step: StepId) => db.steps[step].status !== 'locked' && db.steps[step].status !== 'ready' && db.steps[step].status !== 'drafting';
  function fill() {
    if (reached('brief')) db.docs.brief = BRIEF;
    if (reached('spec')) { db.docs.spec = SPEC; db.graphs.flow = structuredClone(FLOW); }
    if (reached('tech')) { db.docs.tech = TECH; db.docs.conventions = CONVENTIONS; db.graphs.architecture = structuredClone(ARCH); db.graphs.schema = structuredClone(SCHEMA); }
    if (reached('design')) db.docs.design = DESIGN;
  }
  fill();
  const hasPlan = () => reached('plan');
  const files = () => [...Object.keys(db.docs).map((d) => d + '.md'), ...Object.keys(db.graphs).map((g) => g + '.graph.json'), ...(hasPlan() ? ['plan.json'] : [])];

  function chat(id: string) {
    return (db.chats[id] ??= {
      schemaVersion: 1, id, title: 'Plan · ' + id, pinned: false, archived: false, projectId: PID, kind: 'mado', provider: 'claude', sessionId: null,
      created: now(), updated: now(), options: { model: null, effort: null, permissionMode: 'acceptEdits', web: false }, miniWrap: null, miniWrapAt: null, wrappedAt: null,
      dirty: false, running: id === 'chat-tech' && db.steps.tech.status === 'drafting', interrupted: false, costUsd: 0, meta: null,
      messages: id === 'chat-brief' ? [
        { id: 'm1', role: 'assistant', text: 'Let’s start simple. **What do you want to build, and who is it for?**', ts: now() },
        { id: 'm2', role: 'user', text: 'An order-ahead page for my bakery. For the regulars.', ts: now() },
        { id: 'm3', role: 'assistant', text: 'Good. Two more things:\n\n1. Do people pay online or at pick-up?\n2. By what time do you need to know what to bake?', ts: now() },
        { id: 'm4', role: 'user', text: 'At pick-up. By 9 pm.', ts: now() },
        { id: 'm5', role: 'assistant', text: 'That is enough for a first brief. I have written it: have a look on the left.', ts: now() },
      ] : id === 'chat-spec' ? [{ id: 'm1', role: 'assistant', text: 'I turned the brief into four features and drew how the screens connect. Comment on anything that is off.', ts: now() }] : [],
    });
  }

  const timers: (() => void)[] = [];
  function later(fn: () => void) { timers.push(fn); setTimeout(() => { const i = timers.indexOf(fn); if (i >= 0) { timers.splice(i, 1); fn(); } }, 6000); }
  function finishStep(step: StepId) {
    if (db.steps[step].status !== 'drafting') return;
    db.steps[step] = { ...db.steps[step], status: 'review' };
    fill();
    for (const c of db.comments) if (c.status === 'sent') c.status = 'resolved';
    const c = chat(db.steps[step].chatId!);
    c.running = false;
    const message = { id: 'a' + Date.now(), role: 'assistant', text: 'Done. It is ready for you to read.', ts: now() };
    c.messages.push(message);
    emit({ type: 'chat.message', chatId: c.id, message });
    emit({ type: 'chat.done', chatId: c.id, ok: true, title: c.title, chat: { id: c.id, title: c.title, running: false } });
    mado('state'); mado('doc'); mado('graph'); mado('comments');
  }
  function startRun(step: StepId, note = '') {
    const id = 'chat-' + step;
    db.steps[step] = { status: 'drafting', chatId: id };
    const c = chat(id);
    c.running = true;
    if (note) c.messages.push({ id: 'n' + Date.now(), role: 'user', text: note, ts: now() });
    emit({ type: 'chat.updated', chat: { id, title: c.title, running: true } });
    mado('state');
    later(() => finishStep(step));
    return { chatId: id };
  }

  const json = (body: unknown, status = 200) => new Response(JSON.stringify(body), { status, headers: { 'Content-Type': 'application/json' } });
  const refuse = (status: number, error: string, code = '') => json({ error, code }, status);

  async function handle(method: string, path: string, body: any): Promise<Response> {
    const m = /^\/api\/chats\/([^/]+)(\/send|\/stop|\/ack-interrupted)?$/.exec(path);
    if (m) {
      const c = chat(decodeURIComponent(m[1]));
      if (!m[2]) return json(c);
      if (m[2] === '/send') {
        const message = { id: 'u' + Date.now(), role: 'user', text: String(body?.text ?? ''), ts: now() };
        c.messages.push(message); c.running = true;
        emit({ type: 'chat.message', chatId: c.id, message });
        const words = 'Noted. I will keep that in mind for this step.'.split(' ');
        words.forEach((w, i) => setTimeout(() => emit({ type: 'chat.delta', chatId: c.id, text: (i ? ' ' : '') + w, seq: i + 1 }), 500 + i * 90));
        setTimeout(() => {
          const answer = { id: 'a' + Date.now(), role: 'assistant', text: words.join(' '), ts: now() };
          c.messages.push(answer); c.running = false;
          emit({ type: 'chat.message', chatId: c.id, message: answer });
          emit({ type: 'chat.done', chatId: c.id, ok: true, title: c.title, chat: { id: c.id, title: c.title, running: false } });
        }, 700 + words.length * 90);
      }
      if (m[2] === '/stop') c.running = false;
      return json({});
    }
    if (path === '/api/ai/claude/options') return json({ models: ['opus', 'sonnet', 'haiku'], efforts: ['low', 'medium', 'high', 'xhigh', 'max'], permissionModes: ['acceptEdits', 'plan'] });
    if (path === '/api/meter/chat') return json({ usage: { count: 0, tokens: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }, costUsd: 0 }, chatCostUsd: null, provider: null,
      plans: { claude: { seenAt: Date.now() / 1000, windows: [{ id: 'five_hour', percent: 62, resetsAt: Date.now() / 1000 + 5400, minutes: 300, status: null, seenAt: Date.now() / 1000, hasReset: false }, { id: 'seven_day', percent: 31, resetsAt: Date.now() / 1000 + 300000, minutes: 10080, status: null, seenAt: Date.now() / 1000, hasReset: false }] }, codex: null } });
    if (path === '/api/mado' && method === 'GET') {
      if (!db.exists) return json([]);
      const at = currentStep();
      return json([{ projectId: PID, name: 'Corner Bakery', step: at, stepStatus: db.steps[at].status, ...(db.build.status !== 'idle' ? { build: db.build.status } : {}), updatedAt: now() }, ...PLANS_EXTRA]);
    }
    const base = `/api/mado/${PID}`;
    if (!path.startsWith(base)) return refuse(404, 'Not in the preview');
    const p = path.slice(base.length).split('?')[0];
    if (p === '' && method === 'GET') return json({ project: { id: PID, name: 'Corner Bakery', path: '/Users/you/Projects/corner-bakery' }, exists: db.exists, state: db.exists ? { version: 1, steps: db.steps, plan_hash: db.planHash } : null, files: db.exists ? files() : [] });
    if (p === '/options') {
      if (method === 'PUT') { db.options = { model: body?.model || null, effort: body?.effort || null }; }
      return json(db.options);
    }
    if (p === '/build/screens') {
      if (db.build.status !== 'checking' || !db.nonce) return refuse(409, 'The build is not waiting for anyone to look.');
      if (method === 'GET') return json({ url: PLAN.run!.url, screens: PLAN.run!.screens, nonce: db.nonce });
      if (body?.nonce !== db.nonce) return refuse(409, 'Not the request that is open.');
      db.posted++; db.nonce = '';
      const results = Array.isArray(body?.results) ? body.results : [];
      db.build.screens = { status: 'done', count: results.length, failed: results.filter((r: any) => !r.ok).length };
      toReview(1);
      return json({});
    }
    if (p === '/build/guide') return json({ start: PLAN.run!.start, url: PLAN.run!.url, readme: README });
    if (p === '/start') { db.exists = true; db.steps = steps('early'); mado('state'); return json({}); }
    let x: RegExpExecArray | null;
    if ((x = /^\/doc\/(\w+)$/.exec(p))) {
      if (method === 'PUT') { db.docs[x[1]] = String(body?.text ?? ''); mado('doc'); return json({}); }
      return x[1] in db.docs ? json({ text: db.docs[x[1]] }) : refuse(404, 'Not written yet');
    }
    if ((x = /^\/graph\/(\w+)\/positions$/.exec(p))) return json({});
    if ((x = /^\/graph\/(\w+)$/.exec(p))) {
      if (x[1] === 'tasks') return hasPlan() ? json(TASKS) : refuse(404, 'No plan yet');
      return db.graphs[x[1]] ? json(db.graphs[x[1]]) : refuse(404, 'Not drawn yet');
    }
    if (p === '/plan') return hasPlan() ? json(PLAN) : refuse(404, 'No plan yet');
    if ((x = /^\/steps\/(\w+)\/(run|approve|reopen)$/.exec(p))) {
      const step = x[1] as StepId;
      if (x[2] === 'run') return json(startRun(step, body?.note));
      if (x[2] === 'reopen') { db.steps[step] = { ...db.steps[step], status: 'review' }; for (const s of STEPS) if (past(s) > past(step) && db.steps[s].status === 'approved') db.steps[s] = { ...db.steps[s], status: 'stale' }; mado('state'); return json({}); }
      if (step === 'review' && body?.planHash !== db.planHash) return refuse(409, 'The plan changed since this check was shown.', 'plan_changed');
      db.steps[step] = { ...db.steps[step], status: 'approved', approvedAt: now() };
      const opens: Partial<Record<StepId, StepId[]>> = { brief: ['spec'], spec: ['tech', 'design'], tech: ['plan'], design: ['plan'], plan: ['review'], review: ['build'] };
      for (const next of opens[step] ?? []) {
        const needs = next === 'plan' ? ['tech', 'design'] : [step];
        if (db.steps[next].status === 'locked' && needs.every((n) => db.steps[n as StepId].status === 'approved')) db.steps[next] = { status: 'ready' };
      }
      mado('state');
      return json({});
    }
    if (p === '/comments' && method === 'GET') return json({ version: 1, comments: db.comments });
    if (p === '/comments' && method === 'POST') { const c: Comment = { id: 'c' + Date.now(), target: body.target, body: body.body, createdAt: now(), status: 'open' }; db.comments.push(c); mado('comments'); return json(c); }
    if ((x = /^\/comments\/([\w-]+)$/.exec(p))) {
      const c = db.comments.find((k) => k.id === x![1]);
      if (!c) return refuse(404, 'No such comment');
      if (method === 'DELETE') db.comments = db.comments.filter((k) => k !== c); else Object.assign(c, body);
      mado('comments');
      return json(method === 'DELETE' ? {} : c);
    }
    if (p === '/changes') {
      for (const c of db.comments) if (body.commentIds.includes(c.id)) c.status = 'sent';
      const r = startRun(body.step, [body.note, `(${body.commentIds.length} comments, ${(body.graphOps ?? []).reduce((n: number, g: any) => n + g.ops.length, 0)} diagram changes)`].filter(Boolean).join(' '));
      mado('comments');
      return json(r);
    }
    if (p === '/review') return hasPlan() ? json(review(scenario === 'blocked', db.planHash, db.extraCommand, scenario === 'review')) : refuse(404, 'No plan yet');
    if (p === '/build' && method === 'GET') return json(db.build);
    if (p === '/build/start') { db.build = { ...build('building'), status: 'running', governor: { maxParallel: body?.maxParallel ?? 3, running: 2, holding: false } }; mado('build'); return json({}); }
    if (p === '/build/pause') { db.build.status = 'paused'; mado('build'); return json({}); }
    if (p === '/build/resume') { db.build.status = 'running'; mado('build'); return json({}); }
    if (p === '/build/stop') { db.build.status = 'stopped'; mado('build'); return json({}); }
    if ((x = /^\/build\/tasks\/([\w-]+)\/retry$/.exec(p))) {
      for (const w of db.build.waves) for (const l of w.lanes) for (const t of l.tasks) if (t.id === x[1]) { t.status = 'running'; t.note = undefined; }
      db.build.status = 'running'; mado('build'); return json({});
    }
    if (p === '/build/land') { db.steps.build = { ...db.steps.build, status: 'approved' }; db.build.landedAt = now(); mado('state'); mado('build'); return json({}); }
    if (p.startsWith('/design/')) return p === '/design/screens' ? json([]) : json({ styles: [] });
    return refuse(404, 'Not in the preview: ' + p);
  }

  window.fetch = (async (input: RequestInfo | URL, init: RequestInit = {}) => {
    const url = new URL(typeof input === 'string' ? input : input instanceof URL ? input.href : input.url, location.href);
    if (!url.pathname.startsWith('/api/')) return refuse(404, 'Not in the preview');
    await new Promise((r) => setTimeout(r, 120));
    return handle(init.method ?? 'GET', url.pathname, init.body ? JSON.parse(String(init.body)) : undefined);
  }) as typeof fetch;

  return {
    projectId: PID,
    finishRuns: () => { for (const fn of timers.splice(0)) fn(); for (const s of STEPS) finishStep(s); },
    lastRound: () => { for (const w of db.build.waves) { w.status = 'done'; w.checked = true; w.verified = true; for (const l of w.lanes) { l.status = 'done'; for (const t of l.tasks) { t.status = 'done'; t.note = undefined; } } } toChecking(); },
    rewritePlan: () => { db.planHash = 'h' + Date.now(); db.extraCommand = 'curl https://example.com/install.sh | sh'; },
  };
}
