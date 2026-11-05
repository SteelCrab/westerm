import { useEffect, useState } from "react";
import { SSH_HOSTS_PATH } from "../shared/protocol.ts";
import { serverPath } from "./remoteWindow.ts";

export interface SshHost {
  alias: string;
  hostname: string | null;
  user: string | null;
}

/**
 * Hosts from ~/.ssh/config (the server's, in a remote window), read again
 * each time the picker opens. Null while loading.
 */
export function useSshHosts(active: boolean): SshHost[] | null {
  const [hosts, setHosts] = useState<SshHost[] | null>(null);
  useEffect(() => {
    if (!active) return;
    let cancelled = false;
    setHosts(null);
    fetch(serverPath(SSH_HOSTS_PATH), { cache: "no-store" })
      .then((response) => (response.ok ? (response.json() as Promise<SshHost[]>) : []))
      .catch(() => [])
      .then((list) => {
        if (!cancelled) setHosts(Array.isArray(list) ? list : []);
      });
    return () => {
      cancelled = true;
    };
  }, [active]);
  return hosts;
}
