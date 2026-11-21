// Looking at the built product's screens (docs/mado-spec.md, round 0.4.0). The desktop shell opens each
// address in a hidden window of its own that has no access to the app; in a browser there is no shell.
// A stand-in until the shell command exists: returns null ("nobody can look here").
import type { ScreenResult } from './types';

export async function screenCheck(_url: string, _screens: { id: string; path: string }[]): Promise<ScreenResult[] | null> {
  return null;
}
