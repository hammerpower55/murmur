// Talk to the word fixer's language model, to see what it can do. The
// conversation lives in this page only and is never saved.

import { useEffect, useRef, useState } from "react";
import { buttonClass, inputClass, primaryClass } from "../components/controls";
import { api, errorText, events, type ChatMessage } from "../lib/api";
import { useModels } from "../store/models";

type Status = "idle" | "loading" | "writing";

export function Chat({ onOpenModels }: { onOpenModels: () => void }) {
  const view = useModels((m) => m.view);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [status, setStatus] = useState<Status>("idle");
  const [error, setError] = useState<string | null>(null);
  const end = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const offs = [
      events.chatLoading(() => setStatus("loading")),
      events.chatText((text) => {
        setStatus("writing");
        setMessages((m) => {
          const last = m[m.length - 1];
          return last?.role === "assistant"
            ? [...m.slice(0, -1), { role: "assistant", content: text }]
            : [...m, { role: "assistant", content: text }];
        });
      }),
      events.chatDone((message) => {
        setStatus("idle");
        if (message) setError(message);
      }),
    ];
    return () => offs.forEach((off) => void off.then((f) => f()));
  }, []);

  useEffect(() => {
    end.current?.scrollIntoView({ block: "end" });
  }, [messages, status]);

  const installed = view?.models.some((m) => m.family === "fixup" && m.installed) ?? false;
  const busy = status !== "idle";

  const send = async () => {
    const text = draft.trim();
    if (!text || busy) return;
    const next: ChatMessage[] = [...messages, { role: "user", content: text }];
    setMessages(next);
    setDraft("");
    setError(null);
    setStatus("writing");
    try {
      await api.chatSend(next);
    } catch (err) {
      setStatus("idle");
      setError(errorText(err));
    }
  };

  if (view && !installed) {
    return (
      <section className="page">
        <h1 className="page-title">Chat</h1>
        <p className="mt-10 text-center text-sm text-zinc-500">
          <button className="text-accent hover:underline" onClick={onOpenModels}>
            Download the word fixer in Models
          </button>{" "}
          to chat with it.
        </p>
      </section>
    );
  }

  return (
    <section className="page flex h-full flex-col">
      <div className="flex items-center justify-between">
        <h1 className="page-title">Chat</h1>
        <button
          className={buttonClass}
          disabled={busy || messages.length === 0}
          onClick={() => {
            setMessages([]);
            setError(null);
          }}
        >
          New chat
        </button>
      </div>

      <div className="mt-6 flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto pb-4">
        {messages.length === 0 && (
          <p className="py-10 text-center text-sm text-zinc-500">Ask the word fixer anything to see how it thinks.</p>
        )}
        {messages.map((m, i) => (
          <div
            key={i}
            className={`max-w-[80%] animate-rise rounded-xl px-4 py-2.5 text-sm leading-relaxed whitespace-pre-wrap ${
              m.role === "user" ? "self-end bg-accent/15 text-zinc-100" : "surface self-start text-zinc-200"
            }`}
          >
            {m.content}
          </div>
        ))}
        {status === "loading" && <p className="animate-breathe self-start text-xs text-accent">Loading the model</p>}
        {status === "writing" && messages[messages.length - 1]?.role === "user" && (
          <p className="animate-breathe self-start text-xs text-accent">Writing</p>
        )}
        {error && (
          <p key={error} className="animate-shake self-start text-sm text-red-300">
            {error}
          </p>
        )}
        <div ref={end} />
      </div>

      <form
        className="flex gap-3 border-t border-white/6 pt-4"
        onSubmit={(e) => {
          e.preventDefault();
          void send();
        }}
      >
        <input
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="Message the word fixer"
          className={`${inputClass} flex-1`}
        />
        {busy ? (
          <button type="button" className={buttonClass} onClick={() => void api.chatStop()}>
            Stop
          </button>
        ) : (
          <button type="submit" className={primaryClass} disabled={!draft.trim()}>
            Send
          </button>
        )}
      </form>
    </section>
  );
}
