// Undo and redo for the tuner: a list of whole settings, with a place in it.
//
// Dragging a colour picker or a slider changes a value many times a second; those changes are one
// step to undo. A change that carries the same `key` as the one before it, soon after it, replaces
// it instead of adding a step.

export class History<T> {
  private items: T[];
  private at = 0;
  private lastKey: string | null = null;
  private lastTime = 0;

  constructor(first: T, private readonly max = 200, private readonly joinMs = 900, private readonly now: () => number = () => Date.now()) {
    this.items = [first];
  }

  get current(): T { return this.items[this.at]; }
  get canUndo(): boolean { return this.at > 0; }
  get canRedo(): boolean { return this.at < this.items.length - 1; }
  get length(): number { return this.items.length; }

  /** A new state. Anything that could be redone is gone. */
  push(value: T, key: string | null = null): T {
    const t = this.now();
    const join = key !== null && key === this.lastKey && t - this.lastTime <= this.joinMs && this.at > 0 && this.at === this.items.length - 1;
    if (join) this.items[this.at] = value;
    else {
      this.items.length = this.at + 1;
      this.items.push(value);
      if (this.items.length > this.max) this.items.splice(0, this.items.length - this.max);
      this.at = this.items.length - 1;
    }
    this.lastKey = key;
    this.lastTime = t;
    return value;
  }

  undo(): T { if (this.canUndo) this.at--; this.lastKey = null; return this.current; }
  redo(): T { if (this.canRedo) this.at++; this.lastKey = null; return this.current; }

  /** Start again from `value` (another style was picked, or the settings changed elsewhere). */
  reset(value: T): T { this.items = [value]; this.at = 0; this.lastKey = null; return value; }
}
