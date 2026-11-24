// What "Automatic" means where a model is chosen, said under it in the list (ModelPicker.svelte).
import { t } from '../i18n';
import { nameOf, saysDefault, type ModelEntry } from './picker';

/** `ownDefault` is the default chosen in Settings for this assistant, where the choice at hand
 *  falls back to it (a chat's, a plan's); without one, the model the program itself starts with
 *  when it said which. */
export function autoNote(entries: ModelEntry[], ownDefault?: string | null): string {
  if (ownDefault) return t('models72.autoNote.settings', { model: nameOf(entries, ownDefault) });
  const d = entries.find((e) => e.isDefault);
  return d && !saysDefault(d.name) ? t('models72.autoNote.named', { model: nameOf(entries, d.id) }) : t('models72.autoNote');
}
