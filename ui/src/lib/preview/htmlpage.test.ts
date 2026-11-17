import { describe, expect, it } from 'vitest';
import { PAGE_CSP, safePage } from './htmlpage';

describe('safePage', () => {
  const evil = `<!doctype html><html><head><meta http-equiv="refresh" content="0;url=https://x.example/">
    <base href="https://x.example/"><link rel="stylesheet" href="https://x.example/a.css"><script>alert(1)</script>
    <style>body { color: red }</style><title>T</title></head>
    <body onload="alert(2)"><h1 id="top">Hello</h1><a href="https://x.example/">out</a><a href="#top">up</a>
    <a href="javascript:alert(3)">js</a><img src="https://x.example/p.png" srcset="https://x.example/2.png 2x">
    <img src="data:image/png;base64,iVBORw0KGgo="><iframe src="https://x.example/"></iframe>
    <form action="https://x.example/"><input name="q"><button>go</button></form>
    <svg><use href="https://x.example/s.svg#a"></use><a href="https://x.example/">s</a></svg>
    <video src="https://x.example/v.mp4"></video><object data="https://x.example/o"></object></body></html>`;
  const out = safePage(evil);

  it('starts with the policy, before anything of the page', () => {
    expect(out.startsWith(`<!doctype html><meta http-equiv="Content-Security-Policy" content="${PAGE_CSP}">`)).toBe(true);
    expect(PAGE_CSP).toContain("default-src 'none'");
  });
  it('keeps the readable page and its styles', () => {
    expect(out).toContain('Hello');
    expect(out).toContain('color: red');
    expect(out).toContain('href="#top"');
    expect(out).toContain('src="data:image/png;base64,');
  });
  it('leaves no address to load or go to, and nothing that runs', () => {
    const rest = out.slice(out.indexOf('<meta charset="utf-8">'));
    expect(rest).not.toMatch(/x\.example/);
    expect(rest).not.toMatch(/<script|onload|javascript:|<iframe|<form|<input|<object|<video|<base|<link|http-equiv/i);
  });
});
