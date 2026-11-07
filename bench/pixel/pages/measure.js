// Load into the dashboard page as a script (see ../README.md). Waits WARMUP
// seconds, measures for SECONDS, and stores the result in
// window.__benchResult. Reads globalThis.__westermStats
// (src/terminal/pane-stats.ts).
(() => {
  const WARMUP = 8;
  const SECONDS = 10;
  const stats = globalThis.__westermStats;
  if (!stats) return "no __westermStats on this page";
  window.__benchResult = null;

  const measure = () => {
    const start = { ...stats };
    const t0 = performance.now();
    const gaps = [];
    let last = t0;
    let shown = 0;
    let lastFrames = stats.frames;

    // Main-thread tasks of 50 ms or more.
    let longMs = 0;
    let longTasks = 0;
    const observer = new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        longMs += entry.duration;
        longTasks++;
      }
    });
    try {
      observer.observe({ type: "longtask" });
    } catch {}

    // Event loop lag: how late a 10 ms timer runs.
    let lagSum = 0;
    let lagMax = 0;
    let lagCount = 0;
    let expected = performance.now() + 10;
    const lagTimer = setInterval(() => {
      const now = performance.now();
      const lag = Math.max(0, now - expected);
      lagSum += lag;
      lagMax = Math.max(lagMax, lag);
      lagCount++;
      expected = now + 10;
    }, 10);

    const tick = (now) => {
      gaps.push(now - last);
      last = now;
      // A display frame that shows a new picture of the app.
      if (stats.frames !== lastFrames) {
        shown++;
        lastFrames = stats.frames;
      }
      if (now - t0 < SECONDS * 1000) {
        requestAnimationFrame(tick);
        return;
      }
      clearInterval(lagTimer);
      observer.disconnect();
      const secs = (now - t0) / 1000;
      gaps.sort((a, b) => a - b);
      const round = (value, digits = 1) => Number(value.toFixed(digits));
      window.__benchResult = {
        seconds: round(secs, 2),
        appFramesPerSec: round((stats.frames - start.frames) / secs),
        shownFramesPerSec: round(shown / secs),
        textMBPerSec: round((stats.textBytes - start.textBytes) / secs / 1e6, 2),
        pixelMBPerSec: round((stats.pixelBytes - start.pixelBytes) / secs / 1e6, 2),
        pageFps: round(gaps.length / secs),
        p95FrameMs: round(gaps[Math.floor(gaps.length * 0.95)]),
        maxFrameMs: round(gaps[gaps.length - 1]),
        longTaskMsPerSec: round(longMs / secs),
        longTasks,
        timerLagAvgMs: round(lagSum / Math.max(1, lagCount), 2),
        timerLagMaxMs: round(lagMax),
      };
    };
    requestAnimationFrame(tick);
  };

  setTimeout(measure, WARMUP * 1000);
  return "warming up";
})();
