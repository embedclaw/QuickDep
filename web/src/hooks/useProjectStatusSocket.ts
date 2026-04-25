import { useEffect, useState, useRef, useCallback } from "react";

import { toWebSocketUrl } from "../lib/api/client";
import type {
  ProjectRecord,
  ProjectStatusSocketMessage,
} from "../lib/api/types";

export type SocketState = "idle" | "connecting" | "open" | "closed" | "error" | "reconnecting";

export type ReconnectState = {
  attemptCount: number;
  maxAttempts: number;
  nextDelayMs: number;
};

export function useProjectStatusSocket(
  baseUrl: string,
  project: ProjectRecord | null,
) {
  const [socketState, setSocketState] = useState<SocketState>("idle");
  const [message, setMessage] = useState<ProjectStatusSocketMessage | null>(null);

  const reconnectCountRef = useRef(0);
  const socketRef = useRef<WebSocket | null>(null);
  const reconnectTimeoutRef = useRef<number | null>(null);

  const MAX_RECONNECT_ATTEMPTS = 5;
  const BASE_DELAY_MS = 1000;
  const MAX_DELAY_MS = 16000;

  const calculateDelay = useCallback((attempt: number): number => {
    const delay = BASE_DELAY_MS * Math.pow(2, attempt);
    return Math.min(delay, MAX_DELAY_MS);
  }, []);

  const clearReconnectTimeout = useCallback(() => {
    if (reconnectTimeoutRef.current !== null) {
      window.clearTimeout(reconnectTimeoutRef.current);
      reconnectTimeoutRef.current = null;
    }
  }, []);

  const cleanupSocket = useCallback(() => {
    if (socketRef.current) {
      socketRef.current.close();
      socketRef.current = null;
    }
    clearReconnectTimeout();
  }, [clearReconnectTimeout]);

  // Attempt reconnection with exponential backoff
  const attemptReconnect = useCallback(() => {
    if (reconnectCountRef.current >= MAX_RECONNECT_ATTEMPTS) {
      setSocketState("error");
      return;
    }

    const attempt = reconnectCountRef.current;
    const delay = calculateDelay(attempt);

    setSocketState("reconnecting");

    reconnectTimeoutRef.current = window.setTimeout(() => {
      reconnectCountRef.current += 1;
      connectSocket();
    }, delay);
  }, [calculateDelay, clearReconnectTimeout]);

  // Connect socket
  const connectSocket = useCallback(() => {
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
    socketRef.current = socket;

    socket.addEventListener("open", () => {
      setSocketState("open");
      reconnectCountRef.current = 0;
    });

    socket.addEventListener("message", (event) => {
      try {
        const payload = JSON.parse(event.data) as ProjectStatusSocketMessage;
        setMessage(payload);
      } catch {
        setSocketState("error");
      }
    });

    socket.addEventListener("close", (event) => {
      if (event.code !== 1000 && project) {
        attemptReconnect();
      } else {
        setSocketState("closed");
      }
    });

    socket.addEventListener("error", () => {
      if (reconnectCountRef.current < MAX_RECONNECT_ATTEMPTS) {
        setSocketState("error");
      }
    });
  }, [baseUrl, project, attemptReconnect]);

  useEffect(() => {
    if (!project) {
      setSocketState("idle");
      setMessage(null);
      cleanupSocket();
      reconnectCountRef.current = 0;
      return;
    }

    reconnectCountRef.current = 0;
    connectSocket();

    return () => {
      cleanupSocket();
      reconnectCountRef.current = 0;
    };
  }, [baseUrl, project?.id, cleanupSocket, connectSocket]);

  return {
    socketState,
    message,
  };
}
