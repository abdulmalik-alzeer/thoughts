// Plain sentences for why an Office/OpenDocument file can't be shown (safezip.ts OfficeProblem).
import { t } from '../../i18n';
import type { OfficeProblem } from './safezip';

export function problemText(p: OfficeProblem): string {
  return t(`libf-preview-office.problem.${p}`);
}
