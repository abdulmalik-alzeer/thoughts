// A board of your items by status: a ready-made pack that comes with Thoughts. Dragging a card to
// another column (or choosing from the card's menu, for keyboard users) changes that item's status.
export default function (thoughts) {
  const ar = document.documentElement.lang === 'ar';
  const COLUMNS = ar
    ? [{ status: 'seed', title: 'جديد' }, { status: 'active', title: 'قيد العمل' }, { status: 'parked', title: 'معلّق' }, { status: 'done', title: 'منجز' }]
    : [{ status: 'seed', title: 'New' }, { status: 'active', title: 'In progress' }, { status: 'parked', title: 'On hold' }, { status: 'done', title: 'Done' }];
  const L = ar
    ? { page: 'اللوحة', moveFailed: 'تعذّر نقل هذا العنصر.', statusOf: (t) => `حالة ${t}` }
    : { page: 'Board', moveFailed: 'Could not move that item.', statusOf: (t) => `Status of ${t}` };

  thoughts.ui.addView({
    id: 'status-board',
    title: L.page,
    async render(el) {
      const board = document.createElement('div');
      board.style.cssText = `display:grid;grid-template-columns:repeat(${COLUMNS.length},minmax(180px,1fr));gap:12px;padding:16px;align-items:start;overflow-x:auto`;
      el.replaceChildren(board);

      const move = async (path, status) => {
        try {
          await thoughts.ideas.setStatus(path, status);
        } catch {
          thoughts.ui.toast(L.moveFailed);
        }
        await draw();
      };

      const card = (idea) => {
        const box = document.createElement('div');
        box.draggable = true;
        box.style.cssText = 'display:grid;gap:6px;padding:8px;background:var(--panel);border:var(--border-w,1px) solid var(--border);border-radius:var(--radius);cursor:grab';
        box.addEventListener('dragstart', (e) => {
          e.dataTransfer.setData('text/plain', idea.path);
          e.dataTransfer.effectAllowed = 'move';
        });
        const title = document.createElement('div');
        title.dir = 'auto';
        title.textContent = idea.title;
        // The same move without a mouse.
        const pick = document.createElement('select');
        pick.setAttribute('aria-label', L.statusOf(idea.title));
        pick.style.cssText = 'font:inherit;color:var(--fg);background:var(--panel-2);border:var(--border-w,1px) solid var(--border);border-radius:var(--radius)';
        for (const c of COLUMNS) {
          const o = document.createElement('option');
          o.value = c.status;
          o.textContent = c.title;
          o.selected = c.status === idea.status;
          pick.append(o);
        }
        pick.addEventListener('change', () => move(idea.path, pick.value));
        box.append(title, pick);
        return box;
      };

      async function draw() {
        let ideas = [];
        try { ideas = await thoughts.ideas.list(); } catch { /* show empty columns */ }
        board.replaceChildren();
        for (const c of COLUMNS) {
          const col = document.createElement('section');
          col.style.cssText = 'display:grid;gap:8px;align-content:start;min-block-size:120px;padding:8px;background:var(--panel-2);border-radius:var(--radius-lg, var(--radius))';
          const h = document.createElement('h3');
          const mine = ideas.filter((i) => i.status === c.status);
          h.textContent = `${c.title} (${mine.length.toLocaleString(document.documentElement.lang || undefined)})`;
          h.style.cssText = 'margin:0;font-size:0.9em;color:var(--muted)';
          col.append(h, ...mine.map(card));
          col.addEventListener('dragover', (e) => { e.preventDefault(); e.dataTransfer.dropEffect = 'move'; });
          col.addEventListener('drop', (e) => {
            e.preventDefault();
            const path = e.dataTransfer.getData('text/plain');
            const idea = ideas.find((i) => i.path === path);
            if (idea && idea.status !== c.status) move(path, c.status);
          });
          board.append(col);
        }
      }

      await draw();
      thoughts.on('library.changed', draw);
    },
  });
}
