// Overdue todos on Home: a ready-made pack that comes with Thoughts.
export default function (thoughts) {
  const ar = document.documentElement.lang === 'ar';
  const L = ar
    ? { title: 'المهام المتأخرة', none: 'لا توجد مهام متأخرة.', failed: 'تعذّرت قراءة مهامك الآن.' }
    : { title: 'Overdue', none: 'Nothing overdue.', failed: 'Your todos could not be read just now.' };
  const shortDate = (iso) => {
    try { return new Date(`${iso}T12:00:00`).toLocaleDateString(document.documentElement.lang || undefined, { day: 'numeric', month: 'short' }); }
    catch { return iso; }
  };

  thoughts.ui.addHomeWidget({
    id: 'overdue-todos',
    title: L.title,
    async render(el) {
      const note = (text) => {
        const p = document.createElement('p');
        p.textContent = text;
        p.style.cssText = 'margin:0;color:var(--muted)';
        el.replaceChildren(p);
      };
      const draw = async () => {
        const today = new Date().toLocaleDateString('en-CA'); // YYYY-MM-DD, local
        let items;
        try {
          const files = await thoughts.todos.list();
          items = files.flatMap((f) => f.items).filter((i) => !i.done && i.due && i.due < today);
        } catch {
          note(L.failed);
          return;
        }
        items.sort((a, b) => a.due.localeCompare(b.due));
        if (!items.length) { note(L.none); return; }
        el.replaceChildren();
        for (const i of items.slice(0, 7)) {
          const row = document.createElement('button');
          row.dir = 'auto';
          row.textContent = `${i.title} — ${shortDate(i.due)}`;
          row.style.cssText = 'display:block;width:100%;text-align:start;font:inherit;color:inherit;background:none;border:0;padding:4px 0;cursor:pointer';
          row.onclick = () => thoughts.ui.go('todos');
          el.append(row);
        }
      };
      await draw();
      thoughts.on('todos.changed', draw);
    },
  });
}
