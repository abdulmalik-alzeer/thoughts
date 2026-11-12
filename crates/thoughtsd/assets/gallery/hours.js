// Billable hours with this month's total on Home: a ready-made pack that comes with Thoughts.
// Entries are kept in the add-on's own saved data, on this computer.
export default function (thoughts) {
  const ar = document.documentElement.lang === 'ar';
  const L = ar
    ? {
      widget: 'الساعات المحتسبة هذا الشهر', page: 'الساعات', unit: 'س', openPage: 'افتح صفحة الساعات',
      date: 'التاريخ', client: 'العميل', hours: 'عدد الساعات', add: 'إضافة', month: 'هذا الشهر',
      empty: 'لا توجد ساعات بعد. أضف أول إدخال في الأعلى.', remove: 'حذف',
      removeLabel: (e) => `حذف ${e.hours} س للعميل ${e.client} بتاريخ ${e.date}`,
      saveFailed: 'تعذّر الحفظ. إدخالاتك السابقة لم تتغيّر.', readFailed: 'تعذّرت قراءة إدخالاتك الآن.',
    }
    : {
      widget: 'Billable hours this month', page: 'Hours', unit: 'h', openPage: 'open the hours page',
      date: 'Date', client: 'Client', hours: 'Hours', add: 'Add', month: 'This month',
      empty: 'No hours yet. Add your first entry above.', remove: 'Remove',
      removeLabel: (e) => `Remove ${e.hours} h for ${e.client} on ${e.date}`,
      saveFailed: 'Could not save. Your earlier entries are unchanged.', readFailed: 'Your entries could not be read just now.',
    };
  const today = () => new Date().toLocaleDateString('en-CA'); // YYYY-MM-DD, local
  const load = async () => {
    const data = await thoughts.storage.get();
    return Array.isArray(data.entries) ? data.entries.filter((e) => e && typeof e.date === 'string' && typeof e.hours === 'number') : [];
  };
  const monthTotal = (entries) => {
    const month = today().slice(0, 7);
    return entries.filter((e) => e.date.startsWith(month)).reduce((sum, e) => sum + e.hours, 0);
  };
  const fmt = (n) => (Math.round(n * 100) / 100).toLocaleString(document.documentElement.lang || undefined);

  thoughts.ui.addHomeWidget({
    id: 'hours-month',
    title: L.widget,
    async render(el) {
      const total = document.createElement('button');
      total.style.cssText = 'font:inherit;font-size:1.6em;color:var(--fg);background:none;border:0;padding:0;cursor:pointer';
      let entries = [];
      try { entries = await load(); } catch { /* shown as 0 */ }
      total.textContent = `${fmt(monthTotal(entries))} ${L.unit}`;
      total.setAttribute('aria-label', `${total.textContent} — ${L.openPage}`);
      total.onclick = () => thoughts.ui.go('hours');
      el.replaceChildren(total);
    },
  });

  thoughts.ui.addView({
    id: 'hours',
    title: L.page,
    async render(el) {
      let entries = [];
      let readFailed = false;
      try { entries = await load(); } catch { readFailed = true; }
      const wrap = document.createElement('div');
      wrap.style.cssText = 'display:grid;gap:16px;max-inline-size:640px;padding:16px';
      const field = 'font:inherit;color:var(--fg);background:var(--panel);border:var(--border-w,1px) solid var(--border);border-radius:var(--radius);padding:8px';

      const form = document.createElement('form');
      form.style.cssText = 'display:flex;flex-wrap:wrap;gap:8px;align-items:end';
      const date = Object.assign(document.createElement('input'), { type: 'date', value: today(), required: true });
      const client = Object.assign(document.createElement('input'), { type: 'text', placeholder: L.client, required: true, dir: 'auto' });
      const hours = Object.assign(document.createElement('input'), { type: 'number', min: '0.25', max: '24', step: '0.25', placeholder: L.hours, required: true });
      date.setAttribute('aria-label', L.date);
      client.setAttribute('aria-label', L.client);
      hours.setAttribute('aria-label', L.hours);
      for (const i of [date, client, hours]) i.style.cssText = field;
      const add = document.createElement('button');
      add.type = 'submit';
      add.textContent = L.add;
      add.style.cssText = 'font:inherit;padding:8px 16px;border:0;border-radius:var(--radius);background:var(--accent);color:var(--accent-fg);cursor:pointer';
      form.append(date, client, hours, add);

      const total = document.createElement('p');
      total.style.cssText = 'margin:0;color:var(--muted)';
      const list = document.createElement('div');
      list.style.cssText = 'display:grid;gap:4px';

      const save = async (next) => {
        try {
          await thoughts.storage.set({ entries: next });
          entries = next;
        } catch {
          thoughts.ui.toast(L.saveFailed);
        }
        draw();
      };

      function draw() {
        total.textContent = `${L.month}: ${fmt(monthTotal(entries))} ${L.unit}`;
        list.replaceChildren();
        const sorted = [...entries].sort((a, b) => b.date.localeCompare(a.date));
        if (!sorted.length) {
          const empty = document.createElement('p');
          empty.textContent = readFailed ? L.readFailed : L.empty;
          empty.style.color = 'var(--muted)';
          list.append(empty);
        }
        for (const e of sorted.slice(0, 200)) {
          const row = document.createElement('div');
          row.style.cssText = 'display:flex;gap:12px;align-items:center;padding:6px 0;border-block-end:var(--border-w,1px) solid var(--border)';
          const text = document.createElement('span');
          text.dir = 'auto';
          text.style.flex = '1';
          text.textContent = `${e.date} · ${e.client} · ${fmt(e.hours)} ${L.unit}`;
          const del = document.createElement('button');
          del.textContent = L.remove;
          del.setAttribute('aria-label', L.removeLabel({ ...e, hours: fmt(e.hours) }));
          del.style.cssText = 'font:inherit;color:var(--danger);background:none;border:0;cursor:pointer';
          del.onclick = () => save(entries.filter((x) => x.id !== e.id));
          row.append(text, del);
          list.append(row);
        }
      }

      form.addEventListener('submit', (ev) => {
        ev.preventDefault();
        const h = Number(hours.value);
        if (!client.value.trim() || !(h > 0)) return;
        const entry = { id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`, date: date.value || today(), client: client.value.trim(), hours: h };
        client.value = '';
        hours.value = '';
        readFailed = false;
        save([...entries, entry]);
      });

      wrap.append(form, total, list);
      el.replaceChildren(wrap);
      draw();
    },
  });
}
