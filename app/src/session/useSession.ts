import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";
import { api, toCommandError } from "../api";
import { useToasts } from "../components/Toasts";
import type { SessionMessage, SessionStatus } from "../types";

export const SESSION_EVENT = "session";

export interface SessionApi {
  status: SessionStatus | null;
  run: (action: () => Promise<SessionStatus>) => Promise<boolean>;
}

export function useSession(): SessionApi {
  const { notify, reportError } = useToasts();
  const [status, setStatus] = useState<SessionStatus | null>(null);

  useEffect(() => {
    let active = true;
    api
      .sessionStatus()
      .then((loaded) => active && setStatus(loaded))
      .catch(() => undefined);
    const subscription = listen<SessionMessage>(SESSION_EVENT, ({ payload }) => {
      setStatus(payload.status);
      if (payload.notice) {
        notify({ kind: payload.notice.tone, title: payload.notice.title, message: payload.notice.message });
      }
    });
    return () => {
      active = false;
      void subscription.then((unlisten) => unlisten());
    };
  }, [notify]);

  const run = useCallback(
    async (action: () => Promise<SessionStatus>) => {
      try {
        setStatus(await action());
        return true;
      } catch (error) {
        reportError(toCommandError(error));
        return false;
      }
    },
    [reportError],
  );

  return { status, run };
}
