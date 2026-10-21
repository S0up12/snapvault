import { invokeLogged } from "../lib/ipc";
import { useCallback, useEffect, useRef, useState } from "react";

export type ChatThread = {
  id: string;
  external_id: string;
  display_name: string;
  is_group: boolean;
  message_count: number;
  latest_at: string | null;
  latest_preview: string;
};

export type ChatMessageMedia = {
  id: string;
  media_type: "image" | "video" | "audio";
  original_path: string;
  overlay_path: string | null;
  thumbnail_path: string | null;
  playback_path: string | null;
};

export type ChatMessage = {
  id: string;
  sender: string;
  sender_label: string;
  is_me: boolean;
  body: string | null;
  sent_at: string;
  message_type: string;
  media: ChatMessageMedia[];
};

export function useChatThreads() {
  const [threads, setThreads] = useState<ChatThread[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invokeLogged<ChatThread[]>("list_chat_threads")
      .then(setThreads)
      .catch((err) => setError(String(err)))
      .finally(() => setIsLoading(false));
  }, []);

  return { threads, isLoading, error };
}

type ChatMessagePage = {
  messages: ChatMessage[];
  has_more: boolean;
  next_before: string | null;
  has_more_after?: boolean;
  next_after?: string | null;
};

// aroundId: load a window centered on that message instead of the newest page.
// The caller remounts (key) to re-run a jump; the effect also re-runs on change.
export function useChatMessages(threadId: string | null, aroundId: string | null = null) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [hasMore, setHasMore] = useState(false);
  const [hasMoreAfter, setHasMoreAfter] = useState(false);
  const [isLoading, setIsLoading] = useState(false);
  const [isLoadingOlder, setIsLoadingOlder] = useState(false);
  const [isLoadingNewer, setIsLoadingNewer] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const nextBefore = useRef<string | null>(null);
  const nextAfter = useRef<string | null>(null);
  // Bumped on every thread change; responses from an older generation are dropped.
  const generation = useRef(0);
  const busy = useRef(false);

  useEffect(() => {
    const gen = ++generation.current;
    busy.current = false;
    setMessages([]);
    setHasMore(false);
    setHasMoreAfter(false);
    setIsLoadingOlder(false);
    setIsLoadingNewer(false);
    setError(null);
    nextBefore.current = null;
    nextAfter.current = null;
    if (!threadId) {
      setIsLoading(false);
      return;
    }
    setIsLoading(true);
    invokeLogged<ChatMessagePage>("list_chat_messages", aroundId ? { threadId, around: aroundId } : { threadId })
      .then((page) => {
        if (gen !== generation.current) return;
        setMessages(page.messages);
        setHasMore(page.has_more);
        setHasMoreAfter(page.has_more_after ?? false);
        nextBefore.current = page.next_before;
        nextAfter.current = page.next_after ?? null;
      })
      .catch((err) => gen === generation.current && setError(String(err)))
      .finally(() => gen === generation.current && setIsLoading(false));
  }, [threadId, aroundId]);

  const loadOlder = useCallback(async () => {
    if (!threadId || busy.current || !nextBefore.current) return;
    const gen = generation.current;
    busy.current = true;
    setIsLoadingOlder(true);
    try {
      const page = await invokeLogged<ChatMessagePage>("list_chat_messages", {
        threadId,
        before: nextBefore.current,
      });
      if (gen !== generation.current) return;
      setMessages((prev) => {
        const seen = new Set(prev.map((m) => m.id));
        return [...page.messages.filter((m) => !seen.has(m.id)), ...prev];
      });
      setHasMore(page.has_more);
      nextBefore.current = page.next_before;
    } catch (err) {
      if (gen === generation.current) setError(String(err));
    } finally {
      if (gen === generation.current) {
        busy.current = false;
        setIsLoadingOlder(false);
      }
    }
  }, [threadId]);

  const loadNewer = useCallback(async () => {
    if (!threadId || busy.current || !nextAfter.current) return;
    const gen = generation.current;
    busy.current = true;
    setIsLoadingNewer(true);
    try {
      const page = await invokeLogged<ChatMessagePage>("list_chat_messages", {
        threadId,
        after: nextAfter.current,
      });
      if (gen !== generation.current) return;
      setMessages((prev) => {
        const seen = new Set(prev.map((m) => m.id));
        return [...prev, ...page.messages.filter((m) => !seen.has(m.id))];
      });
      setHasMoreAfter(page.has_more_after ?? false);
      nextAfter.current = page.next_after ?? null;
    } catch (err) {
      if (gen === generation.current) setError(String(err));
    } finally {
      if (gen === generation.current) {
        busy.current = false;
        setIsLoadingNewer(false);
      }
    }
  }, [threadId]);

  return { messages, isLoading, isLoadingOlder, isLoadingNewer, error, hasMore, hasMoreAfter, loadOlder, loadNewer };
}

// Avatar initials from a display name, mirroring the reference app's
// text-only avatars (no profile images in a Snapchat export).
export function avatarInitials(name: string): string {
  const parts = name.trim().split(/\s+/).filter(Boolean);
  if (parts.length === 0) {
    return "?";
  }
  if (parts.length === 1) {
    return parts[0].slice(0, 2).toUpperCase();
  }
  return (parts[0][0] + parts[1][0]).toUpperCase();
}

const GROUP_TONES = [
  { label: "text-cyan-300", pole: "bg-cyan-400" },
  { label: "text-lime-300", pole: "bg-lime-400" },
  { label: "text-fuchsia-300", pole: "bg-fuchsia-400" },
  { label: "text-amber-300", pole: "bg-amber-400" },
  { label: "text-violet-300", pole: "bg-violet-400" },
  { label: "text-emerald-300", pole: "bg-emerald-400" },
];
const PRIVATE_ME_TONE = { label: "text-rose-300", pole: "bg-rose-400" };
const PRIVATE_OTHER_TONE = { label: "text-sky-300", pole: "bg-sky-400" };

function hashSenderLabel(label: string): number {
  let hash = 0;
  for (const char of label) {
    hash = (hash * 31 + char.charCodeAt(0)) | 0;
  }
  return Math.abs(hash);
}

// Picks a deterministic accent color per sender: binary me/them in a 1:1
// chat, or a hash-based rotation through fixed tones in a group.
export function senderTone(message: ChatMessage, isGroup: boolean): { label: string; pole: string } {
  if (!isGroup) {
    return message.is_me ? PRIVATE_ME_TONE : PRIVATE_OTHER_TONE;
  }
  return GROUP_TONES[hashSenderLabel(message.sender_label) % GROUP_TONES.length];
}

const AVATAR_GRADIENTS = [
  "linear-gradient(135deg,#565a86,#2b2741)",
  "linear-gradient(135deg,#6b4a5c,#2b2741)",
  "linear-gradient(135deg,#42624f,#2b2741)",
  "linear-gradient(135deg,#7d5f3c,#2b2741)",
  "linear-gradient(135deg,#3a5c6b,#2b2741)",
  "linear-gradient(135deg,#8a6446,#2b2741)",
];

function hashString(value: string): number {
  let hash = 0;
  for (const char of value) {
    hash = (hash * 31 + char.charCodeAt(0)) | 0;
  }
  return Math.abs(hash);
}

// A deterministic muted gradient per thread, so conversations stay visually
// distinct in the list without reaching for saturated per-sender colors.
export function avatarGradient(seed: string): string {
  return AVATAR_GRADIENTS[hashString(seed) % AVATAR_GRADIENTS.length];
}
