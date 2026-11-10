import { useEffect, useState } from "react";
import { Cloud, Copy, TriangleAlert } from "lucide-react";
import { renderSVG } from "uqr";
import type { TunnelDevice, TunnelStatus } from "../../shared/protocol.ts";
import { useI18n } from "../i18n/index.tsx";
import { elapsedMinutes, hoursLeft, installHint } from "../tunnel.ts";
import { useCopy } from "../useCopy.ts";

interface TunnelPanelProps {
  status: TunnelStatus | null;
  /** For tests; the panel keeps its own clock otherwise. */
  now?: number;
  onStart: () => void;
  onStop: () => void;
  onCheck: () => void;
  /** The sidebar is too narrow for text: only the state shows. */
  compact?: boolean;
}

/** The panel's clock: the elapsed time and the auto-stop hours change slowly. A new status reads it again. */
function useClock(fixed: number | undefined, status: TunnelStatus | null): number {
  const [now, setNow] = useState(() => fixed ?? Date.now());
  useEffect(() => {
    if (fixed !== undefined) return;
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 30_000);
    return () => clearInterval(timer);
  }, [fixed, status]);
  return fixed ?? now;
}

/** The sidebar slot's tunnel panel: start a tunnel, then show its address, QR code, and access code. */
export function TunnelPanel({ status, now: fixedNow, onStart, onStop, onCheck, compact = false }: TunnelPanelProps) {
  const { t } = useI18n();
  const now = useClock(fixedNow, status);
  const phase = status?.phase ?? null;
  if (compact) {
    return (
      <aside className="session-panel tunnel-panel compact" aria-label={t.tunnel.title}>
        <div className="tunnel-compact" title={t.tunnel.title}>
          <Cloud size={18} aria-hidden="true" />
          {(phase === "on" || phase === "reconnecting") && <span className={`tunnel-dot ${phase}`} aria-hidden="true" />}
        </div>
      </aside>
    );
  }
  return (
    <aside className="session-panel tunnel-panel" aria-label={t.tunnel.title}>
      <header className="agents-header">
        <span>{t.tunnel.title}</span>
      </header>
      <div className="tunnel-body">{status ? <Body status={status} now={now} onStart={onStart} onStop={onStop} onCheck={onCheck} /> : <p className="tunnel-muted">…</p>}</div>
    </aside>
  );
}

function Body({ status, now, onStart, onStop, onCheck }: Required<Omit<TunnelPanelProps, "compact">> & { status: TunnelStatus }) {
  const { t } = useI18n();
  const reason = status.reason && status.reason !== "stopped" ? t.tunnel.reasons[status.reason] : null;
  switch (status.phase) {
    case "off":
      return (
        <>
          <p className="tunnel-intro">{t.tunnel.intro}</p>
          {reason && <p className="tunnel-reason">{reason}</p>}
          <button type="button" className="button primary tunnel-action" onClick={onStart}>
            {t.tunnel.start}
          </button>
        </>
      );
    case "starting":
      return (
        <>
          <p className="tunnel-state">
            <span className="tunnel-spinner" aria-hidden="true" />
            {t.tunnel.starting}
          </p>
          <button type="button" className="button tunnel-action" onClick={onStop}>
            {t.tunnel.cancel}
          </button>
        </>
      );
    case "missing": {
      const hint = installHint(status.os);
      return (
        <>
          <p className="tunnel-state">{t.tunnel.missing}</p>
          <p className="tunnel-muted">{hint.command ? t.tunnel.missingHint : t.tunnel.missingLinux}</p>
          {hint.command && <CopyField text={hint.command} mono />}
          <a className="tunnel-link" href={hint.link} target="_blank" rel="noreferrer">
            {t.tunnel.downloads}
          </a>
          <button type="button" className="button tunnel-action" onClick={onCheck}>
            {t.tunnel.checkAgain}
          </button>
        </>
      );
    }
    case "failed":
      return (
        <>
          <p className="tunnel-state failed">{t.tunnel.failed}</p>
          {reason && <p className="tunnel-reason">{reason}</p>}
          {status.log.length > 0 && (
            <details className="tunnel-log">
              <summary>{t.tunnel.log}</summary>
              <pre>{status.log.join("\n")}</pre>
            </details>
          )}
          <button type="button" className="button primary tunnel-action" onClick={onStart}>
            {t.tunnel.restart}
          </button>
        </>
      );
    case "on":
    case "reconnecting":
      return <Live status={status} now={now} onStop={onStop} />;
  }
}

function Live({ status, now: clock, onStop }: { status: TunnelStatus; now: number; onStop: () => void }) {
  const { t } = useI18n();
  // A clock read before the tunnel started would add an hour to the auto-stop time.
  const now = Math.max(clock, status.startedAt ?? clock);
  const reconnecting = status.phase === "reconnecting";
  const url = status.url ?? "";
  const code = status.code ?? "";
  const connected = status.devices.some((device) => device.connected);
  return (
    <>
      <p className={`tunnel-state ${status.phase}`}>
        <span className={`tunnel-dot ${status.phase}`} aria-hidden="true" />
        {reconnecting ? t.tunnel.reconnecting : t.tunnel.connectedFor(elapsedMinutes(status.startedAt ?? now, now))}
      </p>
      {reconnecting && <p className="tunnel-muted">{t.tunnel.reconnectingHint}</p>}
      <div className={reconnecting ? "tunnel-live dim" : "tunnel-live"}>
        <span className="tunnel-label">{t.tunnel.address}</span>
        <CopyField text={url} />
        {url && (
          // The QR code holds the address only; the code is typed by hand as a second check.
          <div className="tunnel-qr" role="img" aria-label={t.tunnel.qr} dangerouslySetInnerHTML={{ __html: renderSVG(url, { border: 1 }) }} />
        )}
        <span className="tunnel-label">{t.tunnel.code}</span>
        <p className="tunnel-code">{`${code.slice(0, 3)} ${code.slice(3)}`}</p>
      </div>
      <span className="tunnel-label">{t.tunnel.devices}</span>
      {status.devices.length === 0 ? (
        <p className="tunnel-muted">{t.tunnel.noDevices}</p>
      ) : (
        <ul className="tunnel-devices">
          {status.devices.map((device, index) => (
            <Device key={`${device.label}-${index}`} device={device} now={now} />
          ))}
        </ul>
      )}
      {!connected && status.idleDeadline !== null && (
        <p className="tunnel-muted">{t.tunnel.autoStopIn(hoursLeft(status.idleDeadline, now))}</p>
      )}
      <p className="tunnel-muted">{status.keepAwake ? t.tunnel.keepAwake : t.tunnel.noKeepAwake}</p>
      <p className="tunnel-warning">
        <TriangleAlert size={13} aria-hidden="true" />
        {t.tunnel.warning}
      </p>
      <button type="button" className="button tunnel-action" onClick={onStop}>
        {t.tunnel.stop}
      </button>
    </>
  );
}

function Device({ device, now }: { device: TunnelDevice; now: number }) {
  const { t } = useI18n();
  return (
    <li className="tunnel-device">
      <span className="tree-label">{device.label}</span>
      <span className={device.connected ? "tunnel-device-state online" : "tunnel-device-state"}>
        {device.connected ? t.tunnel.online : t.tunnel.lastSeen(elapsedMinutes(device.lastSeen, now))}
      </span>
    </li>
  );
}

function CopyField({ text, mono = false }: { text: string; mono?: boolean }) {
  const { t } = useI18n();
  const [result, copy] = useCopy();
  const label = result === "copied" ? t.tunnel.copied : result === "failed" ? t.tunnel.copyFailed : t.tunnel.copy;
  return (
    <div className="tunnel-copy">
      <code className={mono ? "tunnel-text mono" : "tunnel-text"}>{text}</code>
      <button type="button" className="icon-button" aria-label={label} title={label} onClick={() => copy(text)}>
        <Copy size={13} aria-hidden="true" />
      </button>
    </div>
  );
}
