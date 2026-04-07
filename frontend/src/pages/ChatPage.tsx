import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { Link, useParams } from "react-router-dom";
import { ArrowLeft, Bot, Loader2, Send } from "lucide-react";
import { cn } from "@/lib/utils";
import { api } from "@/lib/api";

// ── Types ──────────────────────────────────────────────────────
interface ChatMessage {
  id: string;
  role: "user" | "agent" | "system";
  content: string;
  timestamp: Date;
  isStreaming?: boolean;
}

interface PersistedMessage {
  id: number;
  role: string;
  content: string;
  created_at: string;
}

type ConnectionStatus = "connected" | "connecting" | "disconnected";

interface WsIncoming {
  type: "status" | "stream" | "done" | "error";
  content?: string;
  message?: string;
}

// ── Helpers ────────────────────────────────────────────────────
let _msgId = 0;
function nextId(): string {
  return `msg-${Date.now()}-${++_msgId}`;
}

function formatTime(d: Date): string {
  return d.toLocaleTimeString(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
}

function getWsUrl(agentId: string, token: string): string {
  const apiBase = import.meta.env.VITE_API_URL || "/api";
  // Derive WebSocket origin from current page if apiBase is relative
  let base: string;
  if (/^https?:\/\//.test(apiBase)) {
    base = apiBase.replace(/^http/, "ws");
  } else {
    const proto = window.location.protocol === "https:" ? "wss:" : "ws:";
    base = `${proto}//${window.location.host}${apiBase}`;
  }
  return `${base}/agents/${agentId}/chat?token=${encodeURIComponent(token)}`;
}

const STATUS_DOT: Record<ConnectionStatus, string> = {
  connected: "bg-emerald-500",
  connecting: "bg-amber-400 animate-pulse",
  disconnected: "bg-red-500",
};

const STATUS_LABEL: Record<ConnectionStatus, string> = {
  connected: "Connected",
  connecting: "Connecting…",
  disconnected: "Disconnected",
};

const MAX_RECONNECT_DELAY = 16_000;

// ── Component ──────────────────────────────────────────────────
export default function ChatPage() {
  const { id: agentId } = useParams<{ id: string }>();

  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [connectionStatus, setConnectionStatus] =
    useState<ConnectionStatus>("disconnected");
  const [isAgentReady, setIsAgentReady] = useState(false);
  const [isThinking, setIsThinking] = useState(false);
  const [input, setInput] = useState("");

  const wsRef = useRef<WebSocket | null>(null);
  const reconnectTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const reconnectDelay = useRef(1000);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const scrollContainerRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const userScrolledUp = useRef(false);
  // Ref to track the id of the current streaming message so we can append to it
  const streamingMsgId = useRef<string | null>(null);

  // ── Auto-scroll ──────────────────────────────────────────────
  const scrollToBottom = useCallback(() => {
    if (!userScrolledUp.current) {
      messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, []);

  useLayoutEffect(() => {
    scrollToBottom();
  }, [messages, scrollToBottom]);

  function handleScroll() {
    const el = scrollContainerRef.current;
    if (!el) return;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
    userScrolledUp.current = !atBottom;
  }

  // ── WebSocket ────────────────────────────────────────────────
  const addSystemMessage = useCallback((content: string) => {
    setMessages((prev) => [
      ...prev,
      { id: nextId(), role: "system", content, timestamp: new Date() },
    ]);
  }, []);

  const connect = useCallback(() => {
    if (!agentId) return;
    const token = localStorage.getItem("token");
    if (!token) return;

    // Clean up any existing connection
    wsRef.current?.close();
    clearTimeout(reconnectTimer.current);
    setConnectionStatus("connecting");

    const ws = new WebSocket(getWsUrl(agentId, token));
    wsRef.current = ws;

    ws.onopen = () => {
      setConnectionStatus("connected");
      reconnectDelay.current = 1000;
    };

    ws.onmessage = (event) => {
      let data: WsIncoming;
      try {
        data = JSON.parse(event.data) as WsIncoming;
      } catch {
        return;
      }

      switch (data.type) {
        case "status": {
          const msg = data.message ?? "";
          addSystemMessage(msg);
          if (/ready/i.test(msg)) {
            setIsAgentReady(true);
            setIsThinking(false);
          } else if (/thinking/i.test(msg)) {
            setIsThinking(true);
          } else if (/waking/i.test(msg)) {
            setIsAgentReady(false);
            setIsThinking(false);
          }
          break;
        }

        case "stream": {
          const token = data.content ?? "";
          setIsThinking(false);

          setMessages((prev) => {
            // If we have an active streaming message, append to it
            if (streamingMsgId.current) {
              return prev.map((m) =>
                m.id === streamingMsgId.current
                  ? { ...m, content: m.content + token }
                  : m,
              );
            }
            // Otherwise start a new agent message
            const id = nextId();
            streamingMsgId.current = id;
            return [
              ...prev,
              {
                id,
                role: "agent" as const,
                content: token,
                timestamp: new Date(),
                isStreaming: true,
              },
            ];
          });
          break;
        }

        case "done": {
          const fullContent = data.content ?? "";
          setIsThinking(false);

          setMessages((prev) => {
            if (streamingMsgId.current) {
              // Finalize the streaming message with the full content
              const updated = prev.map((m) =>
                m.id === streamingMsgId.current
                  ? { ...m, content: fullContent || m.content, isStreaming: false }
                  : m,
              );
              streamingMsgId.current = null;
              return updated;
            }
            // If no streaming msg was created, add the full message
            streamingMsgId.current = null;
            return [
              ...prev,
              {
                id: nextId(),
                role: "agent" as const,
                content: fullContent,
                timestamp: new Date(),
              },
            ];
          });
          break;
        }

        case "error": {
          streamingMsgId.current = null;
          setIsThinking(false);
          setMessages((prev) => [
            ...prev,
            {
              id: nextId(),
              role: "system",
              content: `⚠ ${data.message ?? "Unknown error"}`,
              timestamp: new Date(),
            },
          ]);
          break;
        }
      }
    };

    ws.onclose = () => {
      setConnectionStatus("disconnected");
      wsRef.current = null;

      // Reconnect with exponential backoff
      const delay = reconnectDelay.current;
      reconnectDelay.current = Math.min(delay * 2, MAX_RECONNECT_DELAY);
      reconnectTimer.current = setTimeout(connect, delay);
    };

    ws.onerror = () => {
      ws.close();
    };
  }, [agentId, addSystemMessage]);

  useEffect(() => {
    connect();
    return () => {
      clearTimeout(reconnectTimer.current);
      wsRef.current?.close();
      wsRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [agentId]);

  // Load persisted chat history on mount
  useEffect(() => {
    if (!agentId) return;
    api
      .get<PersistedMessage[]>(`/agents/${agentId}/messages`)
      .then((history) => {
        if (history.length > 0) {
          const loaded: ChatMessage[] = history.map((m) => ({
            id: `db-${m.id}`,
            role: m.role === "user" ? "user" : "agent",
            content: m.content,
            timestamp: new Date(m.created_at),
          }));
          setMessages((prev) => {
            // Only prepend if we haven't already loaded history
            if (prev.some((p) => p.id.startsWith("db-"))) return prev;
            return [...loaded, ...prev];
          });
        }
      })
      .catch((e) => {
        console.warn("Failed to load chat history:", e);
      });
  }, [agentId]);

  // Auto-focus input
  useEffect(() => {
    inputRef.current?.focus();
  }, []);

  // ── Send message ─────────────────────────────────────────────
  function sendMessage() {
    const text = input.trim();
    if (!text || !wsRef.current || wsRef.current.readyState !== WebSocket.OPEN)
      return;

    setMessages((prev) => [
      ...prev,
      { id: nextId(), role: "user", content: text, timestamp: new Date() },
    ]);
    wsRef.current.send(JSON.stringify({ type: "message", content: text }));
    setInput("");
    userScrolledUp.current = false;
    // Reset textarea height
    if (inputRef.current) inputRef.current.style.height = "auto";
  }

  function handleKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      sendMessage();
    }
  }

  const isSendDisabled =
    !input.trim() ||
    connectionStatus !== "connected" ||
    (!isAgentReady && messages.length === 0) ||
    isThinking;

  // ── No agent selected ────────────────────────────────────────
  if (!agentId) {
    return (
      <div className="flex h-full flex-col items-center justify-center text-muted-foreground">
        <Bot className="mb-4 h-12 w-12 opacity-40" />
        <p className="text-lg font-medium">No agent selected</p>
        <Link
          to="/dashboard"
          className="mt-2 text-sm text-indigo-600 hover:underline"
        >
          ← Back to Dashboard
        </Link>
      </div>
    );
  }

  // ── Render ───────────────────────────────────────────────────
  return (
    <div className="flex h-full flex-col">
      {/* ── Header ──────────────────────────────────────────── */}
      <div className="flex items-center gap-3 border-b border-border bg-white px-4 py-3">
        <Link
          to="/dashboard"
          className="flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground transition-colors"
        >
          <ArrowLeft className="h-4 w-4" />
          <span className="hidden sm:inline">Back</span>
        </Link>

        <div className="h-5 w-px bg-border" />

        <div className="flex items-center gap-2 min-w-0 flex-1">
          <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-indigo-100">
            <Bot className="h-4 w-4 text-indigo-600" />
          </div>
          <span
            className="truncate text-sm font-medium text-foreground"
            title={agentId}
          >
            Agent {agentId.length > 12 ? `${agentId.slice(0, 12)}…` : agentId}
          </span>
        </div>

        <div className="flex items-center gap-1.5">
          <span
            className={cn(
              "h-2.5 w-2.5 rounded-full",
              STATUS_DOT[connectionStatus],
            )}
          />
          <span className="text-xs text-muted-foreground">
            {STATUS_LABEL[connectionStatus]}
          </span>
        </div>
      </div>

      {/* ── Messages area ───────────────────────────────────── */}
      <div
        ref={scrollContainerRef}
        onScroll={handleScroll}
        className="flex-1 overflow-y-auto bg-[#f8fafc] px-4 py-6"
      >
        <div className="mx-auto max-w-2xl space-y-4">
          {messages.map((msg) => (
            <MessageBubble key={msg.id} message={msg} />
          ))}

          {/* Typing indicator while thinking */}
          {isThinking && (
            <div className="flex items-start gap-2">
              <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-indigo-100">
                <Bot className="h-3.5 w-3.5 text-indigo-600" />
              </div>
              <div className="rounded-2xl rounded-tl-sm border border-border bg-white px-4 py-2.5">
                <span className="inline-flex gap-1">
                  <span className="h-2 w-2 animate-bounce rounded-full bg-gray-400 [animation-delay:0ms]" />
                  <span className="h-2 w-2 animate-bounce rounded-full bg-gray-400 [animation-delay:150ms]" />
                  <span className="h-2 w-2 animate-bounce rounded-full bg-gray-400 [animation-delay:300ms]" />
                </span>
              </div>
            </div>
          )}

          {/* Agent waking state */}
          {connectionStatus === "connected" &&
            !isAgentReady &&
            messages.length === 0 && (
              <div className="flex flex-col items-center justify-center py-12 text-muted-foreground">
                <Loader2 className="mb-3 h-8 w-8 animate-spin text-indigo-400" />
                <p className="text-sm">Agent waking up…</p>
              </div>
            )}

          {/* Agent ready, no messages yet */}
          {connectionStatus === "connected" &&
            isAgentReady &&
            messages.filter((m) => m.role !== "system").length === 0 && (
              <div className="flex flex-col items-center justify-center py-12 text-muted-foreground">
                <Bot className="mb-3 h-10 w-10 text-indigo-400" />
                <p className="font-medium text-foreground">Agent is ready!</p>
                <p className="mt-1 text-sm">Send a message to get started.</p>
              </div>
            )}

          <div ref={messagesEndRef} />
        </div>
      </div>

      {/* ── Input bar ───────────────────────────────────────── */}
      <div className="border-t border-border bg-white px-4 py-3">
        <div className="mx-auto flex max-w-2xl items-end gap-2">
          <textarea
            ref={inputRef}
            value={input}
            onChange={(e) => {
              setInput(e.target.value);
              // Auto-resize
              e.target.style.height = "auto";
              e.target.style.height = `${Math.min(e.target.scrollHeight, 160)}px`;
            }}
            onKeyDown={handleKeyDown}
            placeholder="Type a message…"
            rows={1}
            className={cn(
              "flex-1 resize-none rounded-xl border border-border bg-white px-4 py-2.5 text-sm",
              "placeholder:text-muted-foreground focus:border-indigo-400 focus:outline-none focus:ring-2 focus:ring-indigo-400/20",
              "disabled:cursor-not-allowed disabled:opacity-50",
            )}
            disabled={connectionStatus !== "connected"}
          />
          <button
            type="button"
            onClick={sendMessage}
            disabled={isSendDisabled}
            className={cn(
              "flex h-10 w-10 shrink-0 items-center justify-center rounded-full transition-colors",
              isSendDisabled
                ? "bg-gray-200 text-gray-400 cursor-not-allowed"
                : "bg-indigo-600 text-white hover:bg-indigo-700 active:bg-indigo-800",
            )}
          >
            <Send className="h-4 w-4" />
          </button>
        </div>
      </div>
    </div>
  );
}

// ── Message bubble component ───────────────────────────────────
function MessageBubble({ message }: { message: ChatMessage }) {
  const { role, content, timestamp, isStreaming } = message;

  if (role === "system") {
    return (
      <div className="flex justify-center">
        <span className="text-xs italic text-muted-foreground">{content}</span>
      </div>
    );
  }

  if (role === "user") {
    return (
      <div className="flex justify-end">
        <div className="max-w-[75%]">
          <div className="rounded-2xl rounded-tr-sm bg-indigo-600 px-4 py-2.5 text-sm text-white whitespace-pre-wrap">
            {content}
          </div>
          <p className="mt-1 text-right text-[10px] text-muted-foreground">
            {formatTime(timestamp)}
          </p>
        </div>
      </div>
    );
  }

  // Agent message
  return (
    <div className="flex items-start gap-2">
      <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-indigo-100">
        <Bot className="h-3.5 w-3.5 text-indigo-600" />
      </div>
      <div className="max-w-[75%]">
        <div className="rounded-2xl rounded-tl-sm border border-border bg-white px-4 py-2.5 text-sm text-foreground whitespace-pre-wrap">
          {content}
          {isStreaming && (
            <span className="ml-0.5 inline-block h-4 w-1.5 animate-pulse bg-indigo-500 align-text-bottom" />
          )}
        </div>
        <p className="mt-1 text-[10px] text-muted-foreground">
          {formatTime(timestamp)}
        </p>
      </div>
    </div>
  );
}
