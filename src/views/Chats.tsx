import { convertFileSrc } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Clapperboard, LoaderCircle, Play, Search, Users, X } from "lucide-react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { Fragment, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useOutletContext } from "react-router-dom";

import ExportDialog from "../components/ExportDialog";
import Lightbox,{ type LightboxAsset } from "../components/Lightbox";
import type { LayoutOutletContext } from "../components/Layout";
import {
  avatarGradient,
  avatarInitials,
  senderTone,
  useChatMessages,
  useChatThreads,
  type ChatMessage,
  type ChatMessageMedia,
  type ChatThread,
} from "../hooks/useChats";
import { useChatSearch, type ChatSearchHit } from "../hooks/useChatSearch";
import { formatDayGroup } from "../hooks/useMemories";

function formatMessageTime(sentAt: string): string {
  return new Date(sentAt).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
}

const URL_PATTERN = /https?:\/\/\S+/g;
// A trailing char is part of the URL only if it can't plausibly be sentence
// punctuation wrapping it (a period ending the sentence, a closing paren
// around the link, etc.) - not perfect, but covers the common cases.
const TRAILING_PUNCTUATION = /[.,!?:;'")\]]+$/;

// Splits message text on bare URLs and renders each as a clickable link that
// opens in the system browser - the in-app webview has no back/forward/tabs,
// so navigating it away from the chat view would be a dead end.
function Linkified({ text }: { text: string }) {
  const nodes: React.ReactNode[] = [];
  let lastIndex = 0;
  let key = 0;

  for (const match of text.matchAll(URL_PATTERN)) {
    const raw = match[0];
    const trailingMatch = raw.match(TRAILING_PUNCTUATION);
    const trailing = trailingMatch ? trailingMatch[0] : "";
    const url = trailing ? raw.slice(0, -trailing.length) : raw;
    if (!url) {
      continue;
    }

    nodes.push(<Fragment key={key++}>{text.slice(lastIndex, match.index)}</Fragment>);
    nodes.push(
      <a
        key={key++}
        href={url}
        onClick={(event) => {
          event.preventDefault();
          void openUrl(url);
        }}
        className="text-accent-300 underline decoration-accent-300/40 underline-offset-2 hover:decoration-accent-300"
      >
        {url}
      </a>,
    );
    if (trailing) {
      nodes.push(<Fragment key={key++}>{trailing}</Fragment>);
    }
    lastIndex = (match.index ?? 0) + raw.length;
  }
  nodes.push(<Fragment key={key++}>{text.slice(lastIndex)}</Fragment>);

  return <>{nodes}</>;
}

// Snippet match markers: U+0001 starts and U+0002 ends a match, so splitting on
// either yields alternating plain / matched pieces.
function Snippet({ text }: { text: string }) {
  return (
    <>
      {text.split(/[\u0001\u0002]/).map((part, i) => (i % 2 === 1 ? <mark key={i}>{part}</mark> : part))}
    </>
  );
}

type Jump = { id: string; n: number };

export default function Chats() {
  const { chatsFilters } = useOutletContext<LayoutOutletContext>();
  const { threads, isLoading, error } = useChatThreads();
  const [selectedThreadId, setSelectedThreadId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [jump, setJump] = useState<Jump | null>(null);
  const [thisChatOnly, setThisChatOnly] = useState(false);
  const [exportTarget, setExportTarget] = useState<"this" | "all" | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const search = useChatSearch(thisChatOnly ? selectedThreadId : null);
  const selectedThread = threads.find((t) => t.id === selectedThreadId) ?? null;

  const filteredThreads = useMemo(
    () =>
      threads
        .filter((thread) => thread.display_name.toLowerCase().includes(query.trim().toLowerCase()))
        .filter((thread) => {
          if (chatsFilters.scope === "group") return thread.is_group;
          if (chatsFilters.scope === "private") return !thread.is_group;
          return true;
        }),
    [threads, query, chatsFilters.scope],
  );

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const el = event.target as HTMLElement | null;
      const typing = !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
      const isCtrlK = (event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k";
      if (isCtrlK || (event.key === "/" && !typing)) {
        event.preventDefault();
        searchRef.current?.focus();
        searchRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (!selectedThreadId && threads.length > 0) {
      setSelectedThreadId(threads[0].id);
    }
  }, [threads, selectedThreadId]);

  return (
    <section className="flex h-full min-h-0 w-full overflow-hidden rounded-lg ring-1 ring-divider">
      <div className="flex w-[320px] min-w-[220px] shrink-0 flex-col border-r border-divider p-3.5">
        <div className="relative mb-2.5">
          <Search className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-neutral-500" />
          <input
            ref={searchRef}
            type="text"
            value={search.query}
            onChange={(event) => search.setQuery(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape") search.setQuery("");
            }}
            placeholder="Search messages"
            className="input h-9 px-9 text-[13px]"
          />
          {search.query ? (
            <button
              type="button"
              aria-label="Clear search"
              onClick={() => search.setQuery("")}
              className="absolute right-2.5 top-1/2 -translate-y-1/2 text-neutral-500 hover:text-neutral-300"
            >
              <X className="h-3.5 w-3.5" />
            </button>
          ) : null}
        </div>
        <div className="mb-2.5 flex gap-2">
          {selectedThread ? (
            <button type="button" onClick={() => setExportTarget("this")} className="btn btn-secondary flex-1">
              Export this chat
            </button>
          ) : null}
          <button type="button" onClick={() => setExportTarget("all")} className="btn btn-secondary flex-1">
            Export all chats
          </button>
        </div>
        {exportTarget ? (
          <ExportDialog
            kind="chats"
            threadId={exportTarget === "this" && selectedThread ? selectedThread.id : null}
            label={exportTarget === "this" && selectedThread ? selectedThread.display_name : "all chats"}
            onClose={() => setExportTarget(null)}
          />
        ) : null}
        {selectedThread ? (
          <label className="mb-2.5 flex items-center gap-2 text-xs text-neutral-400">
            <input type="checkbox" checked={thisChatOnly} onChange={(event) => setThisChatOnly(event.target.checked)} />
            This chat only
          </label>
        ) : null}
        {search.active ? (
          <SearchResults
            search={search}
            onOpen={(hit) => {
              setSelectedThreadId(hit.thread_id);
              setJump((j) => ({ id: hit.message_id, n: (j?.n ?? 0) + 1 }));
            }}
          />
        ) : (
        <>
        <div className="relative mb-2.5">
          <Search className="pointer-events-none absolute left-3 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-neutral-500" />
          <input
            type="text"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="Search people…"
            className="input h-9 pl-9 text-[13px]"
          />
        </div>
        <div className="flex flex-col gap-0.5 overflow-y-auto">
          {isLoading ? (
            <div className="flex flex-1 items-center justify-center py-8 text-neutral-500">
              <LoaderCircle className="h-5 w-5 animate-spin" />
            </div>
          ) : error ? (
            <div className="rounded-md bg-red-500/8 px-3 py-2 text-xs text-red-300 ring-1 ring-red-400/25">{error}</div>
          ) : filteredThreads.length === 0 ? (
            <p className="p-3 text-sm text-neutral-500">No conversations found.</p>
          ) : (
            filteredThreads.map((thread) => (
              <ConversationRow
                key={thread.id}
                thread={thread}
                isSelected={thread.id === selectedThreadId}
                onSelect={() => {
                  setSelectedThreadId(thread.id);
                  setJump(null);
                }}
              />
            ))
          )}
        </div>
        </>
        )}
      </div>

      <div className="flex min-h-0 flex-1 flex-col">
        {selectedThread ? (
          <MessagePane
            key={`${selectedThread.id}:${jump?.n ?? 0}`}
            thread={selectedThread}
            jumpId={jump?.id ?? null}
            onJumpLatest={() => setJump(null)}
          />
        ) : (
          <div className="flex flex-1 items-center justify-center text-sm text-neutral-500">
            Select a conversation.
          </div>
        )}
      </div>
    </section>
  );
}

function SearchResults({
  search,
  onOpen,
}: {
  search: ReturnType<typeof useChatSearch>;
  onOpen: (hit: ChatSearchHit) => void;
}) {
  const { hits, total, totalCapped, hasMore, isLoading, error, loadMore } = search;
  return (
    <div className="flex flex-col gap-0.5 overflow-y-auto">
      {error ? (
        <div className="rounded-md bg-red-500/8 px-3 py-2 text-xs text-red-300 ring-1 ring-red-400/25">{error}</div>
      ) : null}
      {hits.length > 0 ? (
        <p className="px-1 pb-1 text-[11px] text-neutral-500">
          {total.toLocaleString()}
          {totalCapped ? "+" : ""} {total === 1 && !totalCapped ? "result" : "results"}
        </p>
      ) : null}
      {hits.map((hit) => {
        const title = hit.thread_title ?? "Unknown";
        return (
          <button
            key={hit.message_id}
            type="button"
            onClick={() => onOpen(hit)}
            className="flex w-full items-start gap-3 rounded-[10px] px-3 py-2.5 text-left transition hover:bg-white/[0.03]"
          >
            <div
              className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-[13px] font-semibold text-white"
              style={{ background: avatarGradient(hit.thread_id) }}
            >
              {avatarInitials(title)}
            </div>
            <div className="min-w-0 flex-1">
              <div className="flex items-center justify-between gap-2">
                <p className="truncate text-sm text-neutral-300">{title}</p>
                <span className="shrink-0 text-[11px] text-neutral-500">{formatDayGroup(hit.sent_at).shortLabel}</span>
              </div>
              <p className="text-[11px] text-neutral-500">{hit.sender_label}</p>
              <p className="line-clamp-2 text-xs text-neutral-400">
                <Snippet text={hit.snippet} />
              </p>
            </div>
          </button>
        );
      })}
      {isLoading ? (
        <div className="flex justify-center py-4 text-neutral-500">
          <LoaderCircle className="h-4 w-4 animate-spin" />
        </div>
      ) : null}
      {!isLoading && !error && hits.length === 0 ? <p className="p-3 text-sm text-neutral-500">No messages match</p> : null}
      {hasMore && !isLoading ? (
        <button type="button" onClick={() => void loadMore()} className="p-2 text-xs text-accent-300 hover:underline">
          Show more
        </button>
      ) : null}
    </div>
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
        "flex w-full items-center gap-3 rounded-[10px] px-3 py-2.5 text-left transition",
        isSelected ? "bg-accent/12 shadow-[inset_2px_0_0_var(--color-accent)]" : "hover:bg-white/[0.03]",
      ].join(" ")}
    >
      <div
        className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-[13px] font-semibold text-white"
        style={{ background: avatarGradient(thread.id) }}
      >
        {avatarInitials(thread.display_name)}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-center justify-between gap-2">
          <p className={["truncate text-sm", isSelected ? "text-fg" : "text-neutral-300"].join(" ")}>
            {thread.display_name}
          </p>
          {thread.latest_at ? (
            <span className="shrink-0 text-[11px] text-neutral-500">
              {formatDayGroup(thread.latest_at).shortLabel}
            </span>
          ) : null}
        </div>
        <div className="mt-0.5 flex items-center gap-1.5">
          {thread.is_group ? <Users className="h-3 w-3 shrink-0 text-neutral-500" /> : null}
          <p className="truncate text-xs text-neutral-500">{thread.latest_preview}</p>
        </div>
      </div>
    </button>
  );
}

function MessagePane({
  thread,
  jumpId,
  onJumpLatest,
}: {
  thread: ChatThread;
  jumpId: string | null;
  onJumpLatest: () => void;
}) {
  const { messages, isLoading, isLoadingOlder, isLoadingNewer, error, hasMore, hasMoreAfter, loadOlder, loadNewer } =
    useChatMessages(thread.id, jumpId);
  const [highlightId, setHighlightId] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const [openMediaId, setOpenMediaId] = useState<string | null>(null);

  // Flattened in conversation order so the Lightbox's prev/next arrows browse
  // every photo/video in the thread, the same way Memories browses the grid -
  // not just the attachments in one message.
  const mediaAssets = useMemo<LightboxAsset[]>(
    () =>
      messages.flatMap((message) =>
        message.media
          .filter((media) => media.media_type !== "audio")
          .map((media) => ({
            id: media.id,
            media_type: media.media_type,
            original_path: media.original_path,
            overlay_path: media.overlay_path,
            playback_path: media.playback_path,
            taken_at: message.sent_at,
          })),
      ),
    [messages],
  );
  const openIndex = openMediaId ? mediaAssets.findIndex((asset) => asset.id === openMediaId) : -1;

  const virtualizer = useVirtualizer({
    count: messages.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => 90,
    overscan: 8,
    getItemKey: (index) => messages[index].id,
  });
  const virtualItems = virtualizer.getVirtualItems();
  const firstVisible = virtualItems[0]?.index ?? 0;
  const lastVisible = virtualItems[virtualItems.length - 1]?.index ?? 0;

  const prev = useRef({ firstId: "", count: 0 });
  const didInitialScroll = useRef(false);
  const rafId = useRef(0);
  const [ready, setReady] = useState(false);

  // Re-issues a scrollToIndex every frame for up to ~1s (or until the user
  // takes over) so it converges as rows get measured and estimates shrink/grow.
  const pinScroll = (index: number, align: "start" | "end" | "center", onDone?: () => void) => {
    cancelAnimationFrame(rafId.current);
    const el = scrollRef.current;
    const t0 = performance.now();
    let stable = 0;
    let interrupted = false;
    const stop = () => {
      interrupted = true;
    };
    el?.addEventListener("wheel", stop, { once: true });
    el?.addEventListener("pointerdown", stop, { once: true });
    const tick = () => {
      if (interrupted || !el) return finish();
      virtualizer.scrollToIndex(index, { align });
      stable = el.scrollHeight === lastHeight ? stable + 1 : 0;
      lastHeight = el.scrollHeight;
      if (stable >= 10 || performance.now() - t0 > 1000) return finish();
      rafId.current = requestAnimationFrame(tick);
    };
    let lastHeight = -1;
    const finish = () => {
      el?.removeEventListener("wheel", stop);
      el?.removeEventListener("pointerdown", stop);
      onDone?.();
    };
    tick();
  };

  useLayoutEffect(() => {
    const p = prev.current;
    if (messages.length > 0) {
      if (!didInitialScroll.current) {
        didInitialScroll.current = true;
        const target = jumpId ? messages.findIndex((m) => m.id === jumpId) : -1;
        if (target >= 0) {
          // Jump mode: center the target, no pin-to-bottom, then flash it.
          pinScroll(target, "center", () => {
            setReady(true);
            setHighlightId(jumpId);
          });
        } else {
          pinScroll(messages.length - 1, "end", () => setReady(true));
        }
      } else if (messages[0].id !== p.firstId && p.count > 0 && messages.length > p.count) {
        // Older page prepended: anchor on the message that was first before.
        pinScroll(messages.length - p.count, "start");
      }
    }
    prev.current = { firstId: messages[0]?.id ?? "", count: messages.length };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [messages]);

  useEffect(() => () => cancelAnimationFrame(rafId.current), []);

  useEffect(() => {
    if (!highlightId) return;
    const timer = setTimeout(() => setHighlightId(null), 2000);
    return () => clearTimeout(timer);
  }, [highlightId]);

  useEffect(() => {
    if (ready && hasMoreAfter && !isLoadingNewer && lastVisible >= messages.length - 5) {
      void loadNewer();
    }
  }, [ready, lastVisible, hasMoreAfter, isLoadingNewer, loadNewer, messages.length]);

  useEffect(() => {
    if (ready && hasMore && !isLoadingOlder && firstVisible < 5) {
      void loadOlder();
    }
  }, [ready, firstVisible, hasMore, isLoadingOlder, loadOlder, messages.length]);

  return (
    <>
      <div className="flex items-center gap-3 border-b border-divider px-6 py-4">
        <div
          className="flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-[13px] font-semibold text-white"
          style={{ background: avatarGradient(thread.id) }}
        >
          {avatarInitials(thread.display_name)}
        </div>
        <div>
          <p className="text-[15px]">{thread.display_name}</p>
          <p className="text-[11.5px] text-neutral-500">
            {thread.message_count.toLocaleString()} {thread.message_count === 1 ? "message" : "messages"}
          </p>
        </div>
      </div>

      {hasMoreAfter ? (
        <div className="flex justify-center border-b border-divider py-1.5">
          <button type="button" onClick={onJumpLatest} className="text-xs text-accent-300 hover:underline">
            Jump to latest
          </button>
        </div>
      ) : null}

      <div ref={scrollRef} className="flex-1 overflow-y-auto px-6 py-5">
        {isLoading ? (
          <div className="flex h-full items-center justify-center text-neutral-500">
            <LoaderCircle className="h-5 w-5 animate-spin" />
          </div>
        ) : error && messages.length === 0 ? (
          <div className="rounded-md bg-red-500/8 px-3 py-2 text-xs text-red-300 ring-1 ring-red-400/25">{error}</div>
        ) : messages.length === 0 ? (
          <p className="text-sm text-neutral-500">No messages in this conversation.</p>
        ) : (
          <>
            {isLoadingOlder ? (
              <div className="flex justify-center pb-3 text-neutral-500">
                <LoaderCircle className="h-4 w-4 animate-spin" />
              </div>
            ) : null}
            <div className="relative w-full" style={{ height: virtualizer.getTotalSize() }}>
              {virtualItems.map((item) => (
                <div
                  key={item.key}
                  data-index={item.index}
                  ref={virtualizer.measureElement}
                  className={["absolute left-0 top-0 w-full pb-4.5", messages[item.index].id === highlightId ? "chat-highlight" : ""].join(" ")}
                  style={{ transform: `translateY(${item.start}px)` }}
                >
                  <MessageBubble message={messages[item.index]} isGroup={thread.is_group} onOpenMedia={setOpenMediaId} />
                </div>
              ))}
            </div>
            {error ? (
              <div className="mt-2 rounded-md bg-red-500/8 px-3 py-2 text-xs text-red-300 ring-1 ring-red-400/25">{error}</div>
            ) : null}
          </>
        )}
      </div>

      {openIndex >= 0 ? (
        <Lightbox
          assets={mediaAssets}
          currentIndex={openIndex}
          onClose={() => setOpenMediaId(null)}
          onNavigate={(nextIndex) => {
            if (nextIndex >= 0 && nextIndex < mediaAssets.length) {
              setOpenMediaId(mediaAssets[nextIndex].id);
            }
          }}
        />
      ) : null}
    </>
  );
}

function MessageBubble({
  message,
  isGroup,
  onOpenMedia,
}: {
  message: ChatMessage;
  isGroup: boolean;
  onOpenMedia: (mediaId: string) => void;
}) {
  const tone = senderTone(message, isGroup);
  const hasMedia = message.media.length > 0;
  const text = message.body?.trim() || (!hasMedia && message.message_type !== "TEXT" ? "Media attachment" : "");

  return (
    <div className="flex justify-start">
      <div className="max-w-[min(46rem,94%)]">
        <div className={["px-0.5 text-[10.5px] font-semibold uppercase tracking-[0.2em]", tone.label].join(" ")}>
          {message.sender_label}
        </div>
        <div className="group mt-1.5 inline-flex items-stretch gap-2.5">
          <div className={["w-[3px] shrink-0 rounded-full", tone.pole].join(" ")} />
          <div className="min-w-0 flex-1 rounded-[4px_12px_12px_12px] bg-surface px-4 py-2.75 text-[14px] ring-1 ring-divider">
            {hasMedia ? (
              <div className="flex flex-col gap-2">
                {message.media.map((media) => (
                  <ChatMediaAttachment key={media.id} media={media} onOpen={onOpenMedia} />
                ))}
                {text ? (
                  <p>
                    <Linkified text={text} />
                  </p>
                ) : null}
              </div>
            ) : (
              <Linkified text={text} />
            )}
          </div>
          <div className="inline-flex min-h-11 items-center px-1 text-[11px] uppercase tracking-[0.18em] text-neutral-500 opacity-0 transition duration-150 group-hover:opacity-100">
            <span>{formatMessageTime(message.sent_at)}</span>
          </div>
        </div>
      </div>
    </div>
  );
}

function ChatMediaAttachment({ media, onOpen }: { media: ChatMessageMedia; onOpen: (mediaId: string) => void }) {
  const isVideo = media.media_type === "video";

  // Snapchat voice notes are audio-only .mp4 files (backend reclassifies
  // them from "video" once ffprobe finds no video stream) - rendering them
  // through the image branch below produces a broken, invisible <img>. No
  // Lightbox affordance here: a bigger view of the same inline audio player
  // has nothing more to show.
  if (media.media_type === "audio") {
    const mediaUrl = convertFileSrc(media.playback_path ?? media.original_path);
    // Wider than the 240px photo/video cap - that width leaves almost no
    // room to scrub, which matters once a voice message runs more than a
    // few seconds.
    return <audio src={mediaUrl} controls preload="metadata" className="h-10 w-80 max-w-full" />;
  }

  // A static thumbnail, not an inline player: the whole bubble is one click
  // target that opens the Lightbox, and only one <video>/<audio> element (the
  // Lightbox's) ever exists at a time - previously an inline <video controls>
  // playing here plus the Lightbox playing the same clip meant two players
  // running at once and double audio.
  const overlayUrl = media.overlay_path ? convertFileSrc(media.overlay_path) : null;
  // original_path is only a valid <img> source for images - never fall back
  // to it for a video with no thumbnail yet, or the raw video file renders as
  // a broken image the same way voice notes used to.
  const thumbnailUrl = media.thumbnail_path
    ? convertFileSrc(media.thumbnail_path)
    : isVideo
      ? null
      : convertFileSrc(media.original_path);

  return (
    <button
      type="button"
      onClick={() => onOpen(media.id)}
      title="View"
      className="group relative inline-grid max-h-64 max-w-[240px] cursor-zoom-in place-items-center overflow-hidden rounded-md bg-black/20 transition hover:brightness-95"
    >
      {thumbnailUrl ? (
        <img src={thumbnailUrl} alt="" className="col-start-1 row-start-1 max-h-64 max-w-[240px] object-contain" />
      ) : (
        <div className="col-start-1 row-start-1 flex h-40 w-40 items-center justify-center text-neutral-600">
          <Clapperboard className="h-8 w-8" />
        </div>
      )}
      {overlayUrl ? (
        <img
          src={overlayUrl}
          alt=""
          aria-hidden="true"
          className="pointer-events-none col-start-1 row-start-1 h-full w-full object-fill"
        />
      ) : null}
      {isVideo ? (
        <span className="pointer-events-none col-start-1 row-start-1 z-10 flex h-11 w-11 items-center justify-center rounded-full bg-black/55 text-white">
          <Play className="h-5 w-5 fill-white" />
        </span>
      ) : null}
    </button>
  );
}
