// Focus timer — example plugin. `thoughts` is the plugin API (see app-layer README).
export default function (thoughts) {
  thoughts.addView({
    id: 'focus', title: 'Focus', icon: '⏱',
    render(el) {
      let left = 25 * 60, timer = null;
      el.innerHTML = '<div style="padding:32px;text-align:center"><div id="t" style="font-size:64px;font-variant-numeric:tabular-nums"></div><button id="b" class="btn primary">Start</button></div>';
      const t = el.querySelector('#t'), b = el.querySelector('#b');
      const draw = () => { t.textContent = `${String(Math.floor(left / 60)).padStart(2, '0')}:${String(left % 60).padStart(2, '0')}`; };
      b.onclick = () => {
        if (timer) { clearInterval(timer); timer = null; b.textContent = 'Start'; return; }
        b.textContent = 'Pause';
        timer = setInterval(() => { left--; draw(); if (left <= 0) { clearInterval(timer); thoughts.toast('Focus session done'); left = 25 * 60; } }, 1000);
      };
      draw();
      return () => clearInterval(timer);
    },
  });
}
