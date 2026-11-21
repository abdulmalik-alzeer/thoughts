// Round 0.4.0: a project's own text (README, conventions) is untrusted. It goes through the app's
// markdown renderer, and pictures are taken out.
import { describe, it, expect, vi } from 'vitest';

vi.mock('../i18n', () => ({ t: (k: string) => k }));
const { projectDocHtml } = await import('./readme');

describe('a project’s README', () => {
  it('is drawn as markdown with nothing that runs or loads', () => {
    const html = projectDocHtml('# Shop\n\n<script>alert(1)</script>\n\n<img src="https://evil.example/t.png" onerror="alert(1)">\n\n![A loaf](https://evil.example/loaf.png)\n\n<iframe src="https://evil.example"></iframe>\n\n[site](javascript:alert(1))\n\n<b onclick="x()">bold</b>');
    expect(html).toContain('<h1');
    expect(html).not.toMatch(/<script|<img|<iframe|onerror|onclick|javascript:/i);
    expect(html).not.toContain('evil.example/loaf.png');
    expect(html).toContain('[A loaf]');
    expect(html).toContain('<b>bold</b>');
  });
  it('is cut to the size the contract names', () => {
    const html = projectDocHtml('a'.repeat(9000) + ' END');
    expect(html).not.toContain('END');
  });
  it('leaves a text without pictures as the renderer made it', () => {
    expect(projectDocHtml('Run `npm start`.')).toContain('<code');
  });
});
