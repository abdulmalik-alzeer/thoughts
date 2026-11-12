// A New client form that saves each client as a note in the clients folder: a ready-made pack
// that comes with Thoughts. It never overwrites an existing client.
export default function (thoughts) {
  const ar = document.documentElement.lang === 'ar';
  const L = ar
    ? {
      page: 'عميل جديد', name: 'اسم العميل', contact: 'الهاتف أو البريد الإلكتروني', need: 'ما يحتاج إليه', save: 'حفظ العميل',
      noName: 'اكتب اسم العميل من فضلك.', saved: (n) => `حُفظ العميل ${n}.`, failed: 'تعذّر حفظ العميل. لم يتغيّر شيء.',
      added: 'أُضيف في', reach: 'التواصل',
    }
    : {
      page: 'New client', name: 'Client name', contact: 'Phone or email', need: 'What they need', save: 'Save client',
      noName: 'Please enter the client name.', saved: (n) => `Saved ${n}.`, failed: 'Could not save the client. Nothing was changed.',
      added: 'Added', reach: 'Contact',
    };
  const FIELDS = [
    { key: 'name', label: L.name, required: true },
    { key: 'contact', label: L.contact },
    { key: 'matter', label: L.need, long: true },
  ];

  // A safe file name: letters and digits (any language) joined by dashes.
  const slug = (s) => s.trim().toLowerCase().replace(/[^\p{L}\p{N}]+/gu, '-').replace(/^-+|-+$/g, '').slice(0, 60);

  thoughts.ui.addView({
    id: 'client-intake',
    title: L.page,
    render(el) {
      const form = document.createElement('form');
      form.style.cssText = 'display:grid;gap:12px;max-inline-size:520px;padding:16px';
      const inputs = {};
      for (const f of FIELDS) {
        const label = document.createElement('label');
        label.style.cssText = 'display:grid;gap:4px;color:var(--fg)';
        label.append(f.label);
        const input = document.createElement(f.long ? 'textarea' : 'input');
        input.dir = 'auto';
        input.required = !!f.required;
        if (f.long) input.rows = 4;
        input.style.cssText = 'font:inherit;color:var(--fg);background:var(--panel);border:var(--border-w,1px) solid var(--border);border-radius:var(--radius);padding:8px';
        label.append(input);
        form.append(label);
        inputs[f.key] = input;
      }
      const save = document.createElement('button');
      save.type = 'submit';
      save.textContent = L.save;
      save.style.cssText = 'font:inherit;justify-self:start;padding:8px 16px;border:0;border-radius:var(--radius);background:var(--accent);color:var(--accent-fg);cursor:pointer';
      form.append(save);

      form.addEventListener('submit', async (e) => {
        e.preventDefault();
        const name = inputs.name.value.trim();
        const base = slug(name);
        if (!base) { thoughts.ui.toast(L.noName); return; }
        save.disabled = true;
        try {
          // Never overwrite an existing client: pick a free file name.
          const existing = new Set((await thoughts.library.list()).map((f) => (typeof f === 'string' ? f : f.path)));
          let path = `clients/${base}.md`;
          for (let n = 2; existing.has(path); n++) path = `clients/${base}-${n}.md`;
          const today = new Date().toLocaleDateString('en-CA'); // YYYY-MM-DD, local
          const text = [
            `# ${name}`,
            '',
            `- ${L.added}: ${today}`,
            `- ${L.reach}: ${inputs.contact.value.trim() || '—'}`,
            '',
            `## ${L.need}`,
            '',
            inputs.matter.value.trim() || '—',
            '',
          ].join('\n');
          await thoughts.library.write(path, text);
          form.reset();
          thoughts.ui.toast(L.saved(name));
        } catch {
          thoughts.ui.toast(L.failed);
        } finally {
          save.disabled = false;
        }
      });
      el.replaceChildren(form);
    },
  });
}
