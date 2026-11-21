// Development only (see index.html). Query flags: ?scenario=fresh|early|mid|review|blocked|building|done
// &lang=en|ar &mode=light|dark &theme=saudi|contrast|tech &motion=reduce
import '../../../theme.css';
import { mount } from 'svelte';
import { install } from './fake';

if (!import.meta.env.DEV) throw new Error('The Plan view preview is for development only');

const q = new URLSearchParams(location.search);
const flags = { scenario: q.get('scenario') ?? 'mid', lang: q.get('lang') ?? 'en', mode: q.get('mode') ?? 'light', theme: q.get('theme') ?? 'saudi', motion: q.get('motion') ?? '' };
// The app's client reads the service's address from the query once; give it a made-up one.
q.set('port', '1'); q.set('token', 'dev'); // the flags stay in the address, so a reload shows the same thing
history.replaceState(null, '', `${location.pathname}?${q}`);
const fake = install(flags.scenario);
(window as any).__madoFake = fake; // for poking at it from the console

const { app, applyTheme } = await import('../../../lib/store.svelte');
app.settings = { language: flags.lang, theme: flags.theme, mode: flags.mode, accessibility: flags.motion === 'reduce' ? { zoom: 100, reduceMotion: true, underlineLinks: false, strongFocus: false, textSpacing: false } : null };
app.ready = true;
applyTheme();
const { default: DevShell } = await import('./DevShell.svelte');
mount(DevShell, { target: document.getElementById('app')!, props: { fake, flags } });
