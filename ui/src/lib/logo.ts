// The Facet mark: a cut gem on a rounded tile. One source for the in-app logo,
// the generated static icon, and the theme-coloured Dock icon.

export type LogoColors = { a: string; b: string; bg1: string; bg2: string; ink: string };

export function gemPaths(c: { a: string; b: string; ink: string }): string {
  return `
  <polygon points="50,18 74,30 80,56 64,80 36,80 20,56 26,30" fill="${c.a}"/>
  <polygon points="50,18 74,30 56,46" fill="${c.b}"/>
  <polygon points="50,18 56,46 26,30" fill="${c.b}" opacity=".55"/>
  <polygon points="26,30 56,46 20,56" fill="${c.ink}" opacity=".12"/>
  <polygon points="74,30 80,56 56,46" fill="${c.ink}" opacity=".28"/>
  <polygon points="56,46 80,56 64,80" fill="#000" opacity=".18"/>
  <polygon points="20,56 56,46 36,80" fill="#000" opacity=".08"/>
  <polygon points="56,46 64,80 36,80" fill="#000" opacity=".28"/>`;
}

/** Full app icon (macOS grid: 824px tile inside a 1024 canvas ≈ 80.5%). */
export function iconSvg(c: LogoColors, px = 1024): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${px}" height="${px}" viewBox="0 0 100 100">
  <defs><linearGradient id="g" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${c.bg1}"/><stop offset="1" stop-color="${c.bg2}"/></linearGradient></defs>
  <rect x="9.75" y="9.75" width="80.5" height="80.5" rx="18" fill="url(#g)"/>
  <rect x="10" y="10" width="80" height="80" rx="17.8" fill="none" stroke="${c.ink}" stroke-opacity=".12" stroke-width=".5"/>
  <g transform="translate(50 50) scale(.78) translate(-50 -50)">${gemPaths(c)}</g></svg>`;
}

/** Read the active theme's colours from CSS variables. */
export function themeColors(): LogoColors {
  const s = getComputedStyle(document.documentElement);
  const v = (n: string) => s.getPropertyValue(n).trim();
  const light = document.documentElement.dataset.mode === 'light';
  return { a: v('--accent'), b: v('--accent-2'), ink: v('--fg'),
    bg1: light ? v('--panel') : v('--panel-2'), bg2: light ? v('--panel-2') : v('--bg') };
}

/** Rasterise the themed icon and hand it to the native shell as the Dock icon (Tauri on macOS only). */
export async function syncDockIcon() {
  const w = window as any;
  if (!w.__TAURI_INTERNALS__ || !/Mac/.test(navigator.userAgent)) return;
  const svg = iconSvg(themeColors(), 512);
  const img = new Image();
  img.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(svg);
  await img.decode();
  const cv = document.createElement('canvas'); cv.width = cv.height = 512;
  cv.getContext('2d')!.drawImage(img, 0, 0, 512, 512);
  const blob: Blob = await new Promise((r) => cv.toBlob((b) => r(b!), 'image/png'));
  const bytes = Array.from(new Uint8Array(await blob.arrayBuffer()));
  const { invoke } = await import('@tauri-apps/api/core');
  await invoke('set_dock_icon', { png: bytes }).catch(() => {});
}
