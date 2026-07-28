import { useEffect, useReducer } from "react";

/**
 * Re-render this component whenever a module store notifies.
 *
 * This is `useSyncExternalStore`'s job, and that is what the terminal store's
 * consumers use. Where a store write lands mid-render — from a promise
 * continuation as a modal closes, say — the snapshot comparison can leave a
 * component showing its previous state until something else re-renders it.
 *
 * A plain effect subscription with a forced re-render has no snapshot
 * comparison to get wrong. It gives up the tear-free guarantee, which costs
 * nothing here: these stores are read in one place.
 *
 * The mount-time bump covers the window between rendering and the effect
 * attaching, so a store that changed in between is not missed.
 */
export function useStoreSubscription(
  subscribe: (listener: () => void) => () => void,
): void {
  const [, force] = useReducer((count: number) => count + 1, 0);

  useEffect(() => {
    force();
    return subscribe(force);
  }, [subscribe]);
}
