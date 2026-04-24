import { useEffect, useState } from "react";

import { toWebSocketUrl } from "../lib/api/client";
import type {
  ProjectRecord,
  ProjectStatusSocketMessage,
} from "../lib/api/types";

export type SocketState = "idle" | "connecting" | "open" | "closed" | "error";

export function useProjectStatusSocket(
  baseUrl: string,
  project: ProjectRecord | null,
) {
  const [socketState, setSocketState] = useState<SocketState>("idle");
  const [message, setMessage] = useState<ProjectStatusSocketMessage | null>(null);

  useEffect(() => {
    if (!project) {
      setSocketState("idle");
      setMessage(null);
      return;
    }

    const socketUrl = new URL("/ws/projects", `${toWebSocketUrl(baseUrl)}/`);
    socketUrl.searchParams.set("project_id", project.id);
    socketUrl.searchParams.set("interval_ms", "1200");

    setSocketState("connecting");
    const socket = new WebSocket(socketUrl.toString());

    socket.addEventListener("open", () => {
      setSocketState("open");
    });

    socket.addEventListener("message", (event) => {
      try {
        const payload = JSON.parse(event.data) as ProjectStatusSocketMessage;
        setMessage(payload);
      } catch {
        setSocketState("error");
      }
    });

    socket.addEventListener("close", () => {
      setSocketState("closed");
    });

    socket.addEventListener("error", () => {
      setSocketState("error");
    });

    return () => {
      socket.close();
    };
  }, [baseUrl, project?.id]);

  return {
    socketState,
    message,
  };
}
