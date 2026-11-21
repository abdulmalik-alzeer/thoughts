// Development page for the diagram component: `npx vite`, then /src/lib/mado/graph/dev/demo.html
import '../../../../theme.css';
import { mount } from 'svelte';
import Demo from './Demo.svelte';

mount(Demo, { target: document.getElementById('demo')! });
