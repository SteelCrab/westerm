import { useEffect, useState } from "react";

export type CopyResult = "copied" | "failed" | null;

/** Copies text to the clipboard. The result shows for a moment, then clears. */
export function useCopy(): [CopyResult, (text: string) => void] {
  const [result, setResult] = useState<CopyResult>(null);
  useEffect(() => {
    if (!result) return;
    const timer = setTimeout(() => setResult(null), 1500);
    return () => clearTimeout(timer);
  }, [result]);
  const copy = (text: string) => {
    // `navigator.clipboard` is missing outside secure contexts, and writeText can throw.
    try {
      void navigator.clipboard.writeText(text).then(
        () => setResult("copied"),
        () => setResult("failed"),
      );
    } catch {
      setResult("failed");
    }
  };
  return [result, copy];
}
