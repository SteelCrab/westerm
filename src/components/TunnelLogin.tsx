import { useEffect, useState, type FormEvent } from "react";
import { TUNNEL_LOGIN_PATH } from "../../shared/protocol.ts";
import { useI18n } from "../i18n/index.tsx";
import { codeDigits } from "../tunnel.ts";

interface TunnelLoginProps {
  /** Seconds until logins open again, from the server. */
  lockedFor: number | null;
  onSignedIn: () => void;
}

/**
 * The first screen of a tunnel page: the access code from the computer's
 * tunnel panel. It names neither the computer nor the user: before the code,
 * the page tells nothing.
 */
export function TunnelLogin({ lockedFor, onSignedIn }: TunnelLoginProps) {
  const { t } = useI18n();
  const [code, setCode] = useState("");
  const [wrong, setWrong] = useState(0);
  const [locked, setLocked] = useState(lockedFor ?? 0);
  const [sending, setSending] = useState(false);
  useEffect(() => {
    if (locked <= 0) return;
    const timer = setTimeout(() => setLocked((left) => left - 1), 1000);
    return () => clearTimeout(timer);
  }, [locked]);

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (code.length !== 6 || locked > 0 || sending) return;
    setSending(true);
    try {
      const response = await fetch(TUNNEL_LOGIN_PATH, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ code }),
      });
      if (response.status === 204) {
        onSignedIn();
        return;
      }
      const body = (await response.json().catch(() => null)) as { lockedFor?: unknown } | null;
      const seconds = typeof body?.lockedFor === "number" ? body.lockedFor : 0;
      setCode("");
      if (seconds > 0) setLocked(seconds);
      else setWrong((count) => count + 1);
    } catch {
      setWrong((count) => count + 1);
    } finally {
      setSending(false);
    }
  };

  return (
    <main className="tunnel-login">
      <form className="tunnel-login-card" onSubmit={(event) => void submit(event)}>
        <img className="tunnel-login-logo" src="/favicon.png" alt="" width={40} height={40} />
        <h1>{t.tunnelPage.connectTo}</h1>
        <p className="tunnel-login-hint">{t.tunnelPage.codeHint}</p>
        <input
          // A new key replays the shake on every wrong code.
          key={wrong}
          className={wrong > 0 ? "tunnel-login-code shake" : "tunnel-login-code"}
          aria-label={t.tunnelPage.codeLabel}
          inputMode="numeric"
          autoComplete="one-time-code"
          pattern="[0-9]*"
          // No maxLength: a browser cuts a paste to it before onChange, and
          // "482 913" (as the panel shows it) would lose its last digit.
          autoFocus
          disabled={locked > 0}
          value={code}
          onChange={(event) => setCode(codeDigits(event.target.value))}
        />
        <p className="tunnel-login-error" role="alert">
          {locked > 0 ? t.tunnelPage.lockedFor(locked) : wrong > 0 ? t.tunnelPage.wrongCode : ""}
        </p>
        <button type="submit" className="button primary" disabled={code.length !== 6 || locked > 0 || sending}>
          {t.tunnelPage.connect}
        </button>
      </form>
    </main>
  );
}

/** The end of a tunnel page. An ended tunnel does not come back: its address is gone. */
export function TunnelEndScreen({ kind, onRetry }: { kind: "ended" | "unreachable"; onRetry: () => void }) {
  const { t } = useI18n();
  return (
    <main className="tunnel-login">
      <div className="tunnel-login-card" role="status">
        <img className="tunnel-login-logo" src="/favicon.png" alt="" width={40} height={40} />
        <h1>{kind === "ended" ? t.tunnelPage.ended : t.tunnelPage.unreachable}</h1>
        <p className="tunnel-login-hint">{kind === "ended" ? t.tunnelPage.endedHint : t.tunnelPage.unreachableHint}</p>
        {kind === "unreachable" && (
          <button type="button" className="button primary" onClick={onRetry}>
            {t.tunnelPage.retry}
          </button>
        )}
      </div>
    </main>
  );
}
