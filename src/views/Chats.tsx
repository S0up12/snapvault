import { convertFileSrc } from "@tauri-apps/api/core";
import { LoaderCircle, MessageSquareMore, Users } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import {
  avatarInitials,
  senderTone,
  useChatMessages,
  useChatThreads,
  type ChatMessage,
  type ChatMessageMedia,
  type ChatThread,
} from "../hooks/useChats";
import { formatDayGroup } from "../hooks/useMemories";

function formatMessageTime(sentAt: string): string {
  return new Date(sentAt).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

export default function Chats() {
  const { threads, isLoading, error } = useChatThreads();
  const [selectedThreadId, setSelectedThreadId] = useState<string | null>(null);
  const selectedThread = threads.find((t) => t.id === selectedThreadId) ?? null;

  useEffect(() => {
    if (!selectedThreadId && threads.length > 0) {
      setSelectedThreadId(threads[0].id);
    }
  }, [threads, selectedThreadId]);

  return (
    <section className="flex h-full min-h-0 w-full overflow-hidden rounded-[1.9rem] border border-slate-200/80 bg-white shadow-[0_28px_80px_rgba(15,23,42,0.1)] dark:border-white/10 dark:bg-white/[0.045] dark:shadow-black/25">
      <div className="flex w-[30%] min-w-[220px] flex-col gap-2 overflow-y-auto border-r border-slate-200/70 p-3 dark:border-white/10">
        {isLoading ? (
          <div className="flex flex-1 items-center justify-center text-slate-400 dark:text-slate-500">
            <LoaderCircle className="h-5 w-5 animate-spin" />
          </div>
        ) : error ? (
          <div className="rounded-[1.1rem] border border-rose-300/40 bg-rose-50 px-3 py-2 text-xs text-rose-700 dark:border-rose-400/20 dark:bg-rose-400/10 dark:text-rose-100">
            {error}
          </div>
        ) : threads.length === 0 ? (
          <p className="p-3 text-sm text-slate-400 dark:text-slate-500">No conversations found.</p>
        ) : (
          threads.map((thread) => (
            <ConversationRow
              key={thread.id}
              thread={thread}
              isSelected={thread.id === selectedThreadId}
              onSelect={() => setSelectedThreadId(thread.id)}
            />
          ))
        )}
      </div>

      <div className="flex min-h-0 flex-1 flex-col bg-[linear-gradient(180deg,rgba(248,250,252,0.92),rgba(241,245,249,0.95))] dark:bg-[linear-gradient(180deg,rgba(8,14,24,0.9),rgba(4,8,14,0.95))]">
        {selectedThread ? (
          <MessagePane thread={selectedThread} />
        ) : (
          <div className="flex flex-1 items-center justify-center text-sm text-slate-400 dark:text-slate-500">
            Select a conversation.
          </div>
        )}
      </div>
    </section>
  );
}

function ConversationRow({
  thread,
  isSelected,
  onSelect,
}: {
  thread: ChatThread;
  isSelected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onSelect}
      className={[
        "flex w-full items-center gap-3 rounded-[1.2rem] border px-3 py-3 text-left transition",
        isSelected
          ? "border-sky-300/20 bg-sky-400/[0.12] shadow-[0_18px_36px_rgba(8,47,73,0.12)] dark:text-white"
          : "border-transparent hover:border-slate-200 hover:bg-slate-50/90 dark:hover:border-white/10 dark:hover:bg-white/[0.045]",
      ].join(" ")}
    >
      <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-full bg-[radial-gradient(circle_at_top,_rgba(56,189,248,0.28),_rgba(14,165,233,0.14),_rgba(15,23,42,0.08))] text-sm font-semibold text-slate-900 dark:text-white">
        {avatarInitials(thread.display_name)}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center justify-between gap-2">
          <p className="truncate text-sm font-medium text-slate-900 dark:text-slate-100">
            {thread.display_name}
          </p>
          {thread.latest_at ? (
            <span className="shrink-0 text-[11px] text-slate-400 dark:text-slate-500">
              {formatDayGroup(thread.latest_at).shortLabel}
            </span>
          ) : null}
        </div>
        <div className="flex items-center gap-1.5">
          {thread.is_group ? <Users className="h-3 w-3 shrink-0 text-slate-400" /> : null}
          <p className="truncate text-xs text-slate-500 dark:text-slate-400">{thread.latest_preview}</p>
        </div>
      </div>
    </button>
  );
}

function MessagePane({ thread }: { thread: ChatThread }) {
  const { messages, isLoading, error } = useChatMessages(thread.id);
  const scrollRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [messages]);

  return (
    <>
      <div className="flex items-center gap-3 border-b border-slate-200/70 px-5 py-4 dark:border-white/10">
        <MessageSquareMore className="h-5 w-5 text-sky-600 dark:text-sky-300" />
        <div>
          <p className="text-sm font-semibold text-slate-950 dark:text-white">{thread.display_name}</p>
          <p className="text-xs text-slate-400 dark:text-slate-500">
            {thread.message_count.toLocaleString()} {thread.message_count === 1 ? "message" : "messages"}
          </p>
        </div>
      </div>

      <div ref={scrollRef} className="flex-1 space-y-4 overflow-y-auto px-5 py-4">
        {isLoading ? (
          <div className="flex h-full items-center justify-center text-slate-400 dark:text-slate-500">
            <LoaderCircle className="h-5 w-5 animate-spin" />
          </div>
        ) : error ? (
          <div className="rounded-[1.1rem] border border-rose-300/40 bg-rose-50 px-3 py-2 text-xs text-rose-700 dark:border-rose-400/20 dark:bg-rose-400/10 dark:text-rose-100">
            {error}
          </div>
        ) : messages.length === 0 ? (
          <p className="text-sm text-slate-400 dark:text-slate-500">No messages in this conversation.</p>
        ) : (
          messages.map((message) => (
            <MessageBubble key={message.id} message={message} isGroup={thread.is_group} />
          ))
        )}
      </div>
    </>
  );
}

function MessageBubble({ message, isGroup }: { message: ChatMessage; isGroup: boolean }) {
  const tone = senderTone(message, isGroup);
  const hasMedia = message.media.length > 0;
  const text = message.body?.trim() || (!hasMedia && message.message_type !== "TEXT" ? "Media attachment" : "");

  return (
    <div className="flex justify-start">
      <div className="max-w-[min(46rem,94%)]">
        <div className="flex items-center gap-3 px-1">
          <p className={["text-[11px] font-semibold uppercase tracking-[0.22em]", tone.label].join(" ")}>
            {message.sender_label}
          </p>
        </div>
        <div className="group mt-1.5 inline-flex items-stretch gap-3">
          <div className={["w-[3px] shrink-0 rounded-full", tone.pole].join(" ")} />
          <div className="min-w-0 flex-1 rounded-r-[1.35rem] rounded-bl-[0.45rem] border border-slate-200/80 bg-white/88 px-4 py-3 text-slate-900 shadow-sm transition duration-150 group-hover:border-slate-300 dark:border-white/10 dark:bg-slate-950/70 dark:text-slate-100">
            {hasMedia ? (
              <div className="flex flex-col gap-2">
                {message.media.map((media) => (
                  <ChatMediaAttachment key={media.id} media={media} />
                ))}
                {text ? <p>{text}</p> : null}
              </div>
            ) : (
              text
            )}
          </div>
          <div className="inline-flex min-h-[2.75rem] items-center px-1 text-[11px] font-medium uppercase tracking-[0.18em] text-slate-400 opacity-0 transition duration-150 group-hover:opacity-100">
            <span>{formatMessageTime(message.sent_at)}</span>
          </div>
        </div>
      </div>
    </div>
  );
}

function ChatMediaAttachment({ media }: { media: ChatMessageMedia }) {
  const mediaUrl = convertFileSrc(media.playback_path ?? media.original_path);
  const overlayUrl = media.overlay_path ? convertFileSrc(media.overlay_path) : null;
  const isVideo = media.media_type === "video";

  return (
    <div className="relative inline-grid max-h-64 max-w-[240px] place-items-center overflow-hidden rounded-[0.9rem] bg-black/5 dark:bg-black/20">
      {isVideo ? (
        <video
          src={mediaUrl}
          controls
          playsInline
          preload="metadata"
          className="col-start-1 row-start-1 max-h-64 max-w-[240px] rounded-[0.9rem] object-contain"
        />
      ) : (
        <img
          src={mediaUrl}
          alt=""
          className="col-start-1 row-start-1 max-h-64 max-w-[240px] rounded-[0.9rem] object-contain"
        />
      )}
      {overlayUrl ? (
        <img
          src={overlayUrl}
          alt=""
          aria-hidden="true"
          className="pointer-events-none col-start-1 row-start-1 h-full w-full rounded-[0.9rem] object-fill"
        />
      ) : null}
    </div>
  );
}
