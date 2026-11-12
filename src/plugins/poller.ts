// Runs `tick` on a timer. The next wait starts after a tick ends, so ticks
// never overlap. While the page is hidden nothing is scheduled; `poke()` runs
// one tick at once (page visible, panel opened, action done).

export interface PollerOptions {
  tick: () => Promise<void>;
  /** The wait before the next tick, in ms; read after each tick. */
  interval: () => number;
  hidden: () => boolean;
}

export interface Poller {
  start(): void;
  /** Runs one tick now. During a tick, one more runs right after it. */
  poke(): void;
  stop(): void;
}

export function createPoller({ tick, interval, hidden }: PollerOptions): Poller {
  let timer: ReturnType<typeof setTimeout> | null = null;
  let running = false;
  let again = false;
  let stopped = false;

  function clear() {
    if (timer !== null) clearTimeout(timer);
    timer = null;
  }

  async function run() {
    clear();
    running = true;
    try {
      await tick();
    } catch {
      // A failed tick must not end the polling.
    } finally {
      running = false;
    }
    if (stopped) return;
    if (again) {
      again = false;
      void run();
    } else if (!hidden()) {
      timer = setTimeout(onTimer, interval());
    }
  }

  function onTimer() {
    timer = null;
    // A hidden page asks nothing; the next poke restarts the cycle.
    if (!stopped && !hidden()) void run();
  }

  return {
    start() {
      if (stopped || running || timer !== null || hidden()) return;
      void run();
    },
    poke() {
      if (stopped) return;
      if (running) again = true;
      else void run();
    },
    stop() {
      stopped = true;
      again = false;
      clear();
    },
  };
}
