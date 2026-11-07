// Load into the dashboard page: counts putImageData calls and pixels on 2D
// canvases (wterm's image canvases) for SECONDS, after WARMUP seconds.
// Result in window.__paintResult. Pixel updates drawn in a worker are not
// seen here: this is for panes that draw Kitty images with wterm.
(() => {
  const WARMUP = 8;
  const SECONDS = 10;
  window.__paintResult = null;
  const proto = CanvasRenderingContext2D.prototype;
  const original = proto.putImageData;
  let calls = 0;
  let pixels = 0;
  let counting = false;
  proto.putImageData = function (image, ...rest) {
    if (counting) {
      calls++;
      pixels += image.width * image.height;
    }
    return original.call(this, image, ...rest);
  };
  setTimeout(() => {
    counting = true;
    const frames = globalThis.__westermStats?.frames ?? 0;
    setTimeout(() => {
      counting = false;
      proto.putImageData = original;
      const appFrames = (globalThis.__westermStats?.frames ?? 0) - frames;
      window.__paintResult = {
        putImageDataPerSec: calls / SECONDS,
        megapixelsPerSec: Number((pixels / SECONDS / 1e6).toFixed(2)),
        megapixelsPerAppFrame: appFrames ? Number((pixels / appFrames / 1e6).toFixed(3)) : null,
        appFrames,
      };
    }, SECONDS * 1000);
  }, WARMUP * 1000);
  return "counting";
})();
