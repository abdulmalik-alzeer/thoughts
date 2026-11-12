// The quick capture box (capture.html): a small window of its own that the desktop app opens over
// any other app. It loads only what it needs — no views, no customisations, no add-ons.
import '../theme.css';
import { mount } from 'svelte';
import Capture from './Capture.svelte';
import { reportError } from '../lib/crash';

window.addEventListener('error', (e) => reportError('capture', e.error ?? e.message));
window.addEventListener('unhandledrejection', (e) => reportError('capture', e.reason));

mount(Capture, { target: document.getElementById('capture')! });
