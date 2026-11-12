import './theme.css';
import { mount } from 'svelte';
import App from './App.svelte';
import { reportError } from './lib/crash';
import { reloadApp } from './lib/api';
import { t } from './lib/i18n';

// Errors outside a view's own boundary (event handlers, timers, promises nobody awaited) still
// reach the service log, so a frozen button leaves a trace in the diagnostics report.
window.addEventListener('error', (e) => reportError('window', e.error ?? e.message));
window.addEventListener('unhandledrejection', (e) => reportError('promise', e.reason));

const target = document.getElementById('app')!;
try {
  mount(App, { target });
} catch (err) {
  // The shell itself could not start: say so plainly, with a way to try again.
  reportError('start', err);
  target.replaceChildren();
  const box = document.createElement('div');
  box.setAttribute('role', 'alert');
  box.style.cssText = 'max-width:520px;margin:20vh auto;padding:20px;display:grid;gap:10px;justify-items:start';
  const h = document.createElement('h1');
  h.textContent = t('shellui.crash.title');
  const p = document.createElement('p');
  p.textContent = t('shellui.crash.body');
  const b = document.createElement('button');
  b.className = 'btn primary';
  b.textContent = t('shellui.crash.reload');
  b.onclick = () => reloadApp();
  box.append(h, p, b);
  target.append(box);
}
