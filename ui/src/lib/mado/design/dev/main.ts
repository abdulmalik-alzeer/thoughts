// Dev only (see index.html).
import '../../../../theme.css';
import { mount } from 'svelte';
import Demo from './Demo.svelte';

mount(Demo, { target: document.getElementById('app')! });
