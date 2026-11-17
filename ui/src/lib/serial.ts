// Two small orderings for requests that can overlap (store.svelte.ts).

/** Results of overlapping refreshes, applied newest-first only: an answer to a request started
 *  before the one already shown is dropped, so a slow old answer never puts back what a newer one
 *  replaced. `guard(fetch, apply)` resolves whether `apply` ran. */
export function newestOnly() {
  let started = 0;
  let shown = 0;
  return async function guard<T>(fetch: () => Promise<T>, apply: (v: T) => void): Promise<boolean> {
    const mine = ++started;
    const v = await fetch();
    if (mine < shown) return false;
    shown = mine;
    apply(v);
    return true;
  };
}

/** One job at a time, in the order they were asked for; a failed job does not stop the ones after
 *  it. Each job runs only when its turn comes, so it reads the state the job before it left. */
export function oneAtATime() {
  let tail: Promise<unknown> = Promise.resolve();
  return function run<T>(job: () => Promise<T>): Promise<T> {
    const p = tail.then(job, job);
    tail = p.catch(() => {});
    return p;
  };
}
