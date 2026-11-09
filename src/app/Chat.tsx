// Talk to the word fixer's language model, to see what it can do. The
// conversation lives in this page only and is never saved. It is set like a
// printed interview: who is speaking in the margin, what they said on the axis.

import { useEffect, useRef, useState } from "react";
import { buttonClass, inputClass, linkClass, primaryClass } from "../components/controls";
import { Empty } from "../components/Empty";
import { Hang, Stop, Title } from "../components/Page";
import { api, errorText, events, type ChatMessage } from "../lib/api";
import { useModels } from "../store/models";

type Status = "idle" | "loading" | "writing";

const TURN = "grid grid-cols-12 gap-x-4 py-3";

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
        <Title seed={18}>Chat</Title>
        <Hang>
          <p className="statement max-w-[28ch] py-3">
            <button className={linkClass} style={{ whiteSpace: "normal" }} onClick={onOpenModels}>
              Download the word fixer in Models
            </button>{" "}
            to chat with it
            <Stop />
          </p>
        </Hang>
      </section>
    );
  }

  return (
    <section className="page flex h-full flex-col pb-6">
      <Title
        compact
        seed={18}
        aside={
          <button
            className={linkClass}
            disabled={busy || messages.length === 0}
            onClick={() => {
              setMessages([]);
              setError(null);
            }}
          >
            New chat
          </button>
        }
      >
        Chat
      </Title>

      <div className="rows min-h-0 flex-1 overflow-y-auto border-t border-ink">
        {messages.length === 0 && (
          <div className="grid grid-cols-12 gap-x-4">
            <div className="col-span-9 col-start-4">
              <Empty>Ask the word fixer anything to see how it thinks.</Empty>
            </div>
          </div>
        )}
        {messages.map((m, i) => (
          <div key={i} className={`${TURN} animate-rise`}>
            <p className="col-span-3 font-bold">{m.role === "user" ? "You" : "Word fixer"}</p>
            <p className={`col-span-9 max-w-[68ch] text-read whitespace-pre-wrap ${m.role === "user" ? "" : "text-ink-2"}`}>
              {m.content}
            </p>
          </div>
        ))}
        {(status === "loading" || (status === "writing" && messages[messages.length - 1]?.role === "user")) && (
          <div className={TURN}>
            <p className="col-span-3 font-bold">Word fixer</p>
            <p className="col-span-9 animate-breathe text-accent-text">
              {status === "loading" ? "Loading the model" : "Writing"}
            </p>
          </div>
        )}
        {error && (
          <div className={TURN}>
            <p key={error} className="col-span-9 col-start-4 animate-shake text-danger">
              {error}
            </p>
          </div>
        )}
        <div ref={end} className="border-t-0!" />
      </div>

      <form
        className="grid grid-cols-12 items-end gap-x-4 border-t border-ink pt-3"
        onSubmit={(e) => {
          e.preventDefault();
          void send();
        }}
      >
        <label htmlFor="chat-draft" className="col-span-3 pb-1 font-bold">
          You
        </label>
        <div className="col-span-9 flex items-end gap-4">
          <input
            id="chat-draft"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder="Message the word fixer"
            className={`${inputClass} flex-1 text-read`}
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
        </div>
      </form>
    </section>
  );
}
